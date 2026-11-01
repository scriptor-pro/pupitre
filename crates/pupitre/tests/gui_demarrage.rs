// SPDX-License-Identifier: GPL-3.0-or-later
//! Tests graphiques du démarrage de l'interface face à un processus de fond qui a déjà
//! quelque chose à dire : une demande SSH ouverte (envoyée au `Hello` puis au `List`), ou
//! un message gardé pour la première interface. Lancés par `scripts/test-x11.sh` et
//! `scripts/test-wayland.sh` ; ignorés sinon.
//!
//! Le fond tourne dans ce processus (fil à part), avec un coffre en mémoire : le trousseau
//! n'est jamais approché. Chaque test a sa propre application GTK et son propre fond.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod commun;

use commun::*;
use gtk::glib;
use gtk::prelude::*;
use pupitre::fenetre::Pupitre;
use pupitre::polices::{dossier_polices, enregistrer_polices_embarquees};
use pupitre_config::Config;
use pupitre_fond::chargement::message_verrouille;
use pupitre_fond::serveur::{run_avec_coffre, CoffreCommun};
use pupitre_fond::ssh::EntreesSsh;
use pupitre_proto::{read_frame, write_frame, ClientMsg, EtatTrousseau, Genre, ServerMsg};
use pupitre_ssh::trousseau::{Coffre, CoffreMemoire, ErreurTrousseau};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

const TITRE: &str = "Phrase de passe SSH";

fn main() {
    if !actives() {
        return;
    }
    bus_prive();
    compiler();
    let _perso = isoler_dossiers();
    assert!(enregistrer_polices_embarquees(&dossier_polices().unwrap()));
    glib::set_prgname(Some("pupitre-test-demarrage"));
    gtk::init().unwrap();

    let mut ex = Executeur::new();
    ex.lancer(
        "demande_recue_deux_fois_une_seule_fenetre",
        demande_deux_fois,
    );
    ex.lancer(
        "message_garde_montre_dans_la_premiere_fenetre",
        message_garde,
    );
    ex.lancer(
        "information_retiree_a_la_perte_du_fond",
        information_retiree,
    );
    ex.conclure();
}

/// Processus de fond lancé dans ce processus ; attendu (et son agent tué) à la fin.
struct Fond {
    dossier: tempfile::TempDir,
    socket: PathBuf,
    fil: Option<JoinHandle<()>>,
}

impl Fond {
    fn demarrer(askpass: Option<PathBuf>, programme_agent: &str, coffre: CoffreCommun) -> Fond {
        let dossier = tempfile::tempdir().unwrap();
        let socket = dossier.path().join("pupitre/fond.sock");
        let tmp = dossier.path().join("tmp");
        std::fs::create_dir_all(&tmp).unwrap();
        let entrees = EntreesSsh {
            env_sock: None,
            runtime: Some(dossier.path().join("runtime")),
            tmp,
            askpass,
            programme_agent: programme_agent.into(),
        };
        let s = socket.clone();
        let fil = thread::spawn(move || {
            let _ = run_avec_coffre(&s, Duration::from_secs(1), entrees, coffre);
        });
        let limite = Instant::now() + Duration::from_secs(10);
        while !socket.exists() {
            assert!(Instant::now() < limite, "fond jamais prêt");
            thread::sleep(Duration::from_millis(20));
        }
        Fond {
            dossier,
            socket,
            fil: Some(fil),
        }
    }

    /// Ferme les sessions puis attend l'arrêt du fond (par inactivité).
    fn arreter(mut self) {
        fermer_sessions(&self.socket);
        if let Some(f) = self.fil.take() {
            let limite = Instant::now() + Duration::from_secs(20);
            while !f.is_finished() && Instant::now() < limite {
                patienter(Duration::from_millis(50));
            }
            if f.is_finished() {
                let _ = f.join();
            }
        }
        let _ = &self.dossier;
    }
}

/// Client brut du protocole (interface ou demandeur).
struct Brut(UnixStream);

impl Brut {
    fn accueilli(socket: &Path) -> Brut {
        let mut s = UnixStream::connect(socket).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        write_frame(&mut s, &ClientMsg::Hello { version: 2 }).unwrap();
        assert!(matches!(
            read_frame::<_, ServerMsg>(&mut s).unwrap(),
            ServerMsg::Welcome { .. }
        ));
        Brut(s)
    }

    fn envoyer(&mut self, m: ClientMsg) {
        write_frame(&mut self.0, &m).unwrap();
    }

    fn attendre(&mut self, f: impl Fn(&ServerMsg) -> bool) {
        loop {
            let m = read_frame::<_, ServerMsg>(&mut self.0).unwrap();
            if f(&m) {
                return;
            }
        }
    }
}

fn nouvelle_appli(id: &str) -> gtk::Application {
    let app = gtk::Application::builder()
        .application_id(id)
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gtk::gio::Cancellable::NONE).unwrap();
    app
}

fn fenetres_de_demande() -> Vec<gtk::Window> {
    gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Window>().ok())
        .filter(|w| w.is_visible() && w.title().as_deref() == Some(TITRE))
        .collect()
}

fn fermer_tout(p: &Rc<Pupitre>) {
    for f in p.fenetres() {
        f.fenetre().destroy();
    }
    patienter(Duration::from_millis(200));
}

/// Une demande est ouverte quand l'interface arrive : le fond la lui envoie au `Hello`
/// puis au `List`. Une seule fenêtre de Pupitre, une seule fenêtre de demande, aucun
/// plantage ; la réponse arrive bien au demandeur.
fn demande_deux_fois() -> Resultat {
    let fond = Fond::demarrer(
        None,
        "/nonexistent/ssh-agent",
        Arc::new(CoffreMemoire::default()),
    );
    // Une interface brute, le temps que la demande soit acceptée, puis elle part (comme
    // une interface qui plante pendant un `git pull`).
    let mut ui = Brut::accueilli(&fond.socket);
    ui.envoyer(ClientMsg::List);
    ui.attendre(|m| matches!(m, ServerMsg::Sessions(_)));
    let mut ask = Brut::accueilli(&fond.socket);
    ask.envoyer(ClientMsg::Demander {
        session: None,
        invite: "Enter passphrase for /tmp/x: ".into(),
        genre: Genre::PhraseCle {
            cle: "/tmp/x".into(),
            nouvel_essai: false,
        },
        trousseau: EtatTrousseau::Absent,
    });
    ui.attendre(|m| matches!(m, ServerMsg::Demande { .. }));
    drop(ui);
    // Le demandeur attend sa réponse sur un fil à part (la boucle GTK doit tourner).
    let (tx, reponse) = mpsc::channel();
    thread::spawn(move || loop {
        match read_frame::<_, ServerMsg>(&mut ask.0) {
            Ok(ServerMsg::Reponse { reponse, .. }) => {
                let _ = tx.send(reponse.is_none());
                return;
            }
            Ok(_) => {}
            Err(_) => return,
        }
    });

    let app = nouvelle_appli("fr.pupitre.TestDemarrageDemande");
    let p = Pupitre::demarrer(&app, fond.socket.clone(), Config::default());
    let vue = attendre(Duration::from_secs(10), || {
        !p.fenetres().is_empty() && !fenetres_de_demande().is_empty()
    });
    // Le temps de recevoir le second envoi de la demande.
    patienter(Duration::from_secs(1));
    let resultat = (|| {
        verifier(vue, || "fenêtre de demande jamais ouverte".into())?;
        let n = p.fenetres().len();
        verifier(n == 1, || format!("{n} fenêtres de Pupitre (une attendue)"))?;
        let demandes = fenetres_de_demande();
        verifier(demandes.len() == 1, || {
            format!("{} fenêtres de demande (une attendue)", demandes.len())
        })?;
        let parent = demandes[0].transient_for();
        verifier(
            parent.as_ref() == Some(p.fenetres()[0].fenetre().upcast_ref()),
            || "la demande n'est pas portée par la fenêtre de Pupitre".into(),
        )?;
        // Fermer la fenêtre répond « annulé » au demandeur.
        demandes[0].close();
        let mut annule = None;
        attendre(Duration::from_secs(10), || {
            annule = reponse.try_recv().ok();
            annule.is_some()
        });
        verifier(annule == Some(true), || {
            format!("réponse du demandeur : {annule:?}")
        })
    })();
    fermer_tout(&p);
    drop(p);
    fond.arreter();
    resultat
}

/// Coffre verrouillé qui signale le premier `etat()` (début du chargement).
struct CoffreVerrouille {
    interroge: Arc<AtomicBool>,
}

impl Coffre for CoffreVerrouille {
    fn etat(&self) -> EtatTrousseau {
        self.interroge.store(true, Ordering::SeqCst);
        EtatTrousseau::Verrouille
    }
    fn lire(&self, _: &Path) -> Result<Option<Zeroizing<String>>, ErreurTrousseau> {
        Err(ErreurTrousseau::Verrouille)
    }
    fn ecrire(&self, _: &Path, _: &str) -> Result<(), ErreurTrousseau> {
        Err(ErreurTrousseau::Verrouille)
    }
    fn oublier(&self, _: &Path) -> Result<(), ErreurTrousseau> {
        Err(ErreurTrousseau::Verrouille)
    }
    fn cles(&self) -> Vec<PathBuf> {
        Vec::new()
    }
}

/// Le chargement a dit « trousseau verrouillé » avant toute interface : le message est
/// gardé par le fond, puis montré dans la première fenêtre (qui n'existe pas encore quand
/// il arrive).
fn message_garde() -> Resultat {
    let interroge = Arc::new(AtomicBool::new(false));
    let fond = Fond::demarrer(
        Some("/bin/true".into()),
        "ssh-agent",
        Arc::new(CoffreVerrouille {
            interroge: interroge.clone(),
        }),
    );
    let limite = Instant::now() + Duration::from_secs(10);
    while !interroge.load(Ordering::SeqCst) && Instant::now() < limite {
        thread::sleep(Duration::from_millis(20));
    }
    // Le rapport du chargement atteint le fil central juste après.
    thread::sleep(Duration::from_millis(500));

    let app = nouvelle_appli("fr.pupitre.TestDemarrageMessage");
    let p = Pupitre::demarrer(&app, fond.socket.clone(), Config::default());
    let mut messages = Vec::new();
    let vu = attendre(Duration::from_secs(10), || {
        messages = p
            .fenetres()
            .first()
            .map(|f| f.messages())
            .unwrap_or_default();
        messages.iter().any(|m| *m == message_verrouille())
    });
    let resultat = verifier(vu, || {
        format!(
            "message absent de la première fenêtre ({} fenêtre(s)) : {messages:?}",
            p.fenetres().len()
        )
    });
    fermer_tout(&p);
    drop(p);
    fond.arreter();
    resultat
}

/// Une demande « Information » (bandeau) disparaît quand le fond est perdu : plus
/// personne ne la retirera.
fn information_retiree() -> Resultat {
    const INFO: &str = "Confirm user presence for key ED25519 SHA256:test";
    let dossier = tempfile::tempdir().unwrap();
    let socket = dossier.path().join("pupitre/fond.sock");
    // Fond lancé par l'interface (processus à part, qu'on peut tuer).
    let app = nouvelle_appli("fr.pupitre.TestDemarrageInformation");
    let p = Pupitre::demarrer(&app, socket.clone(), Config::default());
    let resultat = (|| {
        let ok = attendre(Duration::from_secs(10), || {
            p.fenetres()
                .first()
                .is_some_and(|f| f.onglet_actif().is_some())
        });
        verifier(ok, || "pas de fenêtre".into())?;
        let mut ask = Brut::accueilli(&socket);
        ask.envoyer(ClientMsg::Demander {
            session: None,
            invite: INFO.into(),
            genre: Genre::Information,
            trousseau: EtatTrousseau::Absent,
        });
        let f = p.fenetres()[0].clone();
        let ok = attendre(Duration::from_secs(10), || {
            f.messages().iter().any(|m| m == INFO)
        });
        verifier(ok, || format!("information absente : {:?}", f.messages()))?;
        let tue = std::process::Command::new("pkill")
            .arg("-KILL")
            .arg("-f")
            .arg(socket.to_string_lossy().as_ref())
            .status()
            .map_err(|e| e.to_string())?;
        verifier(tue.success(), || "processus de fond introuvable".into())?;
        let ok = attendre(Duration::from_secs(10), || {
            let m = f.messages();
            m.iter()
                .any(|m| m.contains("processus de fond s'est arrêté"))
                && !m.iter().any(|m| m == INFO)
        });
        drop(ask);
        verifier(ok, || {
            format!("après la perte du fond : {:?}", f.messages())
        })
    })();
    fermer_tout(&p);
    drop(p);
    resultat
}
