// SPDX-License-Identifier: GPL-3.0-or-later
//! Tests graphiques des notifications (étape 4, Task 2) : ✓/✗/● sur les onglets, bulles du
//! bureau (comptées par une fausse fonction d'envoi), copie d'un programme signalée.
//! Lancés par `scripts/test-x11.sh` et `scripts/test-wayland.sh` ; ignorés sinon.
//!
//! Le fond est un faux processus de fond dans ce processus (comme `gui_commandes`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod commun;

use commun::*;
use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use pupitre::fenetre::{FenetrePupitre, Onglet, Pupitre};
use pupitre::notifications::{Bulle, Indicateur};
use pupitre::polices::{dossier_polices, enregistrer_polices_embarquees};
use pupitre_config::{Config, ReglagesNotifications};
use pupitre_proto::{read_frame, write_frame, ClientMsg, MarqueCommande, ServerMsg, SessionInfo};
use std::cell::RefCell;
use std::os::unix::net::{UnixListener, UnixStream};
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use vte4::prelude::*;

const NOM_APPLI: &str = "pupitre-test-notifications";
const DELAI: Duration = Duration::from_secs(10);

use MarqueCommande::{Execution, Fin, Invite, Saisie};

fn main() {
    if !actives() {
        return;
    }
    bus_prive();
    compiler();
    let _perso = isoler_dossiers();
    glib::set_prgname(Some(NOM_APPLI));
    glib::set_application_name(NOM_APPLI);
    assert!(enregistrer_polices_embarquees(&dossier_polices().unwrap()));
    gtk::init().unwrap();

    let mut ex = Executeur::new();
    ex.lancer("fin_commande_onglet_cache_coche", fin_commande_coche);
    ex.lancer("echec_onglet_cache_croix", echec_croix);
    ex.lancer("indicateur_efface_a_l_activation", efface_a_l_activation);
    ex.lancer("nom_accessible_avec_indicateur", nom_accessible);
    ex.lancer("notification_programme_point", notification_point);
    ex.lancer("notification_sans_titre_repli", notification_sans_titre);
    ex.lancer(
        "bulle_programme_force_indicateur",
        bulle_force_indicateur_programme,
    );
    ex.lancer(
        "bulle_commande_force_indicateur",
        bulle_force_indicateur_commande,
    );
    ex.lancer("copie_programme_message", copie_message);
    ex.lancer("copie_desactivee", copie_desactivee);
    ex.lancer("notifications_en_rafale_bornees", rafale);
    ex.lancer("bulle_seulement_hors_premier_plan", bulle_hors_premier_plan);
    ex.lancer("bulle_desactivee_et_onglet_desactive", reglages_coupes);
    ex.lancer("montrer_onglet_action", montrer_onglet);
    ex.conclure();
}

/// Faux processus de fond : une session (numéro 1) qui ne fait que ce qu'on lui envoie.
struct FauxFond {
    dossier: tempfile::TempDir,
    flux: Arc<Mutex<Option<UnixStream>>>,
}

impl FauxFond {
    fn demarrer() -> FauxFond {
        use std::os::unix::fs::DirBuilderExt;
        let dossier = tempfile::tempdir().unwrap();
        let sous = dossier.path().join("pupitre");
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&sous)
            .unwrap();
        let ecoute = UnixListener::bind(sous.join("fond.sock")).unwrap();
        let flux = Arc::new(Mutex::new(None));
        let f2 = flux.clone();
        thread::spawn(move || {
            let Ok((mut lecture, _)) = ecoute.accept() else {
                return;
            };
            *f2.lock().unwrap() = lecture.try_clone().ok();
            let envoyer = |m: ServerMsg| {
                let mut g = f2.lock().unwrap();
                if let Some(f) = g.as_mut() {
                    let _ = write_frame(f, &m);
                }
            };
            let mut prochain = 2;
            while let Ok(m) = read_frame::<_, ClientMsg>(&mut lecture) {
                match m {
                    ClientMsg::Hello { version } => envoyer(ServerMsg::Welcome { version }),
                    ClientMsg::List => envoyer(ServerMsg::Sessions(vec![SessionInfo {
                        id: 1,
                        cols: 80,
                        rows: 24,
                        busy: false,
                    }])),
                    ClientMsg::Attach { id } => envoyer(ServerMsg::ReplayMarque {
                        id,
                        data: Vec::new(),
                        marques: Vec::new(),
                    }),
                    ClientMsg::Open { .. } | ClientMsg::Ouvrir { .. } => {
                        envoyer(ServerMsg::Opened { id: prochain });
                        prochain += 1;
                    }
                    _ => {}
                }
            }
        });
        FauxFond { dossier, flux }
    }

    fn socket(&self) -> std::path::PathBuf {
        self.dossier.path().join("pupitre/fond.sock")
    }

    fn envoyer(&self, m: ServerMsg) {
        let mut g = self.flux.lock().unwrap();
        write_frame(g.as_mut().expect("interface non connectée"), &m).unwrap();
    }

    fn octets(&self, id: u64, t: &str) {
        self.envoyer(ServerMsg::Output {
            id,
            data: t.as_bytes().to_vec(),
        });
    }

    fn marque(&self, id: u64, marque: MarqueCommande) {
        self.envoyer(ServerMsg::Marque {
            id,
            marque,
            rejouee: false,
        });
    }

    /// Une commande complète (invite, saisie, exécution, fin).
    fn commande(&self, id: u64, code: i32, duree_ms: Option<u64>) {
        self.marque(id, Invite);
        self.octets(id, "$ ");
        self.marque(id, Saisie);
        self.octets(id, "make\r\n");
        self.marque(id, Execution);
        self.marque(
            id,
            Fin {
                code: Some(code),
                duree_ms,
            },
        );
    }

    fn notification(&self, id: u64, titre: Option<&str>, corps: &str) {
        self.envoyer(ServerMsg::Notification {
            id,
            titre: titre.map(String::from),
            corps: corps.into(),
        });
    }
}

static NUMERO: AtomicU32 = AtomicU32::new(0);

struct Banc {
    fond: FauxFond,
    app: gtk::Application,
    p: Rc<Pupitre>,
    f: FenetrePupitre,
    o: Rc<Onglet>,
    /// Bulles « envoyées au bureau » (la fausse fonction d'envoi les range ici).
    bulles: Rc<RefCell<Vec<Bulle>>>,
}

impl Banc {
    fn nouveau(config: Config) -> Banc {
        pupitre::commandes::forcer_fenetres_inactives(false);
        let fond = FauxFond::demarrer();
        let app = gtk::Application::builder()
            .application_id(format!(
                "fr.pupitre.TestNotifications{}",
                NUMERO.fetch_add(1, Ordering::SeqCst)
            ))
            .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(gtk::gio::Cancellable::NONE).unwrap();
        let p = Pupitre::demarrer(&app, fond.socket(), config);
        let bulles = Rc::new(RefCell::new(Vec::new()));
        let b2 = bulles.clone();
        p.remplacer_envoi_bulle(Rc::new(move |b: &Bulle| b2.borrow_mut().push(b.clone())));
        let pret = attendre(DELAI, || {
            p.fenetres().first().is_some_and(|f| {
                f.onglets()
                    .first()
                    .is_some_and(|o| o.terminal().terminal().is_mapped())
            })
        });
        assert!(pret, "pas de fenêtre au démarrage");
        let f = p.fenetres()[0].clone();
        f.fenetre().present();
        if !attendre(Duration::from_secs(3), || f.fenetre().is_active()) {
            assert!(
                std::env::var_os("WAYLAND_DISPLAY").is_some(),
                "la fenêtre n'est jamais devenue active"
            );
            pupitre::commandes::forcer_fenetre_active(true);
        }
        let o = f.onglets()[0].clone();
        o.terminal().terminal().grab_focus();
        Banc {
            fond,
            app,
            p,
            f,
            o,
            bulles,
        }
    }

    /// Ouvre un deuxième onglet : le premier devient caché.
    fn cacher_premier(&self) -> Resultat {
        self.f.ouvrir_session();
        verifier(attendre(DELAI, || self.f.onglets().len() == 2), || {
            "deuxième onglet jamais ouvert".into()
        })?;
        patienter(Duration::from_millis(200));
        Ok(())
    }

    fn fermer(self) {
        pupitre::commandes::forcer_fenetres_inactives(false);
        for f in self.p.fenetres() {
            f.fenetre().destroy();
        }
        patienter(Duration::from_millis(200));
    }

    fn attendre_indicateur(&self, o: &Rc<Onglet>, voulu: Indicateur) -> Resultat {
        verifier(attendre(DELAI, || o.indicateur() == voulu), || {
            format!("indicateur {:?} au lieu de {voulu:?}", o.indicateur())
        })
    }

    /// Attend que tout ce qui précède dans le flux du fond soit traité.
    fn synchroniser(&self) -> Resultat {
        self.fond.octets(1, "\x1b[2J\x1b[Hsynchro\r\n");
        verifier(
            attendre(DELAI, || {
                self.o
                    .terminal()
                    .terminal()
                    .text_format(vte4::Format::Text)
                    .is_some_and(|t| t.contains("synchro"))
            }),
            || "flux du fond non traité".into(),
        )
    }
}

fn lire_presse_papiers() -> Option<String> {
    let cb = gdk::Display::default()?.clipboard();
    let resultat: Rc<RefCell<Option<Option<String>>>> = Rc::new(RefCell::new(None));
    let r2 = resultat.clone();
    glib::MainContext::default().spawn_local(async move {
        let t = cb
            .read_text_future()
            .await
            .ok()
            .flatten()
            .map(|g| g.to_string());
        *r2.borrow_mut() = Some(t);
    });
    attendre(DELAI, || resultat.borrow().is_some());
    let valeur = resultat.borrow().clone();
    valeur.flatten()
}

fn fin_commande_coche() -> Resultat {
    let b = Banc::nouveau(Config::default());
    b.cacher_premier()?;
    b.fond.commande(1, 0, Some(1000));
    b.attendre_indicateur(&b.o, Indicateur::Reussi)?;
    let t = b.o.titre_affiche();
    let bulles = b.bulles.borrow().len();
    b.fermer();
    verifier(t.starts_with("✓ "), || format!("étiquette « {t} »"))?;
    verifier(bulles == 0, || format!("{bulles} bulle(s) pour 1 s"))
}

fn echec_croix() -> Resultat {
    let b = Banc::nouveau(Config::default());
    b.cacher_premier()?;
    b.fond.commande(1, 2, Some(1000));
    b.attendre_indicateur(&b.o, Indicateur::Echec)?;
    let t = b.o.titre_affiche();
    b.fermer();
    verifier(t.starts_with("✗ "), || format!("étiquette « {t} »"))
}

fn efface_a_l_activation() -> Resultat {
    let b = Banc::nouveau(Config::default());
    b.cacher_premier()?;
    b.fond.commande(1, 0, Some(1000));
    b.attendre_indicateur(&b.o, Indicateur::Reussi)?;
    // Alt+1 : l'onglet 1 redevient actif.
    b.f.traiter_touche(gdk::Key::_1, 10, gdk::ModifierType::ALT_MASK);
    let r = b.attendre_indicateur(&b.o, Indicateur::Aucun);
    let t = b.o.titre_affiche();
    b.fermer();
    r?;
    verifier(!t.starts_with("✓"), || format!("étiquette « {t} »"))
}

fn nom_accessible() -> Resultat {
    let b = Banc::nouveau(Config::default());
    b.cacher_premier()?;
    let titre = b.o.titre();
    verifier(b.o.nom_accessible() == titre, || {
        format!("sans indicateur : « {} »", b.o.nom_accessible())
    })?;
    b.fond.commande(1, 0, Some(1000));
    b.attendre_indicateur(&b.o, Indicateur::Reussi)?;
    let reussi = b.o.nom_accessible();
    b.fond.commande(1, 1, Some(1000));
    b.attendre_indicateur(&b.o, Indicateur::Echec)?;
    let echec = b.o.nom_accessible();
    b.fond.notification(1, None, "fini");
    b.attendre_indicateur(&b.o, Indicateur::Signale)?;
    let notif = b.o.nom_accessible();
    b.o.terminal().on_output(b"\x07");
    patienter(Duration::from_millis(100));
    let cloche = b.o.nom_accessible();
    b.fermer();
    verifier(reussi == format!("{titre}, commande réussie"), || {
        format!("« {reussi} »")
    })?;
    verifier(echec == format!("{titre}, commande en échec"), || {
        format!("« {echec} »")
    })?;
    verifier(notif == format!("{titre}, notification"), || {
        format!("« {notif} »")
    })?;
    verifier(cloche == format!("{titre}, cloche"), || {
        format!("« {cloche} »")
    })
}

fn notification_point() -> Resultat {
    let b = Banc::nouveau(Config::default());
    b.cacher_premier()?;
    b.fond.notification(1, Some("Compilation"), "terminée");
    b.attendre_indicateur(&b.o, Indicateur::Signale)?;
    let ok = attendre(DELAI, || !b.bulles.borrow().is_empty());
    patienter(Duration::from_millis(300));
    let t = b.o.titre_affiche();
    let signale = b.o.signale();
    let bulles = b.bulles.borrow().clone();
    b.fermer();
    verifier(t.starts_with("● "), || format!("étiquette « {t} »"))?;
    verifier(signale, || "signale() faux".into())?;
    verifier(ok && bulles.len() == 1, || format!("bulles : {bulles:?}"))?;
    let u = &bulles[0];
    verifier(
        u.titre == "Compilation" && u.corps == "terminée" && u.session == 1,
        || format!("bulle : {u:?}"),
    )?;
    verifier(u.id == "onglet-1", || format!("identifiant : {}", u.id))
}

fn notification_sans_titre() -> Resultat {
    let b = Banc::nouveau(Config::default());
    b.cacher_premier()?;
    b.o.renommer(Some("Serveur"));
    b.fond.notification(1, None, "prêt");
    let ok = attendre(DELAI, || !b.bulles.borrow().is_empty());
    patienter(Duration::from_millis(300));
    // Un titre vide donne le même repli, sur l'autre onglet (intervalle propre à chacun).
    let autre = b.f.onglets().into_iter().find(|x| !Rc::ptr_eq(x, &b.o));
    let autre = autre.ok_or("un seul onglet")?;
    autre.renommer(Some("Autre"));
    b.fond.notification(2, Some("  "), "fini");
    let ok2 = attendre(DELAI, || b.bulles.borrow().len() >= 2);
    let bulles = b.bulles.borrow().clone();
    b.fermer();
    verifier(ok && ok2 && bulles.len() == 2, || {
        format!("bulles : {bulles:?}")
    })?;
    verifier(
        bulles[0].titre == "Serveur" && bulles[0].corps == "prêt",
        || format!("bulle 1 : {:?}", bulles[0]),
    )?;
    verifier(
        bulles[1].titre == "Autre" && bulles[1].id == "onglet-2",
        || format!("bulle 2 : {:?}", bulles[1]),
    )
}

fn config_bulle_sans_case_onglet() -> Config {
    Config {
        notifications: ReglagesNotifications {
            fin_commande_onglet: false,
            programme_onglet: false,
            ..ReglagesNotifications::default()
        },
        ..Config::default()
    }
}

fn bulle_force_indicateur_programme() -> Resultat {
    let b = Banc::nouveau(config_bulle_sans_case_onglet());
    b.cacher_premier()?;
    b.fond.notification(1, Some("T"), "corps");
    let ok = attendre(DELAI, || !b.bulles.borrow().is_empty());
    let r = b.attendre_indicateur(&b.o, Indicateur::Signale);
    let n = b.bulles.borrow().len();
    b.fermer();
    r?;
    verifier(ok && n == 1, || format!("{n} bulle(s)"))
}

fn bulle_force_indicateur_commande() -> Resultat {
    let b = Banc::nouveau(config_bulle_sans_case_onglet());
    b.cacher_premier()?;
    pupitre::commandes::forcer_fenetres_inactives(true);
    b.fond.commande(1, 0, Some(20_000));
    let ok = attendre(DELAI, || !b.bulles.borrow().is_empty());
    let r = b.attendre_indicateur(&b.o, Indicateur::Reussi);
    let n = b.bulles.borrow().len();
    b.fermer();
    r?;
    verifier(ok && n == 1, || format!("{n} bulle(s)"))
}

fn copie_message() -> Resultat {
    let b = Banc::nouveau(Config::default());
    b.fond.envoyer(ServerMsg::PressePapiers {
        id: 1,
        texte: "abc".into(),
    });
    let attendu = "Un programme a copié 3 caractères.";
    let vu = attendre(DELAI, || b.f.messages().iter().any(|m| m == attendu));
    let messages = b.f.messages();
    let copie = lire_presse_papiers();
    // Le message est bref (3 s).
    let parti = attendre(Duration::from_secs(6), || {
        !b.f.messages().iter().any(|m| m == attendu)
    });
    b.fermer();
    verifier(vu, || format!("messages : {messages:?}"))?;
    verifier(copie.as_deref() == Some("abc"), || {
        format!("presse-papiers : {copie:?}")
    })?;
    verifier(parti, || "message resté affiché".into())
}

fn copie_desactivee() -> Resultat {
    let config = Config {
        notifications: ReglagesNotifications {
            presse_papiers: false,
            ..ReglagesNotifications::default()
        },
        ..Config::default()
    };
    let b = Banc::nouveau(config);
    let cb = gdk::Display::default().unwrap().clipboard();
    cb.set_text("avant");
    patienter(Duration::from_millis(200));
    b.fond.envoyer(ServerMsg::PressePapiers {
        id: 1,
        texte: "abc".into(),
    });
    let r = b.synchroniser();
    patienter(Duration::from_millis(300));
    let messages = b.f.messages();
    let copie = lire_presse_papiers();
    b.fermer();
    r?;
    verifier(copie.as_deref() == Some("avant"), || {
        format!("presse-papiers : {copie:?}")
    })?;
    verifier(messages.is_empty(), || format!("messages : {messages:?}"))
}

fn rafale() -> Resultat {
    let b = Banc::nouveau(Config::default());
    let debut = Instant::now();
    for i in 0..100 {
        b.fond.notification(1, Some("rafale"), &format!("n° {i}"));
        patienter(Duration::from_millis(10));
    }
    let r = b.synchroniser();
    let duree = debut.elapsed();
    let bulles = b.bulles.borrow().len();
    // L'interface répond encore et l'indicateur est posé.
    let signale = b.o.indicateur();
    b.fermer();
    r?;
    verifier(duree < Duration::from_secs(15), || {
        format!("rafale traitée en {duree:?}")
    })?;
    verifier(bulles == 1, || {
        format!("{bulles} bulles pour 100 notifications")
    })?;
    verifier(signale == Indicateur::Signale, || {
        format!("indicateur {signale:?}")
    })
}

fn bulle_hors_premier_plan() -> Resultat {
    let b = Banc::nouveau(Config::default());
    // Fenêtre active : jamais de bulle, même pour une longue commande.
    b.fond.commande(1, 0, Some(20_000));
    patienter(Duration::from_millis(800));
    let active = b.bulles.borrow().len();
    // Aucune fenêtre active : bulle pour une commande de plus de 10 s seulement.
    pupitre::commandes::forcer_fenetres_inactives(true);
    b.fond.commande(1, 0, Some(5_000));
    b.fond.commande(1, 0, None);
    patienter(Duration::from_millis(800));
    let courtes = b.bulles.borrow().len();
    b.fond.commande(1, 3, Some(20_000));
    attendre(DELAI, || !b.bulles.borrow().is_empty());
    let longue = b.bulles.borrow().clone();
    // Une seconde longue commande du même onglet, tout de suite : intervalle de 10 s.
    b.fond.commande(1, 0, Some(30_000));
    patienter(Duration::from_millis(800));
    let apres = b.bulles.borrow().len();
    let indicateur = b.o.indicateur();
    b.fermer();
    verifier(active == 0, || format!("{active} bulle(s) fenêtre active"))?;
    verifier(courtes == 0, || format!("{courtes} bulle(s) pour <= 10 s"))?;
    verifier(longue.len() == 1, || format!("bulles : {longue:?}"))?;
    verifier(apres == 1, || format!("{apres} bulles : intervalle ignoré"))?;
    // Jamais un seul canal : la bulle est doublée d'un indicateur d'onglet.
    verifier(indicateur != Indicateur::Aucun, || {
        "bulle sans indicateur d'onglet".into()
    })
}

fn reglages_coupes() -> Resultat {
    let config = Config {
        notifications: ReglagesNotifications {
            fin_commande_onglet: false,
            fin_commande_bulle: false,
            programme_onglet: false,
            programme_bulle: false,
            cloche_onglet: false,
            presse_papiers: true,
        },
        ..Config::default()
    };
    let b = Banc::nouveau(config);
    b.cacher_premier()?;
    pupitre::commandes::forcer_fenetres_inactives(true);
    b.fond.commande(1, 0, Some(20_000));
    b.fond.notification(1, None, "x");
    b.o.terminal().on_output(b"\x07");
    patienter(Duration::from_millis(300));
    let i = b.o.indicateur();
    let n = b.bulles.borrow().len();
    b.fermer();
    verifier(i == Indicateur::Aucun, || format!("indicateur {i:?}"))?;
    verifier(n == 0, || format!("{n} bulle(s)"))
}

fn montrer_onglet() -> Resultat {
    let b = Banc::nouveau(Config::default());
    b.cacher_premier()?;
    verifier(
        b.f.onglet_actif().is_some_and(|a| !Rc::ptr_eq(&a, &b.o)),
        || "le premier onglet devait être caché".into(),
    )?;
    b.app
        .activate_action("montrer-onglet", Some(&1u64.to_variant()));
    let ok = attendre(DELAI, || {
        b.f.onglet_actif().is_some_and(|a| Rc::ptr_eq(&a, &b.o))
    });
    // Un numéro inconnu est sans effet (et sans plantage).
    b.app
        .activate_action("montrer-onglet", Some(&999u64.to_variant()));
    patienter(Duration::from_millis(200));
    b.fermer();
    verifier(ok, || "l'action n'a pas montré l'onglet".into())
}
