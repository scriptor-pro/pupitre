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

use crate::relecture::{ReplayBuffer, REPLAY_CAPACITY};
use crate::session::{Session, SessionEvent};
use crate::ssh::{EntreesSsh, EtatSsh};
use nix::errno::Errno;
use nix::fcntl::{Flock, FlockArg};
use pupitre_proto::{
    read_frame, write_frame, ClientMsg, ServerMsg, SessionId, SessionInfo, VERSION, VERSION_MIN,
};
use std::collections::HashMap;
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
        io::Error::new(io::ErrorKind::InvalidInput, "chemin de socket sans dossier")
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
            format!("{} : {raison}", dossier.display()),
        ))
    };
    if !m.is_dir() {
        return refus("ce n'est pas un dossier");
    }
    if m.uid() != nix::unistd::getuid().as_raw() {
        return refus("appartient à un autre utilisateur");
    }
    if m.mode() & 0o077 != 0 {
        return refus("droits trop larges (0700 attendu)");
    }
    Ok(())
}

/// Fait tourner le serveur sur `socket` jusqu'à ce qu'il n'y ait plus ni client ni session
/// pendant `idle_exit`. Si un autre processus de fond tient déjà ce socket, retourne
/// aussitôt `Ok(())`. Un socket abandonné (processus de fond disparu) est remplacé.
pub fn run(socket: &Path, idle_exit: Duration) -> io::Result<()> {
    run_avec(socket, idle_exit, EntreesSsh::depuis_env())
}

/// Comme [`run`], avec les entrées de la recherche d'agent SSH fournies (tests).
pub fn run_avec(socket: &Path, idle_exit: Duration, entrees: EntreesSsh) -> io::Result<()> {
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
        evt_tx,
        ssh: EtatSsh::avec_entrees(socket, entrees),
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
        return Err(io::Error::other("serveur en cours d'arrêt"));
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
}

struct SessionEtat {
    session: Arc<Session>,
    saisie: Sender<Vec<u8>>,
    relecture: ReplayBuffer,
    cols: u16,
    rows: u16,
    occupe: bool,
}

struct Etat {
    clients: HashMap<ClientId, ClientEtat>,
    sessions: HashMap<SessionId, SessionEtat>,
    prochain_id: SessionId,
    evt_tx: SyncSender<Evt>,
    ssh: EtatSsh,
}

impl Etat {
    fn boucle(&mut self, evt_rx: &Receiver<Evt>, idle_exit: Duration) {
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
            if self.clients.is_empty() && self.sessions.is_empty() {
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
                    },
                );
            }
            Evt::Depart(id) => {
                self.clients.remove(&id);
            }
            Evt::Message(id, m) => self.message(id, m),
            Evt::Session(sid, SessionEvent::Output(data)) => {
                // Sortie d'une session déjà fermée : ignorée.
                if let Some(s) = self.sessions.get_mut(&sid) {
                    s.relecture.push(&data);
                    self.diffuser(&ServerMsg::Output { id: sid, data });
                }
            }
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
                } else {
                    self.envoyer(
                        cid,
                        ServerMsg::Error(format!(
                            "version du protocole incompatible : interface {version}, processus de fond {VERSION}"
                        )),
                    );
                    self.congedier(cid);
                }
            }
            _ if !accueilli => {
                self.envoyer(
                    cid,
                    ServerMsg::Error("protocole : Hello attendu en premier".into()),
                );
                self.congedier(cid);
            }
            ClientMsg::Hello { .. } => {
                self.envoyer(cid, ServerMsg::Error("protocole : Hello déjà reçu".into()))
            }
            ClientMsg::Open {
                argv,
                cwd,
                cols,
                rows,
            } => {
                self.ouvrir(cid, &argv.unwrap_or_default(), cwd.as_deref(), cols, rows);
            }
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
            }
            ClientMsg::Attach { id } => match self.sessions.get(&id) {
                Some(s) => {
                    let data = s.relecture.snapshot();
                    self.envoyer(cid, ServerMsg::Replay { id, data });
                }
                None => self.inconnue(cid, id),
            },
            // Traité à la Task 6/7. Le contenu (invite, réponse) n'est jamais journalisé.
            ClientMsg::Demander { .. }
            | ClientMsg::Repondre { .. }
            | ClientMsg::ChargerCle { .. } => {}
        }
    }

    fn ouvrir(&mut self, cid: ClientId, argv: &[String], cwd: Option<&Path>, cols: u16, rows: u16) {
        let env = self.ssh.variables(self.prochain_id);
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
                    error: format!("ressources insuffisantes : {e}"),
                },
            );
        }
        self.sessions.insert(
            id,
            SessionEtat {
                session,
                saisie: saisie_tx,
                relecture: ReplayBuffer::new(REPLAY_CAPACITY),
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
        self.envoyer(cid, ServerMsg::Error(format!("session {id} inconnue")));
    }

    /// Met un message dans la file d'un client ; un client qui ne suit plus est coupé.
    /// Un message de la version 2 n'est jamais envoyé à un client en version 1.
    fn envoyer(&mut self, cid: ClientId, m: ServerMsg) {
        let Some(c) = self.clients.get(&cid) else {
            return;
        };
        if m.est_v2() && c.version < 2 {
            return;
        }
        match c.file.try_send(m) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => self.couper(cid),
            Err(TrySendError::Disconnected(_)) => {
                self.clients.remove(&cid);
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
        self.clients.remove(&cid);
    }

    /// Retire un client immédiatement (il ne lit plus) : la coupure débloque son écriture.
    fn couper(&mut self, cid: ClientId) {
        if let Some(c) = self.clients.remove(&cid) {
            let _ = c.flux.shutdown(Shutdown::Both);
        }
    }
}

impl Drop for Etat {
    fn drop(&mut self) {
        for s in self.sessions.values() {
            s.session.kill();
        }
    }
}
