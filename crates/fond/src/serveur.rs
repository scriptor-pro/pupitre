// SPDX-License-Identifier: GPL-3.0-or-later
//! Serveur du processus de fond : socket Unix, plusieurs interfaces clientes, sessions
//! partagées, arrêt automatique quand plus rien ne tourne.
//!
//! Organisation : un thread central possède tout l'état (clients, sessions, tampons de
//! relecture) et traite les événements un par un, ce qui garantit l'ordre (une relecture
//! est toujours suivie exactement de la sortie qui la prolonge). Autour : un thread qui
//! accepte les connexions, deux threads par client (lecture, écriture), deux par session
//! (relais des événements, envoi de la saisie). Le thread central n'attend jamais un client :
//! chaque client a une file d'envoi bornée ; s'il ne suit plus, il est déconnecté.

use crate::chargement::{self, Rapport, Travail};
use crate::marques::{LecteurMarques, LimiteMarques, Morceau};
use crate::relecture::{ReplayBuffer, REPLAY_CAPACITY};
use crate::session::{variable_protegee, Session, SessionEvent};
use crate::ssh::{EntreesSsh, EtatSsh};
use nix::errno::Errno;
use nix::fcntl::{Flock, FlockArg};
use pupitre_i18n::{t, ta, ArgI18n};
use pupitre_proto::{
    read_frame, write_frame, ClientMsg, DemandeId, ServerMsg, SessionId, SessionInfo, VERSION,
    VERSION_MIN,
};
use pupitre_ssh::trousseau::{Coffre, CoffreMemoire, CoffreSecretService};
use std::collections::{BTreeMap, HashMap};
use std::fs::{self, DirBuilder, OpenOptions};
use std::io;
use std::net::Shutdown;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender, TrySendError};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

/// Délai d'arrêt sans client ni session, en production.
pub const ARRET_SI_INACTIF: Duration = Duration::from_secs(5);
/// Messages en attente par client (morceaux de sortie de 64 Kio au plus : 64 Mio maximum).
const FILE_ENVOI_CLIENT: usize = 1024;
/// File d'événements du thread central ; bornée pour que les producteurs ralentissent.
const FILE_EVENEMENTS: usize = 256;
const PERIODE_OCCUPATION: Duration = Duration::from_millis(250);
const PAS: Duration = Duration::from_millis(50);

type ClientId = u64;

/// `$XDG_RUNTIME_DIR/pupitre/fond.sock`, sinon `/tmp/pupitre-<uid>/fond.sock`
/// (session démarrée sans elogind, par exemple). Le dossier est créé par [`preparer_dossier`].
pub fn socket_path() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    {
        Some(d) => d.join("pupitre").join("fond.sock"),
        None => PathBuf::from(format!("/tmp/pupitre-{}", nix::unistd::getuid())).join("fond.sock"),
    }
}

/// Crée le dossier du socket en `0700` s'il n'existe pas, puis vérifie qu'il est bien
/// un dossier (pas un lien), qu'il appartient à l'utilisateur et que personne d'autre
/// n'y a accès. Refuse sinon : sous `/tmp`, un autre compte pourrait l'avoir préparé.
pub fn preparer_dossier(socket: &Path) -> io::Result<()> {
    let dossier = socket.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            t("serveur-socket-sans-dossier"),
        )
    })?;
    match DirBuilder::new().mode(0o700).create(dossier) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    let m = fs::symlink_metadata(dossier)?;
    let refus = |raison: &str| {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            ta(
                "serveur-dossier-refuse",
                &[
                    ("dossier", ArgI18n::from(dossier.display().to_string())),
                    ("raison", ArgI18n::from(raison)),
                ],
            ),
        ))
    };
    if !m.is_dir() {
        return refus(&t("serveur-pas-un-dossier"));
    }
    if m.uid() != nix::unistd::getuid().as_raw() {
        return refus(&t("serveur-autre-utilisateur"));
    }
    if m.mode() & 0o077 != 0 {
        return refus(&t("serveur-droits-larges"));
    }
    Ok(())
}

/// Fait tourner le serveur sur `socket` jusqu'à ce qu'il n'y ait plus ni client ni session
/// pendant `idle_exit`. Si un autre processus de fond tient déjà ce socket, retourne
/// aussitôt `Ok(())`. Un socket abandonné (processus de fond disparu) est remplacé.
pub fn run(socket: &Path, idle_exit: Duration) -> io::Result<()> {
    run_avec_coffre(
        socket,
        idle_exit,
        EntreesSsh::depuis_env(),
        Arc::new(CoffreSecretService::ouvrir()),
    )
}

/// Dépôt des phrases de passe partagé avec les fils de chargement.
pub type CoffreCommun = Arc<dyn Coffre + Send + Sync>;

/// Comme [`run`], avec les entrées de la recherche d'agent SSH fournies (tests). Le
/// trousseau est un coffre en mémoire vide : les tests n'approchent jamais le vrai.
pub fn run_avec(socket: &Path, idle_exit: Duration, entrees: EntreesSsh) -> io::Result<()> {
    run_avec_coffre(
        socket,
        idle_exit,
        entrees,
        Arc::new(CoffreMemoire::default()),
    )
}

/// Comme [`run_avec`], avec le dépôt des phrases de passe choisi.
pub fn run_avec_coffre(
    socket: &Path,
    idle_exit: Duration,
    entrees: EntreesSsh,
    coffre: CoffreCommun,
) -> io::Result<()> {
    preparer_dossier(socket)?;
    // Verrou de démarrage : un seul processus de fond par socket, même lancés ensemble.
    let fichier_verrou = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(socket.with_extension("lock"))?;
    let _verrou = match Flock::lock(fichier_verrou, FlockArg::LockExclusiveNonblock) {
        Ok(v) => v,
        Err((_, Errno::EWOULDBLOCK)) => return Ok(()),
        Err((_, e)) => return Err(e.into()),
    };
    // Nous tenons le verrou : un socket existant est forcément abandonné.
    match fs::remove_file(socket) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let ecoute = UnixListener::bind(socket)?;
    ecoute.set_nonblocking(true)?;

    let (evt_tx, evt_rx) = mpsc::sync_channel(FILE_EVENEMENTS);
    let arret = Arc::new(AtomicBool::new(false));
    let accepteur = {
        let (evt_tx, arret) = (evt_tx.clone(), arret.clone());
        thread::Builder::new()
            .name("pupitre-accueil".into())
            .spawn(move || accepter(&ecoute, &evt_tx, &arret))?
    };
    let mut etat = Etat {
        clients: HashMap::new(),
        sessions: HashMap::new(),
        prochain_id: 1,
        demandes: BTreeMap::new(),
        prochaine_demande: 1,
        evt_tx,
        ssh: EtatSsh::avec_entrees(socket, entrees),
        coffre,
        chargement: ChargementEtat::default(),
    };
    etat.boucle(&evt_rx, idle_exit);
    arret.store(true, Ordering::SeqCst);
    let _ = accepteur.join();
    let _ = fs::remove_file(socket);
    // Les sessions restantes (aucune en arrêt normal) sont fermées en quittant `etat`.
    Ok(())
}

enum Evt {
    Arrivee(ClientId, SyncSender<ServerMsg>, UnixStream),
    Message(ClientId, ClientMsg),
    Depart(ClientId),
    Session(SessionId, SessionEvent),
    /// Fin d'un travail de chargement de clés ; le booléen dit si le travail portait sur
    /// toutes les clés.
    Chargement(Rapport, bool),
}

fn accepter(ecoute: &UnixListener, evt_tx: &SyncSender<Evt>, arret: &AtomicBool) {
    let mut prochain: ClientId = 1;
    while !arret.load(Ordering::SeqCst) {
        match ecoute.accept() {
            Ok((flux, _)) => {
                if demarrer_client(prochain, flux, evt_tx).is_ok() {
                    prochain += 1;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            // Rien à accepter, ou erreur passagère (trop de fichiers ouverts…) : on patiente.
            Err(_) => thread::sleep(PAS),
        }
    }
}

/// Lance les threads de lecture et d'écriture d'un client.
fn demarrer_client(id: ClientId, flux: UnixStream, evt_tx: &SyncSender<Evt>) -> io::Result<()> {
    flux.set_nonblocking(false)?;
    let mut ecriture = flux.try_clone()?;
    let pour_coupure = flux.try_clone()?;
    let (file_tx, file_rx) = mpsc::sync_channel::<ServerMsg>(FILE_ENVOI_CLIENT);
    thread::Builder::new()
        .name("pupitre-envoi".into())
        .spawn(move || {
            for m in file_rx {
                if write_frame(&mut ecriture, &m).is_err() {
                    break;
                }
            }
            // File fermée (client congédié) ou client parti : la lecture verra la fin.
            let _ = ecriture.shutdown(Shutdown::Both);
        })?;
    // L'arrivée est annoncée avant le premier message : même file, même ordre.
    if evt_tx
        .send(Evt::Arrivee(id, file_tx, pour_coupure))
        .is_err()
    {
        return Err(io::Error::other(t("serveur-arret")));
    }
    let evt_lecture = evt_tx.clone();
    let mut lecture = flux;
    let resultat = thread::Builder::new()
        .name("pupitre-reception".into())
        .spawn(move || {
            while let Ok(m) = read_frame::<_, ClientMsg>(&mut lecture) {
                if evt_lecture.send(Evt::Message(id, m)).is_err() {
                    return;
                }
            }
            let _ = lecture.shutdown(Shutdown::Both);
            let _ = evt_lecture.send(Evt::Depart(id));
        });
    if let Err(e) = resultat {
        // Sans thread de lecture, le client ne serait jamais retiré : on le signale tout de suite.
        let _ = evt_tx.send(Evt::Depart(id));
        return Err(e);
    }
    Ok(())
}

struct ClientEtat {
    file: SyncSender<ServerMsg>,
    flux: UnixStream,
    accueilli: bool,
    /// Version négociée au `Hello` (1 tant que le client n'est pas accueilli).
    version: u32,
    /// Vrai dès que le client a envoyé `Demander` (`pupitre-askpass`) : ce n'est pas une
    /// interface et il ne retient pas le processus de fond.
    demandeur: bool,
}

/// Demande de saisie en attente de réponse.
struct DemandeOuverte {
    demandeur: ClientId,
    /// Le `ServerMsg::Demande` à (re)envoyer aux interfaces.
    msg: ServerMsg,
}

struct SessionEtat {
    session: Arc<Session>,
    saisie: Sender<Vec<u8>>,
    relecture: ReplayBuffer,
    /// Repère les marques de commandes (OSC 133) dans la sortie.
    marques: LecteurMarques,
    /// Borne le débit des marques transmises (octets toujours relayés).
    limite_marques: LimiteMarques,
    cols: u16,
    rows: u16,
    occupe: bool,
}

struct Etat {
    clients: HashMap<ClientId, ClientEtat>,
    sessions: HashMap<SessionId, SessionEtat>,
    prochain_id: SessionId,
    demandes: BTreeMap<DemandeId, DemandeOuverte>,
    prochaine_demande: DemandeId,
    evt_tx: SyncSender<Evt>,
    ssh: EtatSsh,
    coffre: CoffreCommun,
    chargement: ChargementEtat,
}

/// Suivi du chargement des clés mémorisées (un seul fil à la fois).
#[derive(Default)]
struct ChargementEtat {
    en_cours: bool,
    /// Demandes arrivées pendant un chargement ou en attente d'un agent.
    attente: Travail,
    /// Trousseau verrouillé au dernier essai : on réessaie à la prochaine ouverture.
    reessai: bool,
    /// Le message « trousseau verrouillé » a été émis (montré ou gardé pour une interface).
    verrouille_signale: bool,
    /// Messages du chargement qu'aucune interface n'a encore pu recevoir : remis à la
    /// première interface qui parle, après la réponse à sa requête.
    messages_en_attente: Vec<String>,
}

impl Etat {
    fn boucle(&mut self, evt_rx: &Receiver<Evt>, idle_exit: Duration) {
        self.verifier_chargement();
        let mut inactif_depuis: Option<Instant> = None;
        let mut dernier_controle = Instant::now();
        loop {
            match evt_rx.recv_timeout(PAS) {
                Ok(e) => self.traiter(e),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            if dernier_controle.elapsed() >= PERIODE_OCCUPATION {
                self.controler_occupation();
                dernier_controle = Instant::now();
            }
            // Un demandeur (et donc sa demande) ne retient pas le fond à lui seul.
            let clients_utiles = self.clients.values().any(|c| !c.demandeur);
            if !clients_utiles && self.sessions.is_empty() {
                if inactif_depuis.get_or_insert_with(Instant::now).elapsed() >= idle_exit {
                    return;
                }
            } else {
                inactif_depuis = None;
            }
        }
    }

    fn traiter(&mut self, e: Evt) {
        match e {
            Evt::Arrivee(id, file, flux) => {
                self.clients.insert(
                    id,
                    ClientEtat {
                        file,
                        flux,
                        accueilli: false,
                        version: VERSION_MIN,
                        demandeur: false,
                    },
                );
            }
            Evt::Depart(id) => {
                self.retirer(id);
            }
            Evt::Message(id, m) => self.message(id, m),
            Evt::Session(sid, SessionEvent::Output(data)) => {
                // Sortie d'une session déjà fermée : ignorée.
                let Some(s) = self.sessions.get_mut(&sid) else {
                    return;
                };
                // Octets et marques repartent dans l'ordre exact du flux ; les octets
                // eux-mêmes ne sont jamais modifiés.
                let mut messages = Vec::new();
                // Octets en attente d'être envoyés : une marque ignorée ne coupe pas le
                // flux en deux messages (sinon un flot de fausses marques ferait un
                // message par marque).
                let mut octets: Vec<u8> = Vec::new();
                let maintenant = Instant::now();
                s.limite_marques.nouvelle_lecture();
                for morceau in s.marques.lire(&data, maintenant) {
                    match morceau {
                        Morceau::Octets(o) => {
                            s.relecture.push(&o);
                            octets.extend(o);
                        }
                        // Marque au-delà du débit permis : ignorée (ni diffusée, ni gardée
                        // pour la relecture), les octets continuent de passer.
                        Morceau::Marque(marque)
                            if !s.limite_marques.admettre(&marque, maintenant) => {}
                        // Copie ou notification : transmises en direct seulement (jamais
                        // gardées pour la relecture, une reprise ne recopie ni ne renotifie),
                        // dans la même limite de débit que les marques.
                        Morceau::PressePapiers(_) | Morceau::Notification { .. }
                            if !s.limite_marques.admettre_evenement(maintenant) => {}
                        Morceau::PressePapiers(texte) => {
                            envoyer_octets(&mut messages, sid, &mut octets);
                            messages.push(ServerMsg::PressePapiers { id: sid, texte });
                        }
                        Morceau::Notification { titre, corps } => {
                            envoyer_octets(&mut messages, sid, &mut octets);
                            messages.push(ServerMsg::Notification {
                                id: sid,
                                titre,
                                corps,
                            });
                        }
                        Morceau::Marque(marque) => {
                            if !octets.is_empty() {
                                let data = std::mem::take(&mut octets);
                                messages.push(ServerMsg::Output { id: sid, data });
                            }
                            s.relecture.push_marque(marque);
                            messages.push(ServerMsg::Marque {
                                id: sid,
                                marque,
                                rejouee: false,
                            });
                        }
                    }
                }
                if !octets.is_empty() {
                    messages.push(ServerMsg::Output {
                        id: sid,
                        data: octets,
                    });
                }
                for m in &messages {
                    self.diffuser(m);
                }
            }
            Evt::Chargement(rapport, toutes) => self.fin_chargement(rapport, toutes),
            Evt::Session(sid, SessionEvent::Exited(code)) => {
                if let Some(s) = self.sessions.remove(&sid) {
                    // Termine aussi ce qui tiendrait encore le terminal.
                    s.session.kill();
                }
                self.diffuser(&ServerMsg::Exited { id: sid, code });
            }
        }
    }

    fn message(&mut self, cid: ClientId, m: ClientMsg) {
        let Some(accueilli) = self.clients.get(&cid).map(|c| c.accueilli) else {
            return;
        };
        match m {
            ClientMsg::Hello { version } if !accueilli => {
                if (VERSION_MIN..=VERSION).contains(&version) {
                    let negociee = version.min(VERSION);
                    if let Some(c) = self.clients.get_mut(&cid) {
                        c.accueilli = true;
                        c.version = negociee;
                    }
                    self.envoyer(cid, ServerMsg::Welcome { version: negociee });
                    self.envoyer_demandes(cid);
                } else {
                    self.envoyer(
                        cid,
                        ServerMsg::Error(ta(
                            "serveur-version-incompatible",
                            &[
                                ("interface", ArgI18n::from(i64::from(version))),
                                ("fond", ArgI18n::from(i64::from(VERSION))),
                            ],
                        )),
                    );
                    self.congedier(cid);
                }
            }
            _ if !accueilli => {
                self.envoyer(cid, ServerMsg::Error(t("serveur-hello-attendu")));
                self.congedier(cid);
            }
            ClientMsg::Hello { .. } => self.envoyer(cid, ServerMsg::Error(t("serveur-hello-deja"))),
            // Un client qui parle sans être demandeur est une interface : elle reçoit ce
            // que le chargement n'a pu dire plus tôt (le demandeur, lui, ne reçoit rien).
            // Après la réponse à sa requête : la réponse à `List` (`Sessions`) crée la
            // première fenêtre, où le message doit apparaître.
            _ if !matches!(m, ClientMsg::Demander { .. } | ClientMsg::ChargerCle { .. }) => {
                self.message_accueilli(cid, m);
                self.livrer_messages_en_attente(cid);
            }
            _ => self.message_accueilli(cid, m),
        }
    }

    fn message_accueilli(&mut self, cid: ClientId, m: ClientMsg) {
        match m {
            ClientMsg::Hello { .. } => {}
            ClientMsg::Open {
                argv,
                cwd,
                cols,
                rows,
            } => {
                self.ouvrir(
                    cid,
                    &argv.unwrap_or_default(),
                    cwd.as_deref(),
                    cols,
                    rows,
                    Vec::new(),
                );
            }
            ClientMsg::Ouvrir {
                argv,
                cwd,
                cols,
                rows,
                env,
            } => self.ouvrir(cid, &argv, cwd.as_deref(), cols, rows, env),
            ClientMsg::Input { id, data } => match self.sessions.get(&id) {
                // Envoi par un thread dédié : un programme qui ne lit pas sa saisie ne
                // bloque pas le serveur.
                Some(s) => drop(s.saisie.send(data)),
                None => self.inconnue(cid, id),
            },
            ClientMsg::Resize { id, cols, rows } => match self.sessions.get_mut(&id) {
                Some(s) => {
                    let _ = s.session.resize(cols, rows);
                    (s.cols, s.rows) = (cols, rows);
                }
                None => self.inconnue(cid, id),
            },
            ClientMsg::Close { id } => match self.sessions.remove(&id) {
                Some(s) => s.session.kill(), // Exited suivra, diffusé à tous.
                None => self.inconnue(cid, id),
            },
            ClientMsg::List => {
                let mut v: Vec<SessionInfo> = self
                    .sessions
                    .iter()
                    .map(|(&id, s)| SessionInfo {
                        id,
                        cols: s.cols,
                        rows: s.rows,
                        busy: s.occupe,
                    })
                    .collect();
                v.sort_by_key(|s| s.id);
                self.envoyer(cid, ServerMsg::Sessions(v));
                self.envoyer_demandes(cid);
            }
            ClientMsg::Attach { id } => match self.sessions.get(&id) {
                Some(s) => {
                    let (data, marques) = s.relecture.snapshot_avec_marques();
                    // Un client en version 3 reçoit toujours `ReplayMarque` (même sans
                    // marque : un seul chemin à gérer, un seul message quelle que soit la
                    // quantité de marques) ; les autres, un `Replay` sans marque.
                    let v3 = self.clients.get(&cid).is_some_and(|c| c.version >= 3);
                    let m = if v3 {
                        let marques = marques
                            .into_iter()
                            .map(|(d, m)| (u32::try_from(d).unwrap_or(u32::MAX), m))
                            .collect();
                        ServerMsg::ReplayMarque { id, data, marques }
                    } else {
                        ServerMsg::Replay { id, data }
                    };
                    self.envoyer(cid, m);
                }
                None => self.inconnue(cid, id),
            },
            // Le contenu des demandes et des réponses n'est jamais journalisé ni mis dans
            // un message d'erreur.
            ClientMsg::Demander {
                session,
                invite,
                genre,
                trousseau,
            } => self.demander(cid, session, invite, genre, trousseau),
            ClientMsg::Repondre {
                demande,
                reponse,
                memoriser,
            } => self.repondre(cid, demande, reponse, memoriser),
            // Envoyé par `pupitre-askpass` (un demandeur) après avoir mémorisé une phrase.
            ClientMsg::ChargerCle { cle } => {
                if self.clients.get(&cid).is_some_and(|c| c.demandeur) {
                    self.chargement.attente.fusionner(Travail {
                        toutes: false,
                        cles: vec![cle],
                    });
                    self.lancer_chargement();
                }
            }
        }
    }

    /// Prend en compte un nouvel agent (démarrage, agent relancé) puis lance le chargement.
    fn verifier_chargement(&mut self) {
        if self.ssh.agent_change() {
            self.chargement.attente.toutes = true;
        }
        self.lancer_chargement();
    }

    /// Lance, sur un fil à part, le chargement des clés en attente (une seule fois à la fois ;
    /// les demandes qui arrivent entre-temps sont regroupées).
    fn lancer_chargement(&mut self) {
        if self.chargement.en_cours || self.chargement.attente.est_vide() {
            return;
        }
        let (Some(agent), Some(askpass)) = (self.ssh.agent(), self.ssh.askpass()) else {
            self.chargement.attente = Travail::default();
            return;
        };
        let (agent, askpass) = (agent.to_path_buf(), askpass.to_path_buf());
        let travail = std::mem::take(&mut self.chargement.attente);
        let toutes = travail.toutes;
        let coffre = self.coffre.clone();
        let evt_tx = self.evt_tx.clone();
        self.chargement.en_cours = true;
        let lance = thread::Builder::new()
            .name("pupitre-chargement".into())
            .spawn(move || {
                let r = chargement::executer(coffre.as_ref(), &agent, &askpass, &travail);
                let _ = evt_tx.send(Evt::Chargement(r, toutes));
            });
        if lance.is_err() {
            self.chargement.en_cours = false;
        }
    }

    fn fin_chargement(&mut self, rapport: Rapport, toutes: bool) {
        self.chargement.en_cours = false;
        // Le nouvel essai n'est levé que par un chargement de TOUTES les clés réussi : un
        // chargement ponctuel (une phrase qu'on vient de saisir) laisse les autres en plan.
        if rapport.verrouille {
            self.chargement.reessai = true;
        } else if toutes {
            self.chargement.reessai = false;
        }
        if rapport.verrouille {
            // Une seule fois.
            if !self.chargement.verrouille_signale {
                self.chargement.verrouille_signale = true;
                self.notifier(chargement::message_verrouille());
            }
        } else {
            self.chargement.verrouille_signale = false;
        }
        for texte in rapport.messages {
            self.notifier(texte);
        }
        self.lancer_chargement();
    }

    /// Montre `texte` aux interfaces ; sans interface, il est gardé pour la première.
    fn notifier(&mut self, texte: String) {
        let interfaces = self.interfaces();
        if interfaces.is_empty() {
            self.chargement.messages_en_attente.push(texte);
            return;
        }
        for i in interfaces {
            self.envoyer(i, ServerMsg::Message(texte.clone()));
        }
    }

    /// Remet à `cid`, si c'est une interface, les messages gardés.
    fn livrer_messages_en_attente(&mut self, cid: ClientId) {
        if self.chargement.messages_en_attente.is_empty() || !self.interfaces().contains(&cid) {
            return;
        }
        for texte in std::mem::take(&mut self.chargement.messages_en_attente) {
            self.envoyer(cid, ServerMsg::Message(texte));
        }
    }

    /// Interfaces : clients accueillis en version 2 qui ne sont pas demandeurs.
    fn interfaces(&self) -> Vec<ClientId> {
        let mut v: Vec<ClientId> = self
            .clients
            .iter()
            .filter(|(_, c)| c.accueilli && c.version >= 2 && !c.demandeur)
            .map(|(&id, _)| id)
            .collect();
        v.sort_unstable();
        v
    }

    fn demander(
        &mut self,
        cid: ClientId,
        session: Option<SessionId>,
        invite: String,
        genre: pupitre_proto::Genre,
        trousseau: pupitre_proto::EtatTrousseau,
    ) {
        if let Some(c) = self.clients.get_mut(&cid) {
            c.demandeur = true;
        }
        let interfaces = self.interfaces();
        if interfaces.is_empty() {
            // Personne pour répondre : refus tout de suite, rien n'est conservé.
            self.envoyer(
                cid,
                ServerMsg::Reponse {
                    reponse: None,
                    memoriser: false,
                },
            );
            return;
        }
        let demande = self.prochaine_demande;
        self.prochaine_demande += 1;
        let msg = ServerMsg::Demande {
            demande,
            session,
            invite,
            genre,
            trousseau,
        };
        self.demandes.insert(
            demande,
            DemandeOuverte {
                demandeur: cid,
                msg: msg.clone(),
            },
        );
        for i in interfaces {
            self.envoyer(i, msg.clone());
        }
    }

    fn repondre(
        &mut self,
        cid: ClientId,
        demande: DemandeId,
        reponse: Option<Zeroizing<String>>,
        memoriser: bool,
    ) {
        // Seule une interface répond ; une demande inconnue ou déjà réglée est ignorée.
        if !self.interfaces().contains(&cid) {
            return;
        }
        let Some(d) = self.demandes.remove(&demande) else {
            return;
        };
        self.envoyer(d.demandeur, ServerMsg::Reponse { reponse, memoriser });
        for i in self.interfaces() {
            if i != cid {
                self.envoyer(i, ServerMsg::DemandeClose { demande });
            }
        }
    }

    /// Envoie à `cid` les demandes ouvertes, s'il est une interface.
    fn envoyer_demandes(&mut self, cid: ClientId) {
        if !self.interfaces().contains(&cid) {
            return;
        }
        let msgs: Vec<ServerMsg> = self.demandes.values().map(|d| d.msg.clone()).collect();
        for m in msgs {
            self.envoyer(cid, m);
        }
    }

    /// Retire un client ; s'il était demandeur, ses demandes ouvertes sont closes.
    fn retirer(&mut self, cid: ClientId) -> Option<ClientEtat> {
        let c = self.clients.remove(&cid)?;
        let fermees: Vec<DemandeId> = self
            .demandes
            .iter()
            .filter(|(_, d)| d.demandeur == cid)
            .map(|(&id, _)| id)
            .collect();
        for demande in fermees {
            self.demandes.remove(&demande);
            for i in self.interfaces() {
                self.envoyer(i, ServerMsg::DemandeClose { demande });
            }
        }
        Some(c)
    }

    fn ouvrir(
        &mut self,
        cid: ClientId,
        argv: &[String],
        cwd: Option<&Path>,
        cols: u16,
        rows: u16,
        env_client: Vec<(String, String)>,
    ) {
        // Les variables du client d'abord : celles du fond (agent, askpass, session) passent
        // après et l'emportent, pour que l'interface ne puisse pas détourner l'agent isolé.
        let mut env: Vec<(String, String)> = env_client
            .into_iter()
            .filter(|(k, v)| {
                !k.is_empty()
                    && !k.contains(['=', '\0'])
                    && !v.contains('\0')
                    && !variable_protegee(k)
            })
            .collect();
        env.extend(self.ssh.variables(self.prochain_id));
        if self.chargement.reessai {
            self.chargement.attente.toutes = true;
        }
        self.verifier_chargement();
        let (session, evenements) = match Session::open(argv, cwd, cols, rows, &env) {
            Ok(r) => r,
            Err(e) => {
                return self.envoyer(
                    cid,
                    ServerMsg::OpenFailed {
                        error: e.to_string(),
                    },
                )
            }
        };
        let id = self.prochain_id;
        self.prochain_id += 1;
        let session = Arc::new(session);
        let (saisie_tx, saisie_rx) = mpsc::channel::<Vec<u8>>();
        let s = session.clone();
        let saisie = thread::Builder::new()
            .name("pupitre-saisie".into())
            .spawn(move || {
                for d in saisie_rx {
                    // Échec = session terminée ; Exited arrivera par le relais.
                    let _ = s.write_input(&d);
                }
            });
        let evt_tx = self.evt_tx.clone();
        let relais = thread::Builder::new()
            .name("pupitre-relais".into())
            .spawn(move || {
                for e in evenements {
                    if evt_tx.send(Evt::Session(id, e)).is_err() {
                        return;
                    }
                }
            });
        if let Err(e) = saisie.and(relais) {
            session.kill();
            return self.envoyer(
                cid,
                ServerMsg::OpenFailed {
                    error: ta(
                        "serveur-ressources",
                        &[("erreur", ArgI18n::from(e.to_string()))],
                    ),
                },
            );
        }
        self.sessions.insert(
            id,
            SessionEtat {
                session,
                saisie: saisie_tx,
                relecture: ReplayBuffer::new(REPLAY_CAPACITY),
                marques: LecteurMarques::new(),
                limite_marques: LimiteMarques::new(),
                cols,
                rows,
                occupe: false,
            },
        );
        self.envoyer(cid, ServerMsg::Opened { id });
        if let Some(texte) = self.ssh.message_a_envoyer() {
            self.envoyer(cid, ServerMsg::Message(texte));
        }
    }

    fn controler_occupation(&mut self) {
        let mut changements = Vec::new();
        for (&id, s) in &mut self.sessions {
            let occupe = s.session.is_busy();
            if occupe != s.occupe {
                s.occupe = occupe;
                changements.push(ServerMsg::Busy { id, busy: occupe });
            }
        }
        for m in changements {
            self.diffuser(&m);
        }
    }

    fn inconnue(&mut self, cid: ClientId, id: SessionId) {
        self.envoyer(
            cid,
            ServerMsg::Error(ta(
                "serveur-session-inconnue",
                &[("id", ArgI18n::from(i64::try_from(id).unwrap_or(0)))],
            )),
        );
    }

    /// Met un message dans la file d'un client ; un client qui ne suit plus est coupé.
    /// Un message d'une version plus récente que celle du client ne lui est jamais envoyé
    /// (voir [`adapter_pour`]).
    fn envoyer(&mut self, cid: ClientId, m: ServerMsg) {
        let Some(c) = self.clients.get(&cid) else {
            return;
        };
        let Some(m) = adapter_pour(m, c.version) else {
            return;
        };
        match c.file.try_send(m) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => self.couper(cid),
            Err(TrySendError::Disconnected(_)) => {
                self.retirer(cid);
            }
        }
    }

    /// Envoie à tous les clients accueillis.
    fn diffuser(&mut self, m: &ServerMsg) {
        let destinataires: Vec<ClientId> = self
            .clients
            .iter()
            .filter(|(_, c)| c.accueilli)
            .map(|(&id, _)| id)
            .collect();
        for cid in destinataires {
            self.envoyer(cid, m.clone());
        }
    }

    /// Retire un client après lui avoir laissé recevoir ce qui est déjà dans sa file.
    fn congedier(&mut self, cid: ClientId) {
        self.retirer(cid);
    }

    /// Retire un client immédiatement (il ne lit plus) : la coupure débloque son écriture.
    fn couper(&mut self, cid: ClientId) {
        if let Some(c) = self.retirer(cid) {
            let _ = c.flux.shutdown(Shutdown::Both);
        }
    }
}

/// Met en message les octets en attente (s'il y en a), avant un événement du flux.
fn envoyer_octets(messages: &mut Vec<ServerMsg>, id: SessionId, octets: &mut Vec<u8>) {
    if !octets.is_empty() {
        messages.push(ServerMsg::Output {
            id,
            data: std::mem::take(octets),
        });
    }
}

/// Message tel qu'un client en `version` peut le recevoir, ou `None` s'il ne doit rien
/// recevoir : un message de la version 2 n'est jamais envoyé à un client en version 1, ni
/// une marque de commande (version 3) à un client en version 1 ou 2, ni une copie ou une
/// notification de programme (version 4) à un client en version 1 à 3 ; l'état « trousseau
/// lent » (version 3) devient « absent » pour un client en version 2.
pub fn adapter_pour(m: ServerMsg, version: u32) -> Option<ServerMsg> {
    // Un rejeu avec marques n'arrive jamais à un client en version 1 ou 2 : il devient un
    // `Replay` ordinaire (les octets sont les mêmes).
    let m = match m {
        ServerMsg::ReplayMarque { id, data, .. } if version < 3 => ServerMsg::Replay { id, data },
        m => m,
    };
    if (m.est_v2() && version < 2) || (m.est_v3() && version < 3) || (m.est_v4() && version < 4) {
        return None;
    }
    match m {
        ServerMsg::Demande {
            demande,
            session,
            invite,
            genre,
            trousseau: pupitre_proto::EtatTrousseau::Lent,
        } if version < 3 => Some(ServerMsg::Demande {
            demande,
            session,
            invite,
            genre,
            trousseau: pupitre_proto::EtatTrousseau::Absent,
        }),
        m => Some(m),
    }
}

impl Drop for Etat {
    fn drop(&mut self) {
        for s in self.sessions.values() {
            s.session.kill();
        }
    }
}
