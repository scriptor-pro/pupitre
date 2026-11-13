// SPDX-License-Identifier: GPL-3.0-or-later
//! Tests graphiques des onglets, fenêtres, fermeture et reprise après plantage (Task 9).
//! Lancés par `scripts/test-x11.sh` et `scripts/test-wayland.sh` ; ignorés sinon.
//!
//! L'application est pilotée dans ce processus (actions de fenêtre, comme les raccourcis
//! les déclenchent) ; la reprise après plantage utilise le vrai binaire, tué par SIGKILL.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod commun;

use commun::*;
use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use pupitre::client::{connect_or_spawn_at, Client};
use pupitre::fenetre::{FenetrePupitre, Onglet, Pupitre};
use pupitre::polices::{dossier_polices, enregistrer_polices_embarquees};
use pupitre::raccourcis::{raccourci, Raccourci};
use pupitre_config::Config;
use pupitre_proto::{ClientMsg, ServerMsg, SessionInfo};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use std::time::{Duration, Instant};
use vte4::prelude::*;

const NOM_APPLI: &str = "pupitre-test-onglets";

fn main() {
    if !actives() {
        return;
    }
    bus_prive();
    compiler();
    let _perso = isoler_dossiers();
    assert!(enregistrer_polices_embarquees(&dossier_polices().unwrap()));
    glib::set_prgname(Some(NOM_APPLI));
    glib::set_application_name(NOM_APPLI);
    gtk::init().unwrap();

    let mut ex = Executeur::new();
    ex.lancer("raccourcis_reconnus", raccourcis_reconnus);

    let dossier = tempfile::tempdir().unwrap();
    let socket = dossier.path().join("pupitre/fond.sock");
    let app = gtk::Application::builder()
        .application_id("fr.pupitre.TestOnglets")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gtk::gio::Cancellable::NONE).unwrap();
    let pupitre = Pupitre::demarrer(&app, socket.clone(), Config::default());
    let (temoin, temoin_rx) = connect_or_spawn_at(&socket).unwrap();
    let temoin = Temoin {
        client: temoin,
        rx: temoin_rx,
    };

    ex.lancer("premiere_fenetre_ouvre_une_session", || {
        premiere_fenetre(&pupitre, &temoin)
    });
    ex.lancer("nouvel_onglet_ouvre_une_session", || {
        nouvel_onglet(&pupitre, &temoin)
    });
    ex.lancer("titre_du_programme_puis_renommage", || titre(&pupitre));
    ex.lancer("fermeture_occupee_demande_confirmation", || {
        fermeture_occupee(&pupitre, &temoin)
    });
    ex.lancer("exit_ferme_l_onglet", || exit_ferme(&pupitre));
    ex.lancer("fermeture_en_arriere_plan_annoncee", || {
        fermeture_en_arriere_plan(&pupitre)
    });
    ex.lancer("connexion_coupee_garde_l_onglet", || {
        connexion_coupee(&pupitre)
    });
    ex.lancer("echec_au_demarrage_garde_l_onglet", || {
        echec_au_demarrage(&pupitre)
    });
    ex.lancer("onglet_shell_termine_affiche_message", || {
        shell_termine(&pupitre)
    });
    ex.lancer("detacher_onglet_dans_une_nouvelle_fenetre", || {
        detacher(&pupitre)
    });
    ex.lancer("fermer_une_fenetre_ferme_ses_sessions", || {
        fermer_fenetre(&pupitre, &temoin)
    });
    ex.lancer("focus_apres_nouvelle_fenetre", || {
        focus_nouvelle_fenetre(&pupitre, &temoin)
    });
    ex.lancer("focus_apres_reprise", || {
        focus_apres_reprise(&pupitre, &socket)
    });
    ex.lancer("deux_entrees_un_seul_shell", || {
        deux_entrees(&pupitre, &temoin)
    });
    ex.lancer("connexion_perdue_affiche_un_message", || {
        connexion_perdue(&pupitre, &socket)
    });
    ex.lancer("reprise_apres_plantage", reprise_apres_plantage);
    drop(pupitre);
    fermer_sessions(&socket);
    ex.conclure();
}

/// Second client sur le même processus de fond : il observe les sessions.
struct Temoin {
    client: Client,
    rx: async_channel::Receiver<ServerMsg>,
}

impl Temoin {
    fn sessions(&self) -> Vec<SessionInfo> {
        while self.rx.try_recv().is_ok() {}
        self.client.send(ClientMsg::List).unwrap();
        let limite = Instant::now() + Duration::from_secs(5);
        while Instant::now() < limite {
            match self.rx.try_recv() {
                Ok(ServerMsg::Sessions(v)) => return v,
                Ok(_) => {}
                Err(_) => patienter(Duration::from_millis(20)),
            }
        }
        panic!("pas de réponse à List");
    }
}

fn texte(o: &Onglet) -> String {
    o.terminal()
        .terminal()
        .text_format(vte4::Format::Text)
        .map(|s| s.to_string())
        .unwrap_or_default()
}

/// Tape `commande` dans l'onglet (comme au clavier : signal `commit`) et attend `motif`.
fn executer(o: &Onglet, commande: &str, motif: &str) -> bool {
    o.terminal()
        .terminal()
        .feed_child(format!("{commande}\n").as_bytes());
    attendre(Duration::from_secs(10), || texte(o).contains(motif))
}

/// Fenêtres visibles ; vérifie au passage qu'aucune page n'est orpheline.
fn fenetres_visibles(p: &Pupitre) -> Vec<FenetrePupitre> {
    let v: Vec<FenetrePupitre> = p
        .fenetres()
        .into_iter()
        .filter(|f| f.fenetre().is_visible())
        .collect();
    for f in &v {
        assert_eq!(
            f.nombre_pages(),
            f.onglets().len(),
            "page affichée sans onglet connu (ou l'inverse)"
        );
    }
    v
}

fn raccourcis_reconnus() -> Resultat {
    let cs = gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::SHIFT_MASK;
    let cas = [
        (gdk::Key::T, cs, Some(Raccourci::NouvelOnglet)),
        (gdk::Key::t, cs, Some(Raccourci::NouvelOnglet)),
        (gdk::Key::W, cs, Some(Raccourci::FermerOnglet)),
        (gdk::Key::N, cs, Some(Raccourci::NouvelleFenetre)),
        (gdk::Key::t, gdk::ModifierType::CONTROL_MASK, None),
        (gdk::Key::T, gdk::ModifierType::SHIFT_MASK, None),
        (gdk::Key::W, cs | gdk::ModifierType::ALT_MASK, None),
    ];
    for (k, m, attendu) in cas {
        let obtenu = raccourci(k, 0, m);
        verifier(obtenu == attendu, || {
            format!("{k:?} {m:?} → {obtenu:?}, attendu {attendu:?}")
        })?;
    }
    Ok(())
}

fn premiere_fenetre(p: &Rc<Pupitre>, temoin: &Temoin) -> Resultat {
    let ok = attendre(Duration::from_secs(10), || {
        fenetres_visibles(p)
            .first()
            .is_some_and(|f| f.onglets().len() == 1)
    });
    verifier(ok, || "pas de fenêtre avec un onglet au démarrage".into())?;
    let f = &fenetres_visibles(p)[0];
    let o = f.onglets()[0].clone();
    verifier(executer(&o, "echo un-$((1+1))", "un-2"), || {
        format!("le shell ne répond pas : {:?}", texte(&o))
    })?;
    let n = temoin.sessions().len();
    verifier(n == 1, || format!("{n} sessions côté fond"))
}

fn nouvel_onglet(p: &Rc<Pupitre>, temoin: &Temoin) -> Resultat {
    let f = fenetres_visibles(p)[0].clone();
    WidgetExt::activate_action(f.fenetre(), "win.nouvel-onglet", None)
        .map_err(|e| e.to_string())?;
    let ok = attendre(Duration::from_secs(10), || f.onglets().len() == 2);
    verifier(ok, || format!("{} onglet(s)", f.onglets().len()))?;
    let actif = f.onglet_actif().ok_or("aucun onglet actif")?;
    verifier(Rc::ptr_eq(&actif, &f.onglets()[1]), || {
        "le nouvel onglet n'est pas actif".into()
    })?;
    // Le terminal du nouvel onglet a le focus (c'est lui que le clavier et Orca suivent).
    verifier(a_le_focus(&f, &actif), || {
        "le terminal du nouvel onglet n'a pas le focus de la fenêtre".into()
    })?;
    verifier(executer(&actif, "echo deux-$((1+1))", "deux-2"), || {
        "le nouvel onglet ne répond pas".into()
    })?;
    let n = temoin.sessions().len();
    verifier(n == 2, || format!("{n} sessions côté fond"))
}

/// Vrai si le widget qui a le focus dans la fenêtre `f` est le terminal de l'onglet `o`.
fn a_le_focus(f: &FenetrePupitre, o: &Rc<Onglet>) -> bool {
    let terminal: gtk::Widget = o.terminal().terminal().clone().upcast();
    GtkWindowExt::focus(f.fenetre()).is_some_and(|w| w == terminal)
}

/// Nouvelle fenêtre (Ctrl+Maj+N) : son premier onglet a le focus.
fn focus_nouvelle_fenetre(p: &Rc<Pupitre>, temoin: &Temoin) -> Resultat {
    let avant = temoin.sessions().len();
    let n = fenetres_visibles(p).len();
    let f = p.nouvelle_fenetre();
    let ok = attendre(Duration::from_secs(10), || {
        fenetres_visibles(p).len() == n + 1 && f.onglets().len() == 1
    });
    verifier(ok, || "la nouvelle fenêtre n'a pas d'onglet".into())?;
    let o = f.onglet_actif().ok_or("aucun onglet actif")?;
    let ok = attendre(Duration::from_secs(5), || a_le_focus(&f, &o));
    verifier(ok, || {
        "le terminal du premier onglet de la nouvelle fenêtre n'a pas le focus".into()
    })?;
    f.fenetre().close();
    let ok = attendre(Duration::from_secs(5), || fenetres_visibles(p).len() == n);
    verifier(ok, || "la nouvelle fenêtre ne s'est pas fermée".into())?;
    let ok = attendre(Duration::from_secs(5), || temoin.sessions().len() == avant);
    verifier(ok, || {
        "la session de la fenêtre fermée existe encore".into()
    })
}

/// Reprise : une seconde interface démarre sur le même processus de fond et rattache les
/// sessions existantes ; dans sa fenêtre, le terminal de l'onglet courant a le focus.
fn focus_apres_reprise(_p: &Rc<Pupitre>, socket: &Path) -> Resultat {
    let app = gtk::Application::builder()
        .application_id("fr.pupitre.TestOngletsReprise")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gtk::gio::Cancellable::NONE)
        .map_err(|e| e.to_string())?;
    let reprise = Pupitre::demarrer(&app, socket.to_path_buf(), Config::default());
    let ok = attendre(Duration::from_secs(15), || {
        fenetres_visibles(&reprise)
            .first()
            .is_some_and(|f| !f.onglets().is_empty())
    });
    verifier(ok, || "aucun onglet restauré par la reprise".into())?;
    let f = fenetres_visibles(&reprise)[0].clone();
    let o = f
        .onglet_actif()
        .ok_or("aucun onglet actif après la reprise")?;
    let ok = attendre(Duration::from_secs(5), || a_le_focus(&f, &o));
    let res = verifier(ok, || {
        "après la reprise, le terminal de l'onglet courant n'a pas le focus".into()
    });
    // Ménage : fermer les fenêtres de la reprise sans fermer les sessions des autres.
    for g in reprise.fenetres() {
        g.fenetre().destroy();
    }
    drop(reprise);
    res
}

fn titre(p: &Rc<Pupitre>) -> Resultat {
    let f = fenetres_visibles(p)[0].clone();
    let o = f.onglet_actif().ok_or("aucun onglet actif")?;
    // Le nouveau nom de l'onglet doit être signalé aux lecteurs d'écran : sinon Orca garde
    // l'ancien, le croit différent de celui du terminal et quitte le terminal.
    let mut ecoute = Command::new("python3")
        .arg(racine().join("tests/a11y/ecoute.py"))
        .args(["--app", NOM_APPLI, "--duree", "6"])
        .args(["--evenement", "object:property-change:accessible-name"])
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    patienter(Duration::from_secs(2));
    executer(&o, "printf '\\033]0;mon-titre\\007'; echo fait", "fait");
    let ok = attendre(Duration::from_secs(5), || o.titre() == "mon-titre");
    attendre(Duration::from_secs(15), || {
        matches!(ecoute.try_wait(), Ok(Some(_)))
    });
    let _ = ecoute.kill();
    let mut sortie = String::new();
    if let Some(mut s) = ecoute.stdout.take() {
        let _ = std::io::Read::read_to_string(&mut s, &mut sortie);
    }
    let _ = ecoute.wait();
    verifier(ok, || format!("titre « {} »", o.titre()))?;
    let lignes: Vec<&str> = sortie.lines().collect();
    let signale = lignes
        .windows(2)
        .any(|l| l[0] == "annonce\tmon-titre" && l[1] == "source\tpage tab");
    verifier(signale, || {
        format!("nouveau nom de l'onglet non signalé ; écoute : {sortie:?}")
    })?;
    let titre_fenetre = f
        .fenetre()
        .title()
        .map(|t| t.to_string())
        .unwrap_or_default();
    verifier(titre_fenetre.contains("mon-titre"), || {
        format!("titre de fenêtre « {titre_fenetre} »")
    })?;
    // Nom accessible de l'onglet = titre (lu par la sonde AT-SPI).
    let (ok, arbre) = sonder_jusqu_a(NOM_APPLI, Duration::from_secs(10), |a| {
        a.lines().any(|l| l.starts_with("page tab\tmon-titre"))
    });
    verifier(ok, || {
        format!("onglet « mon-titre » absent de l'arbre AT-SPI :\n{arbre}")
    })?;
    o.renommer(Some("perso"));
    executer(&o, "printf '\\033]0;autre\\007'; echo refait", "refait");
    patienter(Duration::from_millis(200));
    verifier(o.titre() == "perso", || {
        format!("le nom choisi doit l'emporter : « {} »", o.titre())
    })?;
    o.renommer(None);
    verifier(o.titre() == "autre", || {
        format!("après effacement : « {} »", o.titre())
    })
}

/// Fenêtre de dialogue visible (rôle accessible « dialog »), s'il y en a une.
fn dialogue() -> Option<gtk::Window> {
    gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Window>().ok())
        .find(|w| w.is_visible() && w.accessible_role() == gtk::AccessibleRole::Dialog)
}

fn bouton(racine: &gtk::Widget, etiquette: &str) -> Option<gtk::Button> {
    let mut pile = vec![racine.clone()];
    while let Some(w) = pile.pop() {
        if let Ok(b) = w.clone().downcast::<gtk::Button>() {
            if b.label().is_some_and(|l| l == etiquette) {
                return Some(b);
            }
        }
        let mut enfant = w.first_child();
        while let Some(e) = enfant {
            enfant = e.next_sibling();
            pile.push(e);
        }
    }
    None
}

fn fermeture_occupee(p: &Rc<Pupitre>, temoin: &Temoin) -> Resultat {
    let f = fenetres_visibles(p)[0].clone();
    let o = f.onglet_actif().ok_or("aucun onglet actif")?;
    o.terminal().terminal().feed_child(b"sleep 30\n");
    let ok = attendre(Duration::from_secs(5), || o.occupe());
    verifier(ok, || "l'onglet n'est jamais signalé occupé".into())?;
    WidgetExt::activate_action(f.fenetre(), "win.fermer-onglet", None)
        .map_err(|e| e.to_string())?;
    let mut d = None;
    attendre(Duration::from_secs(5), || {
        d = dialogue();
        d.is_some()
    });
    let d = d.ok_or("aucun dialogue de confirmation")?;
    let (ok, arbre) = sonder_jusqu_a(NOM_APPLI, Duration::from_secs(10), |a| {
        a.lines().any(|l| l.starts_with("dialog\t"))
    });
    verifier(ok, || {
        format!("rôle « dialog » absent de l'arbre AT-SPI :\n{arbre}")
    })?;
    // Annuler est le bouton par défaut.
    let annuler = bouton(d.upcast_ref(), "Annuler").ok_or("pas de bouton Annuler")?;
    verifier(
        d.default_widget().as_ref() == Some(annuler.upcast_ref()),
        || "Annuler n'est pas le bouton par défaut".into(),
    )?;
    annuler.emit_clicked();
    patienter(Duration::from_millis(300));
    verifier(f.onglets().len() == 2 && dialogue().is_none(), || {
        format!("après Annuler : {} onglets", f.onglets().len())
    })?;
    // Confirmer : l'onglet et sa session disparaissent.
    WidgetExt::activate_action(f.fenetre(), "win.fermer-onglet", None)
        .map_err(|e| e.to_string())?;
    let mut d = None;
    attendre(Duration::from_secs(5), || {
        d = dialogue();
        d.is_some()
    });
    let d = d.ok_or("pas de second dialogue")?;
    bouton(d.upcast_ref(), "Fermer")
        .ok_or("pas de bouton Fermer")?
        .emit_clicked();
    let ok = attendre(Duration::from_secs(5), || f.onglets().len() == 1);
    verifier(ok, || {
        format!("après Fermer : {} onglets", f.onglets().len())
    })?;
    let ok = attendre(Duration::from_secs(5), || temoin.sessions().len() == 1);
    verifier(ok, || "la session fermée existe encore".into())
}

/// Fin voulue du shell (`exit`, Ctrl+D) : l'onglet se ferme, les autres restent. Après une
/// commande en échec, `exit` rend le code de cette commande (1) : l'onglet se ferme aussi.
fn exit_ferme(p: &Rc<Pupitre>) -> Resultat {
    let f = fenetres_visibles(p)[0].clone();
    let avant = f.onglets().len();
    WidgetExt::activate_action(f.fenetre(), "win.nouvel-onglet", None)
        .map_err(|e| e.to_string())?;
    let ok = attendre(Duration::from_secs(10), || f.onglets().len() == avant + 1);
    verifier(ok, || "pas de nouvel onglet".into())?;
    let o = f.onglet_actif().ok_or("aucun onglet actif")?;
    verifier(executer(&o, "echo pret-$((1+1))", "pret-2"), || {
        "le shell ne répond pas".into()
    })?;
    o.terminal().terminal().feed_child(b"false\n");
    o.terminal().terminal().feed_child(b"exit\n");
    let ok = attendre(Duration::from_secs(10), || f.onglets().len() == avant);
    verifier(ok, || format!("{} onglet(s) après exit", f.onglets().len()))?;
    verifier(!f.onglets().iter().any(|x| Rc::ptr_eq(x, &o)), || {
        "l'onglet fermé est encore là".into()
    })
}

/// Un onglet non affiché dont le shell se termine : il se ferme, et c'est annoncé (le nombre
/// d'onglets ne change pas en silence).
fn fermeture_en_arriere_plan(p: &Rc<Pupitre>) -> Resultat {
    let f = fenetres_visibles(p)[0].clone();
    let avant = f.onglets().len();
    WidgetExt::activate_action(f.fenetre(), "win.nouvel-onglet", None)
        .map_err(|e| e.to_string())?;
    let ok = attendre(Duration::from_secs(10), || f.onglets().len() == avant + 1);
    verifier(ok, || "pas de nouvel onglet".into())?;
    let o = f.onglet_actif().ok_or("aucun onglet actif")?;
    verifier(executer(&o, "echo pret-$((2+1))", "pret-3"), || {
        "le shell ne répond pas".into()
    })?;
    o.terminal().terminal().feed_child(b"sleep 2; exit\n");
    f.executer(Raccourci::Onglet(1));
    let ok = attendre(Duration::from_secs(15), || f.onglets().len() == avant);
    verifier(ok, || "l'onglet d'arrière-plan ne s'est pas fermé".into())?;
    let ok = attendre(Duration::from_secs(2), || {
        f.messages()
            .iter()
            .any(|m| m.contains("fermé : son shell s'est terminé."))
    });
    verifier(ok, || {
        format!("fermeture non annoncée : {:?}", f.messages())
    })
}

/// Programme qui échoue avant toute invite (comme un `.bashrc` en erreur) : l'onglet reste,
/// avec le code, pour que l'utilisateur voie ce qui s'est passé.
fn echec_au_demarrage(p: &Rc<Pupitre>) -> Resultat {
    let f = fenetres_visibles(p)[0].clone();
    let avant = f.onglets().len();
    f.ouvrir_connexion(
        vec![
            "/bin/sh".into(),
            "-c".into(),
            "echo demarrage-rate; exit 3".into(),
        ],
        "essai".into(),
    );
    let ok = attendre(Duration::from_secs(10), || f.onglets().len() == avant + 1);
    verifier(ok, || "pas de nouvel onglet".into())?;
    let o = f.onglet_actif().ok_or("aucun onglet actif")?;
    let ok = attendre(Duration::from_secs(10), || o.termine());
    verifier(ok, || "fin du programme non signalée".into())?;
    patienter(Duration::from_millis(300));
    verifier(f.onglets().len() == avant + 1, || {
        "l'onglet d'un programme en échec au démarrage a été fermé".into()
    })?;
    // Pas un shell (connexion, éditeur) : le message parle du programme.
    verifier(
        texte(&o).contains(
            "Le programme s'est terminé (code 3). Appuyez sur Entrée pour ouvrir un shell.",
        ),
        || format!("message absent : {:?}", texte(&o)),
    )?;
    f.executer(Raccourci::FermerOnglet);
    let ok = attendre(Duration::from_secs(10), || f.onglets().len() == avant);
    verifier(ok, || "onglet de l'essai non refermé".into())
}

/// Connexion SSH coupée (ssh rend 255) vers un hôte qui a affiché son invite (marque A) :
/// l'onglet reste, avec le message d'erreur et l'historique.
fn connexion_coupee(p: &Rc<Pupitre>) -> Resultat {
    let f = fenetres_visibles(p)[0].clone();
    let avant = f.onglets().len();
    f.ouvrir_connexion(
        vec![
            "/bin/sh".into(),
            "-c".into(),
            "printf '\\033]133;A\\007distant$ '; sleep 1; echo coupure; exit 255".into(),
        ],
        "serveur".into(),
    );
    let ok = attendre(Duration::from_secs(10), || f.onglets().len() == avant + 1);
    verifier(ok, || "pas de nouvel onglet".into())?;
    let o = f.onglet_actif().ok_or("aucun onglet actif")?;
    let ok = attendre(Duration::from_secs(10), || o.termine());
    patienter(Duration::from_millis(300));
    verifier(ok && f.onglets().len() == avant + 1, || {
        format!(
            "onglet de connexion coupée fermé ({} onglet(s))",
            f.onglets().len()
        )
    })?;
    verifier(
        texte(&o).contains("coupure") && texte(&o).contains("(code 255)"),
        || format!("historique ou code absent : {:?}", texte(&o)),
    )?;
    f.executer(Raccourci::FermerOnglet);
    let ok = attendre(Duration::from_secs(10), || f.onglets().len() == avant);
    verifier(ok, || "onglet de l'essai non refermé".into())
}

/// Shell tué (signal) : l'onglet reste, avec le message et le moyen d'en rouvrir un.
fn shell_termine(p: &Rc<Pupitre>) -> Resultat {
    let f = fenetres_visibles(p)[0].clone();
    let o = f.onglet_actif().ok_or("aucun onglet actif")?;
    let ancien = o.terminal().id();
    o.terminal().terminal().feed_child(b"kill -9 $$\n");
    let ok = attendre(Duration::from_secs(10), || o.termine());
    verifier(ok, || "fin du shell non signalée".into())?;
    let (ok, arbre) = sonder_jusqu_a(NOM_APPLI, Duration::from_secs(10), |a| {
        a.contains(
            "Le shell s'est terminé (code signal). Appuyez sur Entrée pour en ouvrir un nouveau.",
        )
    });
    verifier(ok, || {
        format!("message absent de l'arbre AT-SPI :\n{arbre}")
    })?;
    verifier(f.onglets().len() == 1, || "l'onglet a été fermé".into())?;
    // Entrée : nouveau shell dans le même onglet.
    p.rouvrir(&o);
    let ok = attendre(Duration::from_secs(10), || {
        !o.termine() && o.terminal().id() != ancien
    });
    verifier(ok, || "pas de nouveau shell".into())?;
    verifier(executer(&o, "echo rouvert-$((2+2))", "rouvert-4"), || {
        "le nouveau shell ne répond pas".into()
    })
}

fn detacher(p: &Rc<Pupitre>) -> Resultat {
    let f = fenetres_visibles(p)[0].clone();
    WidgetExt::activate_action(f.fenetre(), "win.nouvel-onglet", None)
        .map_err(|e| e.to_string())?;
    attendre(Duration::from_secs(10), || f.onglets().len() == 2);
    let o = f.onglet_actif().ok_or("aucun onglet actif")?;
    f.detacher_onglet(o.terminal().id());
    let ok = attendre(Duration::from_secs(5), || fenetres_visibles(p).len() == 2);
    verifier(ok, || format!("{} fenêtre(s)", fenetres_visibles(p).len()))?;
    let toutes = fenetres_visibles(p);
    let nouvelle = toutes
        .iter()
        .find(|g| g.onglets().iter().any(|x| Rc::ptr_eq(x, &o)))
        .ok_or("onglet détaché introuvable")?;
    verifier(
        nouvelle.onglets().len() == 1 && f.onglets().len() == 1,
        || "répartition des onglets incorrecte".into(),
    )?;
    let ok = attendre(Duration::from_secs(5), || {
        o.terminal().terminal().is_mapped()
    });
    verifier(ok, || "onglet détaché non affiché".into())?;
    verifier(executer(&o, "echo detache-$((3+3))", "detache-6"), || {
        "la session de l'onglet détaché ne répond plus".into()
    })
}

fn fermer_fenetre(p: &Rc<Pupitre>, temoin: &Temoin) -> Resultat {
    let avant = temoin.sessions().len();
    let toutes = fenetres_visibles(p);
    let f = toutes.last().ok_or("aucune fenêtre")?.clone();
    f.fenetre().close();
    let ok = attendre(Duration::from_secs(5), || fenetres_visibles(p).len() == 1);
    verifier(ok, || "la fenêtre ne s'est pas fermée".into())?;
    verifier(p.fenetres().len() == 1, || {
        format!("{} fenêtres encore listées", p.fenetres().len())
    })?;
    let ok = attendre(Duration::from_secs(5), || {
        temoin.sessions().len() == avant - 1
    });
    verifier(ok, || {
        "la session de la fenêtre fermée existe encore".into()
    })
}

fn connexion_perdue(p: &Rc<Pupitre>, socket: &Path) -> Resultat {
    let f = fenetres_visibles(p)[0].clone();
    // Tue le processus de fond de ce test (repéré par son socket).
    let tue = Command::new("pkill")
        .arg("-KILL")
        .arg("-f")
        .arg(socket.to_string_lossy().as_ref())
        .status()
        .map_err(|e| e.to_string())?;
    verifier(tue.success(), || "processus de fond introuvable".into())?;
    let attendu = "Le processus de fond s'est arrêté : les shells ouverts sont perdus.";
    let ok = attendre(Duration::from_secs(5), || {
        f.message().is_some_and(|m| m == attendu)
    });
    verifier(ok, || format!("message : {:?}", f.message()))?;
    let (ok, arbre) = sonder_jusqu_a(NOM_APPLI, Duration::from_secs(10), |a| a.contains(attendu));
    verifier(ok, || {
        format!("message absent de l'arbre AT-SPI :\n{arbre}")
    })?;
    // Demandes faites pendant la panne : rien ne doit rester en attente pour la suite.
    f.ouvrir_connexion(
        vec!["/bin/sh".into(), "-c".into(), "sleep 20".into()],
        "orpheline".into(),
    );
    WidgetExt::activate_action(f.fenetre(), "win.nouvel-onglet", None)
        .map_err(|e| e.to_string())?;
    f.activer_message();
    let ok = attendre(Duration::from_secs(10), || {
        f.message().is_none() && f.onglets().len() == 1 && f.onglets().iter().all(|o| !o.termine())
    });
    verifier(ok, || {
        format!(
            "après « Ouvrir un nouveau terminal » : message {:?}, {} onglet(s)",
            f.message(),
            f.onglets().len()
        )
    })?;
    let o = f.onglets()[0].clone();
    verifier(o.titre() != "orpheline", || {
        "le nouveau terminal a reçu une demande faite pendant la panne".into()
    })?;
    verifier(executer(&o, "echo neuf-$((4+4))", "neuf-8"), || {
        "le nouveau terminal ne répond pas".into()
    })?;
    WidgetExt::activate_action(f.fenetre(), "win.nouvel-onglet", None)
        .map_err(|e| e.to_string())?;
    let ok = attendre(Duration::from_secs(10), || f.onglets().len() == 2);
    patienter(Duration::from_millis(500));
    verifier(ok && f.onglets().len() == 2, || {
        format!("{} onglet(s) après Ctrl+Maj+T", f.onglets().len())
    })?;
    let n = f.onglet_actif().ok_or("aucun onglet actif")?;
    verifier(n.titre() != "orpheline", || {
        "le nouvel onglet a reçu une demande faite pendant la panne".into()
    })
}

fn lancer_pupitre(runtime: &Path) -> Child {
    let mut cmd = Command::new(racine().join("target/debug/pupitre"));
    cmd.env("XDG_RUNTIME_DIR", runtime).stdout(Stdio::null());
    // Le socket Wayland est relatif à l'ancien XDG_RUNTIME_DIR : le rendre absolu.
    if let (Some(rt), Some(wd)) = (
        std::env::var_os("XDG_RUNTIME_DIR"),
        std::env::var_os("WAYLAND_DISPLAY"),
    ) {
        let wd = PathBuf::from(wd);
        if wd.is_relative() {
            cmd.env("WAYLAND_DISPLAY", PathBuf::from(rt).join(wd));
        }
    }
    cmd.spawn().unwrap()
}

/// Bout en bout : « echo avant-plantage » ; SIGKILL de l'interface ; relance → un onglet
/// dont le texte contient « avant-plantage » ; la session répond encore.
fn reprise_apres_plantage() -> Resultat {
    let runtime = tempfile::tempdir().unwrap();
    let socket = runtime.path().join("pupitre/fond.sock");
    let mut appli = lancer_pupitre(runtime.path());
    let res = (|| {
        // L'interface a ouvert une session ; un client témoin y tape la commande.
        let limite = Instant::now() + Duration::from_secs(15);
        let (temoin, rx) = loop {
            if socket.exists() {
                if let Ok(c) = connect_or_spawn_at(&socket) {
                    break c;
                }
            }
            if Instant::now() > limite {
                return Err("le processus de fond n'a pas démarré".to_string());
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        let temoin = Temoin { client: temoin, rx };
        let mut id = None;
        let limite = Instant::now() + Duration::from_secs(10);
        while id.is_none() && Instant::now() < limite {
            id = temoin.sessions().first().map(|s| s.id);
        }
        let id = id.ok_or("aucune session ouverte par l'interface")?;
        // Attendre que l'interface ait affiché l'invite avant d'écrire.
        let (ok, _) = sonder_jusqu_a("pupitre", Duration::from_secs(15), |a| {
            a.lines()
                .any(|l| l.starts_with("terminal\t") && (l.contains('$') || l.contains('λ')))
        });
        verifier(ok, || "invite jamais affichée".into())?;
        temoin
            .client
            .send(ClientMsg::Input {
                id,
                data: b"echo avant-$((40+2))-plantage\n".to_vec(),
            })
            .unwrap();
        let (ok, arbre) = sonder_jusqu_a("pupitre", Duration::from_secs(10), |a| {
            a.contains("avant-42-plantage")
        });
        verifier(ok, || {
            format!("sortie absente avant le plantage :\n{arbre}")
        })?;

        // Plantage brutal de l'interface, puis relance.
        let _ = appli.kill();
        let _ = appli.wait();
        verifier(temoin.sessions().iter().any(|s| s.id == id), || {
            "la session n'a pas survécu à l'interface".into()
        })?;
        appli = lancer_pupitre(runtime.path());
        let (ok, arbre) = sonder_jusqu_a("pupitre", Duration::from_secs(15), |a| {
            a.lines()
                .filter(|l| l.starts_with("terminal\t"))
                .any(|l| l.contains("avant-42-plantage"))
        });
        verifier(ok, || {
            format!("relecture absente après la relance :\n{arbre}")
        })?;
        let n = arbre
            .lines()
            .filter(|l| l.starts_with("terminal\t"))
            .count();
        verifier(n == 1, || {
            format!("{n} terminaux après la relance (1 attendu)")
        })?;
        verifier(temoin.sessions().len() == 1, || {
            "la relance a ouvert une session de trop".into()
        })?;
        temoin
            .client
            .send(ClientMsg::Input {
                id,
                data: b"echo apres-$((50+5))\n".to_vec(),
            })
            .unwrap();
        let (ok, arbre) = sonder_jusqu_a("pupitre", Duration::from_secs(10), |a| {
            a.contains("apres-55")
        });
        verifier(ok, || {
            format!("la session ne répond plus après la reprise :\n{arbre}")
        })?;
        // Ménage : fermer les sessions (le processus de fond s'arrête ensuite).
        for s in temoin.sessions() {
            let _ = temoin.client.send(ClientMsg::Close { id: s.id });
        }
        Ok(())
    })();
    let _ = appli.kill();
    let _ = appli.wait();
    fermer_sessions(&socket);
    res
}

/// Deux Entrée rapides sur un onglet terminé : un seul nouveau shell dans le fond.
fn deux_entrees(p: &Rc<Pupitre>, temoin: &Temoin) -> Resultat {
    let f = fenetres_visibles(p)[0].clone();
    let o = f.onglet_actif().ok_or("aucun onglet actif")?;
    let ancien = o.terminal().id();
    o.terminal().terminal().feed_child(b"kill -9 $$\n");
    let ok = attendre(Duration::from_secs(10), || o.termine());
    verifier(ok, || "fin du shell non signalée".into())?;
    let avant: Vec<_> = temoin.sessions().iter().map(|s| s.id).collect();
    p.rouvrir(&o);
    p.rouvrir(&o);
    let ok = attendre(Duration::from_secs(10), || {
        !o.termine() && o.terminal().id() != ancien
    });
    verifier(ok, || "pas de nouveau shell".into())?;
    // Laisse le temps à un éventuel second shell d'arriver.
    attendre(Duration::from_millis(800), || false);
    let nouvelles = temoin
        .sessions()
        .iter()
        .filter(|s| !avant.contains(&s.id))
        .count();
    verifier(nouvelles == 1, || {
        format!("{nouvelles} sessions nouvelles dans le fond (une attendue)")
    })
}
