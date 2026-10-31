// SPDX-License-Identifier: GPL-3.0-or-later
//! Tests d'accessibilité (Task 12) : arbre AT-SPI conforme, tout au clavier, annonces.
//! Lancés par `scripts/test-x11.sh` et `scripts/test-wayland.sh` ; ignorés sinon.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod commun;

use commun::*;
use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use pupitre::fenetre::{FenetrePupitre, Pupitre};
use pupitre::polices::{dossier_polices, enregistrer_polices_embarquees};
use pupitre_config::Config;
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::Duration;
use vte4::prelude::*;

const NOM_APPLI: &str = "pupitre-test-a11y";
const CTRL: gdk::ModifierType = gdk::ModifierType::CONTROL_MASK;
const CTRL_MAJ: gdk::ModifierType =
    gdk::ModifierType::CONTROL_MASK.union(gdk::ModifierType::SHIFT_MASK);
/// Code matériel de la touche Tab (evdev + 8).
const CODE_TAB: u32 = 23;

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

    let dossier = tempfile::tempdir().unwrap();
    let socket = dossier.path().join("pupitre/fond.sock");
    let app = gtk::Application::builder()
        .application_id("fr.pupitre.TestA11y")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gtk::gio::Cancellable::NONE).unwrap();
    let p = Pupitre::demarrer(&app, socket.clone(), Config::default());
    let pret = attendre(Duration::from_secs(10), || {
        p.fenetres().first().is_some_and(|f| {
            f.onglets()
                .first()
                .is_some_and(|o| o.terminal().terminal().is_mapped())
        })
    });
    assert!(pret, "pas de fenêtre au démarrage");
    let f = p.fenetres()[0].clone();

    let mut ex = Executeur::new();
    ex.lancer("arbre_accessible", || arbre(&f));
    ex.lancer("tout_au_clavier", || clavier(&f));
    ex.lancer("dialogue_de_fermeture_au_clavier", || dialogue(&f));
    ex.lancer("messages_annonces", || annonces(&f));
    if x11() {
        ex.lancer("f10_reste_au_programme", || f10(&f));
    } else {
        println!("test f10_reste_au_programme ... ignoré (xdotool : X11 seulement)");
    }
    fermer_sessions(&socket);
    ex.conclure();
}

fn focus(f: &FenetrePupitre) -> Option<gtk::Widget> {
    GtkWindowExt::focus(f.fenetre())
}

fn nom_type(w: &Option<gtk::Widget>) -> String {
    w.as_ref()
        .map(|w| w.type_().name().to_string())
        .unwrap_or_else(|| "aucun".into())
}

/// Lance la sonde en mode vérification sans bloquer la boucle GTK.
fn sonde_attendu(fichier: &str) -> (bool, String) {
    let mut enfant = Command::new("python3")
        .arg(sonde_a11y())
        .args(["--app", NOM_APPLI, "--attendu"])
        .arg(racine().join(fichier))
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    attendre(Duration::from_secs(30), || {
        matches!(enfant.try_wait(), Ok(Some(_)))
    });
    let _ = enfant.kill();
    let o = enfant.wait_with_output().unwrap();
    (
        o.status.success(),
        String::from_utf8_lossy(&o.stdout).into_owned(),
    )
}

fn arbre(f: &FenetrePupitre) -> Resultat {
    // Deux onglets, un message avec bouton, la barre de recherche ouverte : tout ce qui
    // porte un nom doit en avoir un.
    f.traiter_touche(gdk::Key::T, 28, CTRL_MAJ);
    let ok = attendre(Duration::from_secs(10), || f.onglets().len() == 2);
    verifier(ok, || "second onglet absent".into())?;
    f.afficher_message("Message de test");
    f.traiter_touche(gdk::Key::F, 41, CTRL_MAJ);
    let o = f.onglet_actif().ok_or("aucun onglet actif")?;
    attendre(Duration::from_secs(10), || {
        o.terminal()
            .terminal()
            .text_format(vte4::Format::Text)
            .is_some_and(|t| t.contains('$'))
    });
    patienter(Duration::from_millis(500));
    let (ok, sortie) = sonde_attendu("tests/a11y/fenetre.json");
    f.fermer_recherche();
    verifier(ok, || sortie)
}

/// Ctrl+Tab / Ctrl+Maj+Tab sortent du terminal (qui garde Tab pour le shell) et
/// parcourent les éléments de la fenêtre ; le focus est toujours sur un élément visible.
fn clavier(f: &FenetrePupitre) -> Resultat {
    let t = f
        .onglet_actif()
        .ok_or("aucun onglet")?
        .terminal()
        .terminal()
        .clone();
    t.grab_focus();
    patienter(Duration::from_millis(100));
    verifier(focus(f).as_ref() == Some(t.upcast_ref()), || {
        format!("focus initial sur {}", nom_type(&focus(f)))
    })?;
    let mut vus: Vec<String> = Vec::new();
    let mut retour = false;
    for _ in 0..12 {
        f.traiter_touche(gdk::Key::Tab, CODE_TAB, CTRL);
        patienter(Duration::from_millis(50));
        let w = focus(f).ok_or("plus aucun focus")?;
        verifier(w.is_visible() && w.is_mapped(), || {
            format!("focus sur un élément invisible : {}", w.type_().name())
        })?;
        if &w == t.upcast_ref::<gtk::Widget>() {
            retour = true;
            break;
        }
        let chemin = format!(
            "{} (dans {})",
            w.type_().name(),
            w.parent()
                .map(|p| p.type_().name().to_string())
                .unwrap_or_default()
        );
        vus.push(chemin);
    }
    let resume = vus.join(" → ");
    verifier(retour, || {
        format!("Ctrl+Tab ne revient pas au terminal : {resume}")
    })?;
    verifier(
        vus.iter()
            .any(|v| v.contains("Notebook") || v.contains("Tab")),
        || format!("onglets non atteints : {resume}"),
    )?;
    verifier(vus.iter().any(|v| v.contains("MenuButton")), || {
        format!("menu non atteint : {resume}")
    })?;
    // Ctrl+Maj+Tab part dans l'autre sens.
    f.traiter_touche(gdk::Key::ISO_Left_Tab, CODE_TAB, CTRL_MAJ);
    patienter(Duration::from_millis(50));
    let w = focus(f);
    verifier(w.as_ref() != Some(t.upcast_ref()) && w.is_some(), || {
        "Ctrl+Maj+Tab sans effet".into()
    })?;
    // Le menu s'ouvre au clavier et se ferme par Échap.
    let menu = f.bouton_menu();
    menu.popup();
    patienter(Duration::from_millis(200));
    let pop = menu.popover().ok_or("pas de menu")?;
    verifier(pop.is_visible(), || "menu non ouvert".into())?;
    menu.popdown();
    // Recherche : le focus va dans la saisie, puis revient au terminal.
    t.grab_focus();
    f.traiter_touche(gdk::Key::F, 41, CTRL_MAJ);
    patienter(Duration::from_millis(100));
    let entree = f.entree_recherche();
    let w = focus(f);
    verifier(
        w.as_ref()
            .is_some_and(|w| w == entree.upcast_ref::<gtk::Widget>() || w.is_ancestor(&entree)),
        || format!("recherche : focus sur {}", nom_type(&w)),
    )?;
    if x11() {
        // Sous X11 la fenêtre est active : l'état AT-SPI « focused » est vérifiable.
        let (ok, arbre) = sonder_jusqu_a(NOM_APPLI, Duration::from_secs(10), |a| {
            a.lines().any(|l| {
                let c: Vec<&str> = l.split('\t').collect();
                c.len() == 4
                    && c[0] == "entry"
                    && c[3].split(',').any(|e| e == "focused")
                    && c[3].split(',').any(|e| e == "showing")
            })
        });
        verifier(ok, || {
            format!("saisie de recherche non « focused » :\n{arbre}")
        })?;
    }
    f.fermer_recherche();
    patienter(Duration::from_millis(100));
    verifier(focus(f).as_ref() == Some(t.upcast_ref()), || {
        format!("après la recherche, focus sur {}", nom_type(&focus(f)))
    })
}

fn x11() -> bool {
    std::env::var("GDK_BACKEND").is_ok_and(|b| b == "x11")
}

/// Le dialogue de fermeture s'ouvre avec le focus sur « Annuler », lisible par Orca.
fn dialogue(f: &FenetrePupitre) -> Resultat {
    let o = f.onglet_actif().ok_or("aucun onglet")?;
    o.terminal().terminal().feed_child(b"sleep 30\n");
    let ok = attendre(Duration::from_secs(5), || o.occupe());
    verifier(ok, || "onglet jamais occupé".into())?;
    f.traiter_touche(gdk::Key::W, 25, CTRL_MAJ);
    let mut d: Option<gtk::Window> = None;
    attendre(Duration::from_secs(5), || {
        d = gtk::Window::list_toplevels()
            .into_iter()
            .filter_map(|w| w.downcast::<gtk::Window>().ok())
            .find(|w| w.is_visible() && w.accessible_role() == gtk::AccessibleRole::Dialog);
        d.is_some()
    });
    let d = d.ok_or("pas de dialogue")?;
    let w = GtkWindowExt::focus(&d);
    let res = (|| {
        verifier(
            w.as_ref()
                .and_then(|w| w.clone().downcast::<gtk::Button>().ok())
                .and_then(|b| b.label())
                .is_some_and(|l| l == "Annuler"),
            || format!("focus du dialogue sur {}", nom_type(&w)),
        )?;
        // Orca lit la question : le dialogue est décrit par son texte.
        let (ok, arbre) = sonder_jusqu_a(NOM_APPLI, Duration::from_secs(10), |a| {
            a.lines().any(|l| l.starts_with("dialog\t"))
                && a.contains("Une commande est en cours dans cet onglet")
        });
        verifier(ok, || format!("dialogue ou question absents :\n{arbre}"))
    })();
    d.destroy();
    o.terminal().terminal().feed_child(&[3]); // Ctrl+C : fin du sleep
    res
}

/// Un message du bandeau est annoncé au lecteur d'écran (événement AT-SPI).
fn annonces(f: &FenetrePupitre) -> Resultat {
    let mut ecoute = Command::new("python3")
        .arg(racine().join("tests/a11y/ecoute.py"))
        .args(["--app", NOM_APPLI, "--duree", "6"])
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    // Laisser l'écoute s'abonner avant d'annoncer.
    patienter(Duration::from_secs(2));
    let message = "Message annoncé au lecteur d'écran";
    f.afficher_message(message);
    attendre(Duration::from_secs(15), || {
        matches!(ecoute.try_wait(), Ok(Some(_)))
    });
    let _ = ecoute.kill();
    let mut sortie = String::new();
    if let Some(mut s) = ecoute.stdout.take() {
        let _ = s.read_to_string(&mut sortie);
    }
    let _ = ecoute.wait();
    let attendu = format!("annonce\t{message}");
    verifier(sortie.lines().any(|l| l == attendu), || {
        format!("annonce non reçue ; écoute : {sortie:?}")
    })
}

/// F10 va au programme du terminal (mc, htop quittent par F10), pas au menu de Pupitre.
fn f10(f: &FenetrePupitre) -> Resultat {
    let o = f.onglet_actif().ok_or("aucun onglet")?;
    let t = o.terminal().terminal().clone();
    let texte = || {
        t.text_format(vte4::Format::Text)
            .map(|s| s.to_string())
            .unwrap_or_default()
    };
    f.fenetre().present();
    t.grab_focus();
    t.feed_child(b"cat -v\n");
    patienter(Duration::from_millis(500));
    let ok = Command::new("xdotool")
        .args(["key", "F10"])
        .status()
        .is_ok_and(|s| s.success());
    verifier(ok, || "xdotool".into())?;
    let recu = attendre(Duration::from_secs(5), || texte().contains("^[[21~"));
    let menu_ouvert = f.bouton_menu().popover().is_some_and(|p| p.is_visible());
    t.feed_child(&[3]); // Ctrl+C : fin de cat
    verifier(recu && !menu_ouvert, || {
        format!("F10 : reçu par le programme = {recu}, menu ouvert = {menu_ouvert}")
    })
}
