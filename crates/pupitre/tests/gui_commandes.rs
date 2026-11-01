// SPDX-License-Identifier: GPL-3.0-or-later
//! Tests graphiques de la fin des commandes (annonce à Orca) et de la navigation de
//! commande en commande (Task 7). Lancés par `scripts/test-x11.sh` et
//! `scripts/test-wayland.sh` ; ignorés sinon.
//!
//! Le fond est un faux processus de fond dans ce processus (fil à part) : il parle le
//! protocole v3 et envoie à la demande des octets et des marques OSC 133. Aucun vrai shell,
//! aucun vrai fond, ni trousseau. Les annonces sont relevées dans le journal de l'onglet
//! (`Onglet::annonces`) ; l'une d'elles est aussi vérifiée jusqu'au bus d'accessibilité.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod commun;

use commun::*;
use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use pupitre::fenetre::{FenetrePupitre, Onglet, Pupitre};
use pupitre::polices::{dossier_polices, enregistrer_polices_embarquees};
use pupitre_config::{AnnonceFin, Config};
use pupitre_proto::{read_frame, write_frame, ClientMsg, MarqueCommande, ServerMsg, SessionInfo};
use std::io::Read;
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use vte4::prelude::*;

const NOM_APPLI: &str = "pupitre-test-commandes";
const CTRL_MAJ: gdk::ModifierType =
    gdk::ModifierType::CONTROL_MASK.union(gdk::ModifierType::SHIFT_MASK);
/// Codes matériels (evdev + 8) des flèches haut et bas.
const CODE_HAUT: u32 = 111;
const CODE_BAS: u32 = 116;
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
    ex.lancer("annonce_commande_longue", annonce_commande_longue);
    ex.lancer(
        "pas_d_annonce_commande_courte",
        pas_d_annonce_commande_courte,
    );
    ex.lancer("annonce_echec", annonce_echec);
    ex.lancer("annonce_toutes_et_aucune", annonce_toutes_et_aucune);
    ex.lancer("onglet_cache_sans_annonce", onglet_cache_sans_annonce);
    ex.lancer(
        "marques_rejouees_sans_annonce",
        marques_rejouees_sans_annonce,
    );
    ex.lancer(
        "navigation_precedente_suivante",
        navigation_precedente_suivante,
    );
    ex.lancer("navigation_bout_de_liste", navigation_bout_de_liste);
    ex.lancer("navigation_commande_en_echec", navigation_echec);
    ex.lancer("navigation_invite_repetee", navigation_invite_repetee);
    ex.lancer("navigation_sans_saisie", navigation_sans_saisie);
    ex.lancer("navigation_commande_vide", navigation_commande_vide);
    ex.lancer("navigation_historique_borne", navigation_historique_borne);
    ex.lancer("onglet_cache_puis_navigation", onglet_cache_puis_navigation);
    ex.lancer("largeur_changee_garde_la_navigation", largeur_changee);
    ex.lancer("reponse_tardive_pas_envoyee", reponse_tardive);
    ex.lancer("entree_vide_et_ctrl_c", entree_vide_et_ctrl_c);
    ex.lancer("ecran_alternatif_pendant_un_zoom", ecran_alternatif);
    ex.lancer("cout_borne_du_redimensionnement", cout_borne);
    ex.lancer("raccourcis_sans_conflit", raccourcis_sans_conflit);
    ex.conclure();
}

/// Un morceau de flux du faux fond : des octets ou une marque.
enum Morceau {
    O(String),
    M(MarqueCommande),
}
use Morceau::{M, O};

/// Octets et marques (avec leurs décalages) d'un rejeu.
fn rejeu(morceaux: Vec<Morceau>) -> (Vec<u8>, Vec<(u32, MarqueCommande)>) {
    let mut data = Vec::new();
    let mut marques = Vec::new();
    for m in morceaux {
        match m {
            O(t) => data.extend(t.as_bytes()),
            M(m) => marques.push((u32::try_from(data.len()).unwrap(), m)),
        }
    }
    (data, marques)
}

/// Faux processus de fond : une session (numéro 1) qui ne fait que ce qu'on lui envoie.
struct FauxFond {
    dossier: tempfile::TempDir,
    flux: Arc<Mutex<Option<UnixStream>>>,
    /// Octets reçus de l'interface en `Input` (ce que le shell recevrait).
    entrees: Arc<Mutex<Vec<u8>>>,
}

type Rejeu = Option<(Vec<u8>, Vec<(u32, MarqueCommande)>)>;

impl FauxFond {
    fn demarrer(rejeu: Rejeu) -> FauxFond {
        use std::os::unix::fs::DirBuilderExt;
        let dossier = tempfile::tempdir().unwrap();
        let sous = dossier.path().join("pupitre");
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&sous)
            .unwrap();
        let ecoute = UnixListener::bind(sous.join("fond.sock")).unwrap();
        let flux = Arc::new(Mutex::new(None));
        let entrees = Arc::new(Mutex::new(Vec::new()));
        let (f2, e2) = (flux.clone(), entrees.clone());
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
            let mut rejeu = rejeu;
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
                    ClientMsg::Attach { id } => {
                        let (data, marques) = rejeu.take().unwrap_or_default();
                        envoyer(ServerMsg::ReplayMarque { id, data, marques });
                    }
                    ClientMsg::Open { .. } | ClientMsg::Ouvrir { .. } => {
                        envoyer(ServerMsg::Opened { id: prochain });
                        prochain += 1;
                    }
                    ClientMsg::Input { data, .. } => e2.lock().unwrap().extend(data),
                    _ => {}
                }
            }
        });
        FauxFond {
            dossier,
            flux,
            entrees,
        }
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

    fn marque(&self, id: u64, marque: MarqueCommande, rejouee: bool) {
        self.envoyer(ServerMsg::Marque {
            id,
            marque,
            rejouee,
        });
    }

    /// Une invite, une saisie, l'exécution et la fin d'une commande de `lignes` lignes de
    /// sortie, comme un shell avec OSC 133. L'invite suivante n'est pas envoyée.
    fn commande(&self, id: u64, texte: &str, lignes: usize, code: i32, duree_ms: Option<u64>) {
        self.marque(id, Invite, false);
        self.octets(id, "$ ");
        self.marque(id, Saisie, false);
        self.octets(id, &format!("{texte}\r\n"));
        self.marque(id, Execution, false);
        let sortie: String = (0..lignes).map(|i| format!("sortie {i}\r\n")).collect();
        self.octets(id, &sortie);
        self.marque(
            id,
            Fin {
                code: Some(code),
                duree_ms,
            },
            false,
        );
    }
}

static NUMERO: AtomicU32 = AtomicU32::new(0);

/// Interface et faux fond prêts : un onglet affiché dans une fenêtre active.
struct Banc {
    fond: FauxFond,
    p: Rc<Pupitre>,
    f: FenetrePupitre,
    o: Rc<Onglet>,
}

impl Banc {
    fn nouveau(config: Config, rejeu: Rejeu) -> Banc {
        let fond = FauxFond::demarrer(rejeu);
        let app = gtk::Application::builder()
            .application_id(format!(
                "fr.pupitre.TestCommandes{}",
                NUMERO.fetch_add(1, Ordering::SeqCst)
            ))
            .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(gtk::gio::Cancellable::NONE).unwrap();
        let p = Pupitre::demarrer(&app, fond.socket(), config);
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
        // Weston sans écran ne donne le focus à aucune fenêtre : elle est alors tenue pour
        // active par l'application (X11 : vraie activation).
        if !attendre(Duration::from_secs(3), || f.fenetre().is_active()) {
            assert!(
                std::env::var_os("WAYLAND_DISPLAY").is_some(),
                "la fenêtre n'est jamais devenue active"
            );
            pupitre::commandes::forcer_fenetre_active(true);
        }
        let o = f.onglets()[0].clone();
        o.terminal().terminal().grab_focus();
        Banc { fond, p, f, o }
    }

    fn fermer(self) {
        for f in self.p.fenetres() {
            f.fenetre().destroy();
        }
        patienter(Duration::from_millis(200));
    }

    /// Attend que l'onglet connaisse `n` commandes (les marques sont traitées dans l'ordre).
    fn attendre_commandes(&self, n: usize) -> Resultat {
        verifier(attendre(DELAI, || self.o.nombre_commandes() >= n), || {
            format!(
                "{} commande(s) connue(s) au lieu de {n}",
                self.o.nombre_commandes()
            )
        })
    }

    /// Attend que le journal d'annonces compte `n` entrées.
    fn attendre_annonces(&self, n: usize) -> Resultat {
        verifier(attendre(DELAI, || self.o.annonces().len() >= n), || {
            format!(
                "annonces : {:?} (au moins {n} attendue(s))",
                self.o.annonces()
            )
        })
    }

    /// Ctrl+Maj+↑ (`haut`) ou ↓ ; attend la nouvelle annonce et la rend.
    fn naviguer(&self, haut: bool) -> Result<String, String> {
        let avant = self.o.annonces().len();
        let (touche, code) = if haut {
            (gdk::Key::Up, CODE_HAUT)
        } else {
            (gdk::Key::Down, CODE_BAS)
        };
        verifier(self.f.traiter_touche(touche, code, CTRL_MAJ), || {
            "touche non consommée".into()
        })?;
        self.attendre_annonces(avant + 1)?;
        Ok(self.o.annonces().pop().unwrap_or_default())
    }

    /// Première ligne du texte visible du terminal.
    fn haut_visible(&self) -> String {
        self.o
            .terminal()
            .terminal()
            .text_format(vte4::Format::Text)
            .map(|s| s.lines().next().unwrap_or("").to_string())
            .unwrap_or_default()
    }
}

fn attendu(r: Result<String, String>, voulu: &str) -> Resultat {
    let a = r?;
    verifier(a == voulu, || {
        format!("annonce « {a} » au lieu de « {voulu} »")
    })
}

fn config_annonce(a: AnnonceFin) -> Config {
    Config {
        annonce_fin_commande: a,
        ..Config::default()
    }
}

/// Trois commandes de 40 lignes de sortie, puis l'invite suivante (sans commande).
fn trois_commandes(b: &Banc) -> Resultat {
    for (i, t) in ["echo un", "echo deux", "echo trois"].iter().enumerate() {
        b.fond.commande(1, t, 40, 0, Some(100));
        b.attendre_commandes(i + 1)?;
    }
    b.fond.marque(1, Invite, false);
    b.fond.octets(1, "$ ");
    b.fond.marque(1, Saisie, false);
    patienter(Duration::from_millis(300));
    Ok(())
}

fn annonce_commande_longue() -> Resultat {
    let b = Banc::nouveau(Config::default(), None);
    // Écoute AT-SPI : l'annonce part vraiment vers le lecteur d'écran. On attend que
    // l'écoute soit abonnée (quelques secondes, machine chargée) avant d'envoyer la marque.
    let mut ecoute = Command::new("python3")
        .arg(racine().join("tests/a11y/ecoute.py"))
        .args(["--app", NOM_APPLI, "--duree", "12"])
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let lignes = Arc::new(Mutex::new(String::new()));
    if let Some(mut s) = ecoute.stdout.take() {
        let l = lignes.clone();
        thread::spawn(move || {
            let mut t = String::new();
            let _ = s.read_to_string(&mut t);
            l.lock().unwrap().push_str(&t);
        });
    }
    patienter(Duration::from_secs(4));
    // Comme Orca, interroger l'arbre d'accessibilité : l'objet du terminal n'est publié
    // qu'à la première interrogation (sans elle, l'annonce y est perdue sous Weston).
    let _ = sonder(NOM_APPLI);
    patienter(Duration::from_secs(1));
    b.fond.marque(1, Execution, false);
    b.fond.marque(
        1,
        Fin {
            code: Some(0),
            duree_ms: Some(6000),
        },
        false,
    );
    let r = b.attendre_annonces(1);
    attendre(Duration::from_secs(20), || {
        matches!(ecoute.try_wait(), Ok(Some(_)))
    });
    let _ = ecoute.kill();
    let _ = ecoute.wait();
    patienter(Duration::from_millis(300));
    let sortie = lignes.lock().unwrap().clone();
    let annonces = b.o.annonces();
    b.fermer();
    r?;
    verifier(annonces == ["Commande terminée en 6 s."], || {
        format!("journal : {annonces:?}")
    })?;
    verifier(
        sortie
            .lines()
            .any(|l| l == "annonce\tCommande terminée en 6 s."),
        || format!("annonce non reçue par AT-SPI ; écoute : {sortie:?}"),
    )?;
    // Faite sur l'objet accessible du terminal (rôle « terminal »), pas sur la fenêtre.
    verifier(sortie.lines().any(|l| l == "source\tterminal"), || {
        format!("annonce pas faite sur le terminal ; écoute : {sortie:?}")
    })
}

fn pas_d_annonce_commande_courte() -> Resultat {
    let b = Banc::nouveau(Config::default(), None);
    b.fond.commande(1, "ls", 2, 0, Some(1200));
    b.fond.commande(1, "true", 0, 0, None);
    // Seuil : 4,999 s n'est pas long, 5 s l'est.
    b.fond.commande(1, "sleep 4", 0, 0, Some(4999));
    let r = b.attendre_commandes(3);
    patienter(Duration::from_millis(700));
    let a = b.o.annonces();
    b.fermer();
    r?;
    verifier(a.is_empty(), || format!("annonces inattendues : {a:?}"))
}

fn annonce_echec() -> Resultat {
    let b = Banc::nouveau(Config::default(), None);
    b.fond.commande(1, "make", 0, 2, Some(12_000));
    let r = b.attendre_annonces(1);
    let a = b.o.annonces();
    b.fermer();
    r?;
    verifier(a == ["Commande en échec, code 2, en 12 s."], || {
        format!("{a:?}")
    })
}

fn annonce_toutes_et_aucune() -> Resultat {
    let b = Banc::nouveau(config_annonce(AnnonceFin::Toutes), None);
    b.fond.commande(1, "ls", 0, 0, Some(800));
    b.fond.commande(1, "x", 0, 1, None);
    b.fond.commande(1, "long", 0, 0, Some(125_000));
    let r = b.attendre_annonces(3);
    let a = b.o.annonces();
    let attendues = [
        "Commande terminée en moins d'une seconde.",
        "Commande en échec, code 1.",
        "Commande terminée en 2 min 5 s.",
    ];
    if r.is_err() || a != attendues {
        b.fermer();
        r?;
        return Err(format!("toutes : {a:?}"));
    }
    // Réglage « aucune » appliqué à chaud.
    b.p.appliquer_config(Ok(config_annonce(AnnonceFin::Aucune)));
    b.fond.commande(1, "encore", 0, 0, Some(30_000));
    let r = b.attendre_commandes(4);
    patienter(Duration::from_millis(700));
    let n = b.o.annonces().len();
    b.fermer();
    r?;
    verifier(n == 3, || {
        format!("{n} annonces avec « aucune » (3 attendues)")
    })
}

fn onglet_cache_sans_annonce() -> Resultat {
    let b = Banc::nouveau(Config::default(), None);
    b.f.ouvrir_session();
    let ok = attendre(DELAI, || b.f.onglets().len() == 2);
    verifier(ok, || "deuxième onglet jamais ouvert".into())?;
    // L'onglet 1 est caché : sa commande longue n'est pas annoncée.
    b.fond.commande(1, "make", 0, 0, Some(20_000));
    let r = b.attendre_commandes(1);
    patienter(Duration::from_millis(700));
    let cache = b.o.annonces();
    // De retour sur l'onglet 1 : une nouvelle commande longue est annoncée.
    b.f.traiter_touche(gdk::Key::_1, 10, gdk::ModifierType::ALT_MASK);
    patienter(Duration::from_millis(300));
    b.fond.commande(1, "make", 0, 0, Some(20_000));
    let r2 = b.attendre_annonces(1);
    let visible = b.o.annonces();
    b.fermer();
    r?;
    verifier(cache.is_empty(), || {
        format!("annoncé en étant caché : {cache:?}")
    })?;
    r2?;
    verifier(visible == ["Commande terminée en 20 s."], || {
        format!("{visible:?}")
    })
}

fn marques_rejouees_sans_annonce() -> Resultat {
    let fin = |code, duree| Fin {
        code: Some(code),
        duree_ms: Some(duree),
    };
    let r = rejeu(vec![
        M(Invite),
        O("$ ".into()),
        M(Saisie),
        O("make\r\n".into()),
        M(Execution),
        O("fait\r\n".into()),
        M(fin(0, 9000)),
        M(Invite),
        O("$ ".into()),
        M(Saisie),
        O("make clean\r\n".into()),
        M(Execution),
        O("fait\r\n".into()),
        M(fin(3, 30_000)),
        M(Invite),
        O("$ ".into()),
        M(Saisie),
    ]);
    let b = Banc::nouveau(Config::default(), Some(r));
    let r1 = b.attendre_commandes(2);
    // Une marque marquée « rejouée » en direct : jamais d'annonce non plus.
    b.fond.marque(1, Execution, true);
    b.fond.marque(1, fin(0, 40_000), true);
    patienter(Duration::from_millis(700));
    let a = b.o.annonces();
    // Mais elles servent à la navigation : d'abord la commande lancée en direct (sans
    // texte), puis les deux du rejeu.
    let n1 = b.naviguer(true);
    let n2 = b.naviguer(true);
    let n3 = b.naviguer(true);
    let n4 = b.naviguer(true);
    b.fermer();
    r1?;
    verifier(a.is_empty(), || format!("annonces pour du rejeu : {a:?}"))?;
    attendu(n1, "Commande vide.")?;
    attendu(n2, "Commande : make clean, en échec, code 3")?;
    attendu(n3, "Commande : make")?;
    attendu(n4, "Aucune commande avant.")
}

fn navigation_precedente_suivante() -> Resultat {
    let b = Banc::nouveau(Config::default(), None);
    let res = (|| {
        trois_commandes(&b)?;
        attendu(b.naviguer(true), "Commande : echo trois")?;
        attendu(b.naviguer(true), "Commande : echo deux")?;
        verifier(b.haut_visible() == "$ echo deux", || {
            format!("haut de la vue : « {} »", b.haut_visible())
        })?;
        attendu(b.naviguer(false), "Commande : echo trois")?;
        verifier(b.haut_visible() == "$ echo trois", || {
            format!("haut de la vue : « {} »", b.haut_visible())
        })
    })();
    b.fermer();
    res
}

fn navigation_bout_de_liste() -> Resultat {
    let b = Banc::nouveau(Config::default(), None);
    let res = (|| {
        trois_commandes(&b)?;
        attendu(b.naviguer(false), "Aucune commande après.")?;
        attendu(b.naviguer(true), "Commande : echo trois")?;
        attendu(b.naviguer(true), "Commande : echo deux")?;
        attendu(b.naviguer(true), "Commande : echo un")?;
        attendu(b.naviguer(true), "Aucune commande avant.")?;
        attendu(b.naviguer(true), "Aucune commande avant.")?;
        verifier(b.haut_visible() == "$ echo un", || {
            format!("haut de la vue : « {} »", b.haut_visible())
        })?;
        attendu(b.naviguer(false), "Commande : echo deux")
    })();
    b.fermer();
    res
}

fn navigation_echec() -> Resultat {
    let b = Banc::nouveau(Config::default(), None);
    b.fond.commande(1, "exit 3", 1, 3, Some(10));
    let res = b
        .attendre_commandes(1)
        .and_then(|()| attendu(b.naviguer(true), "Commande : exit 3, en échec, code 3"));
    b.fermer();
    res
}

fn navigation_invite_repetee() -> Resultat {
    let b = Banc::nouveau(Config::default(), None);
    // Nushell redessine l'invite à chaque frappe : A, B, A, B… avant C.
    for _ in 0..3 {
        b.fond.marque(1, Invite, false);
        b.fond.octets(1, "\r$ ");
        b.fond.marque(1, Saisie, false);
    }
    b.fond.octets(1, "ls\r\n");
    b.fond.marque(1, Execution, false);
    b.fond.marque(
        1,
        Fin {
            code: Some(0),
            duree_ms: Some(5),
        },
        false,
    );
    let res = b.attendre_commandes(1).and_then(|()| {
        patienter(Duration::from_millis(300));
        verifier(b.o.nombre_commandes() == 1, || {
            format!("{} commandes pour une seule", b.o.nombre_commandes())
        })?;
        attendu(b.naviguer(true), "Commande : ls")?;
        attendu(b.naviguer(true), "Aucune commande avant.")
    });
    b.fermer();
    res
}

fn navigation_sans_saisie() -> Resultat {
    // Pas de marque B (elvish) : toute la ligne de l'invite est lue.
    let b = Banc::nouveau(Config::default(), None);
    b.fond.marque(1, Invite, false);
    b.fond.octets(1, "$ ls -l\r\n");
    b.fond.marque(1, Execution, false);
    b.fond.octets(1, "total 0\r\n");
    b.fond.marque(
        1,
        Fin {
            code: Some(0),
            duree_ms: None,
        },
        false,
    );
    let res = b
        .attendre_commandes(1)
        .and_then(|()| attendu(b.naviguer(true), "Commande : $ ls -l"));
    b.fermer();
    res
}

fn navigation_commande_vide() -> Resultat {
    let b = Banc::nouveau(Config::default(), None);
    b.fond.commande(1, "", 0, 0, Some(1));
    let res = b
        .attendre_commandes(1)
        .and_then(|()| attendu(b.naviguer(true), "Commande vide."));
    b.fermer();
    res
}

fn navigation_historique_borne() -> Resultat {
    // Historique de 60 lignes : les premières commandes en sortent et sont oubliées.
    let config = Config {
        historique_lignes: 60,
        ..Config::default()
    };
    let b = Banc::nouveau(config, None);
    let total = 15;
    for i in 0..total {
        b.fond.commande(1, &format!("echo {i}"), 9, 0, Some(10));
    }
    let res = (|| {
        b.attendre_commandes(total)?;
        patienter(Duration::from_millis(500));
        let mut vues = Vec::new();
        for _ in 0..total + 2 {
            let a = b
                .naviguer(true)
                .map_err(|e| format!("{e} ; vues : {vues:?}"))?;
            if a == "Aucune commande avant." {
                break;
            }
            vues.push(a);
        }
        verifier(!vues.is_empty() && vues.len() < total, || {
            format!("{} commandes visées sur {total} : {vues:?}", vues.len())
        })?;
        // La plus ancienne visée doit encore être à l'écran ou dans l'historique.
        let derniere = vues.last().cloned().unwrap_or_default();
        let texte =
            b.o.terminal()
                .terminal()
                .text_format(vte4::Format::Text)
                .map(|s| s.to_string())
                .unwrap_or_default();
        let nom = derniere.trim_start_matches("Commande : ");
        verifier(texte.contains(&format!("$ {nom}")), || {
            format!("« {derniere} » visée mais absente de la vue :\n{texte}")
        })
    })();
    b.fermer();
    res
}

fn raccourcis_sans_conflit() -> Resultat {
    let b = Banc::nouveau(Config::default(), None);
    let haut = b.f.traiter_touche(gdk::Key::Up, CODE_HAUT, CTRL_MAJ);
    let bas = b.f.traiter_touche(gdk::Key::Down, CODE_BAS, CTRL_MAJ);
    // ↑ et ↓ seules restent au shell.
    let seule =
        b.f.traiter_touche(gdk::Key::Up, CODE_HAUT, gdk::ModifierType::empty());
    patienter(Duration::from_millis(500));
    let recu = b.fond.entrees.lock().unwrap().clone();
    b.fermer();
    verifier(haut && bas, || "Ctrl+Maj+↑/↓ non consommés".into())?;
    verifier(!seule, || "↑ seule consommée".into())?;
    verifier(recu.is_empty(), || {
        format!("octets envoyés au shell : {recu:?}")
    })
}

/// Marques reçues pendant que l'onglet est caché : les lignes notées sont les bonnes.
fn onglet_cache_puis_navigation() -> Resultat {
    let b = Banc::nouveau(Config::default(), None);
    let res = (|| {
        b.f.ouvrir_session();
        verifier(attendre(DELAI, || b.f.onglets().len() == 2), || {
            "deuxième onglet jamais ouvert".into()
        })?;
        patienter(Duration::from_millis(300));
        for t in ["echo un", "echo deux", "echo trois"] {
            b.fond.commande(1, t, 40, 0, Some(100));
        }
        b.fond.marque(1, Invite, false);
        b.fond.octets(1, "$ ");
        b.fond.marque(1, Saisie, false);
        patienter(Duration::from_millis(700));
        // De retour sur l'onglet 1.
        b.f.traiter_touche(gdk::Key::_1, 10, gdk::ModifierType::ALT_MASK);
        b.attendre_commandes(4)?;
        patienter(Duration::from_millis(300));
        attendu(b.naviguer(true), "Commande : echo trois")?;
        attendu(b.naviguer(true), "Commande : echo deux")?;
        verifier(b.haut_visible() == "$ echo deux", || {
            format!("haut de la vue : « {} »", b.haut_visible())
        })
    })();
    b.fermer();
    res
}

/// Zoom (donc largeur de grille) changé : une commande trop longue pour une ligne, repliée
/// puis dépliée par VTE, est retrouvée avec son texte entier.
fn largeur_changee() -> Resultat {
    let b = Banc::nouveau(Config::default(), None);
    let longue = format!("echo {}", "x".repeat(115));
    let res = (|| {
        for t in [longue.as_str(), "echo deux", "echo trois"] {
            b.fond.commande(1, t, 30, 0, Some(100));
        }
        b.attendre_commandes(3)?;
        patienter(Duration::from_millis(400));
        let avant = b.o.terminal().grille().map(|g| g.0).unwrap_or(0);
        // Plus petit : plus de colonnes, la ligne repliée se déplie.
        b.o.terminal().zoomer(Some(-5), 200);
        patienter(Duration::from_millis(800));
        let apres = b.o.terminal().grille().map(|g| g.0).unwrap_or(0);
        verifier(apres > avant, || format!("colonnes {avant} puis {apres}"))?;
        verifier(b.o.nombre_commandes() == 3, || {
            format!("{} commandes après le zoom", b.o.nombre_commandes())
        })?;
        attendu(b.naviguer(true), "Commande : echo trois")?;
        attendu(b.naviguer(true), "Commande : echo deux")?;
        attendu(b.naviguer(true), &format!("Commande : {longue}"))?;
        // Plus grand : repli de nouveau.
        b.o.terminal().zoomer(Some(10), 200);
        patienter(Duration::from_millis(800));
        attendu(b.naviguer(false), "Commande : echo deux")?;
        attendu(b.naviguer(false), "Commande : echo trois")?;
        attendu(b.naviguer(true), "Commande : echo deux")?;
        attendu(b.naviguer(true), &format!("Commande : {longue}"))?;
        verifier(b.haut_visible().starts_with("$ echo xxx"), || {
            format!("haut de la vue : « {} »", b.haut_visible())
        })
    })();
    b.fermer();
    res
}

/// La réponse de VTE à la demande de position arrive après l'expiration de l'attente : elle
/// est gardée, jamais envoyée au programme.
fn reponse_tardive() -> Resultat {
    let b = Banc::nouveau(Config::default(), None);
    let res = (|| {
        b.fond.commande(1, "echo un", 3, 0, Some(10));
        b.attendre_commandes(1)?;
        patienter(Duration::from_millis(300));
        // Délai nul : l'attente expire avant que VTE n'ait répondu.
        b.o.terminal().definir_delai_attente(Duration::ZERO);
        let avant = b.o.terminal().reponses_gardees();
        verifier(
            b.f.traiter_touche(gdk::Key::Up, CODE_HAUT, CTRL_MAJ),
            || "touche non consommée".into(),
        )?;
        let ok = attendre(DELAI, || b.o.terminal().reponses_gardees() > avant);
        verifier(ok, || "réponse de position jamais reçue".into())?;
        patienter(Duration::from_millis(500));
        let recu = b.fond.entrees.lock().unwrap().clone();
        verifier(recu.is_empty(), || {
            format!("octets envoyés au shell : {recu:?}")
        })
    })();
    b.fermer();
    res
}

/// Entrée vide puis Ctrl+C à l'invite (bash, zsh : D sans C) : aucune entrée navigable, aucune
/// annonce, même avec « toutes ».
fn entree_vide_et_ctrl_c() -> Resultat {
    let b = Banc::nouveau(config_annonce(AnnonceFin::Toutes), None);
    let d = |code| Fin {
        code: Some(code),
        duree_ms: None,
    };
    for code in [0, 130] {
        b.fond.marque(1, Invite, false);
        b.fond.octets(1, "$ ");
        b.fond.marque(1, Saisie, false);
        b.fond.octets(1, "\r\n");
        b.fond.marque(1, d(code), false);
    }
    b.fond.marque(1, Invite, false);
    b.fond.octets(1, "$ ");
    b.fond.marque(1, Saisie, false);
    let res = b.attendre_commandes(1).and_then(|()| {
        patienter(Duration::from_millis(700));
        verifier(b.o.nombre_commandes() == 1, || {
            format!("{} entrées pour une invite", b.o.nombre_commandes())
        })?;
        verifier(b.o.annonces().is_empty(), || {
            format!("annonces : {:?}", b.o.annonces())
        })?;
        attendu(b.naviguer(true), "Aucune commande avant.")
    });
    b.fermer();
    res
}

/// Écran alternatif (vim, less) affiché pendant un zoom : la navigation n'est pas effacée.
fn ecran_alternatif() -> Resultat {
    let b = Banc::nouveau(Config::default(), None);
    let res = (|| {
        trois_commandes(&b)?;
        b.fond.octets(1, "\x1b[?1049h\x1b[H\x1b[2Jvim");
        patienter(Duration::from_millis(400));
        b.o.terminal().zoomer(Some(-3), 200);
        patienter(Duration::from_millis(800));
        b.o.terminal().zoomer(Some(2), 200);
        patienter(Duration::from_millis(800));
        b.fond.octets(1, "\x1b[?1049l");
        patienter(Duration::from_millis(500));
        verifier(b.o.nombre_commandes() >= 3, || {
            format!(
                "{} commandes après l'écran alternatif",
                b.o.nombre_commandes()
            )
        })?;
        attendu(b.naviguer(true), "Commande : echo trois")?;
        attendu(b.naviguer(true), "Commande : echo deux")?;
        attendu(b.naviguer(true), "Commande : echo un")
    })();
    b.fermer();
    res
}

/// Historique de 10 000 lignes, environ 500 commandes notées, 20 changements de largeur
/// d'un même geste : le coût des conversions est mesuré et borné, la navigation reste juste.
fn cout_borne() -> Resultat {
    let config = Config {
        historique_lignes: 10_000,
        ..Config::default()
    };
    let b = Banc::nouveau(config, None);
    let total = 480;
    let res = (|| {
        for i in 0..total {
            b.fond.commande(1, &format!("echo {i}"), 19, 0, Some(10));
        }
        // Chaque marque attend VTE : quelques dizaines de secondes pour 480 commandes.
        verifier(
            attendre(Duration::from_secs(120), || b.o.nombre_commandes() >= total),
            || format!("{} commandes sur {total}", b.o.nombre_commandes()),
        )?;
        patienter(Duration::from_millis(600));
        let (t0, n0, _) = b.o.terminal().conversions();
        let debut = std::time::Instant::now();
        for i in 0..20 {
            b.o.terminal()
                .zoomer(Some(if i % 2 == 0 { -1 } else { 1 }), 200);
            patienter(Duration::from_millis(30));
        }
        patienter(Duration::from_millis(900));
        let ecoule = debut.elapsed();
        let (t, n, imprecis) = b.o.terminal().conversions();
        println!(
            "  mesure : {} conversions en {:?} (20 changements de largeur, {ecoule:?} de geste), {imprecis} non suivi(s)",
            n - n0,
            t - t0
        );
        verifier(imprecis == 0, || {
            format!("{imprecis} réagencements non suivis")
        })?;
        verifier(n - n0 <= 4, || {
            format!("{} conversions pour un geste", n - n0)
        })?;
        verifier(t - t0 < Duration::from_millis(200), || {
            format!("conversions : {:?} (200 ms au plus)", t - t0)
        })?;
        attendu(b.naviguer(true), &format!("Commande : echo {}", total - 1))?;
        attendu(b.naviguer(true), &format!("Commande : echo {}", total - 2))?;
        verifier(b.haut_visible() == format!("$ echo {}", total - 2), || {
            format!("haut de la vue : « {} »", b.haut_visible())
        })
    })();
    b.fermer();
    res
}
