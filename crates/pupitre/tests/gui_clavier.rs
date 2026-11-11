// SPDX-License-Identifier: GPL-3.0-or-later
//! Tests graphiques du clavier, du zoom, de la recherche, de la copie, des liens et de la
//! cloche (Task 11). Lancés par `scripts/test-x11.sh` et `scripts/test-wayland.sh`.
//!
//! Les raccourcis passent par `traiter_touche` (même chemin que les vraies frappes) ; sous
//! X11, de vraies frappes sont aussi envoyées par xdotool (disposition QWERTY : Xvfb
//! ignore ici tout changement de disposition ; la fr dvorak est couverte par les tests
//! unitaires de `raccourcis`, avec les symboles du fichier de disposition du système).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod commun;

use commun::*;
use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use pupitre::fenetre::{FenetrePupitre, Onglet, Pupitre};
use pupitre::polices::{dossier_polices, enregistrer_polices_embarquees};
use pupitre_config::Config;
use std::cell::RefCell;
use std::process::Command;
use std::rc::Rc;
use std::time::Duration;
use vte4::prelude::*;

const NOM_APPLI: &str = "pupitre-test-clavier";
const CTRL: gdk::ModifierType = gdk::ModifierType::CONTROL_MASK;
const CTRL_MAJ: gdk::ModifierType =
    gdk::ModifierType::CONTROL_MASK.union(gdk::ModifierType::SHIFT_MASK);

fn main() {
    if !actives() {
        return;
    }
    bus_prive();
    compiler();
    let _perso = isoler_dossiers();
    let x11 = std::env::var("GDK_BACKEND").is_ok_and(|b| b == "x11");
    glib::set_prgname(Some(NOM_APPLI));
    glib::set_application_name(NOM_APPLI);
    assert!(enregistrer_polices_embarquees(&dossier_polices().unwrap()));
    gtk::init().unwrap();

    let dossier = tempfile::tempdir().unwrap();
    let app = gtk::Application::builder()
        .application_id("fr.pupitre.TestClavier")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gtk::gio::Cancellable::NONE).unwrap();
    let p = Pupitre::demarrer(
        &app,
        dossier.path().join("pupitre/fond.sock"),
        Config::default(),
    );
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
    ex.lancer("zoom_jusqu_a_200", || zoom(&f));
    ex.lancer("alt_chiffre_change_d_onglet", || alt_chiffre(&f));
    if x11 {
        ex.lancer("vraies_frappes", || vraies_frappes(&f));
    } else {
        println!("test vraies_frappes ... ignoré (xdotool : X11 seulement)");
    }
    ex.lancer("onglet_suivant_et_precedent", || suivant_precedent(&f));
    ex.lancer("recherche_trouve_le_texte", || recherche(&f));
    ex.lancer("copier_met_la_selection_dans_le_presse_papiers", || {
        copier(&f)
    });
    ex.lancer("recherche_laisse_le_presse_papiers", || {
        recherche_sans_copie(&f)
    });
    if x11 {
        ex.lancer("selection_a_la_souris_copiee", || selection_souris(&f));
    } else {
        println!("test selection_a_la_souris_copiee ... ignoré (xdotool : X11 seulement)");
    }
    ex.lancer("coller_envoie_au_shell", || coller(&f));
    ex.lancer("cloches_rapprochees_limitees", || cloches(&f));
    ex.lancer("lien_non_ouvert_sans_ctrl", || lien(&f));
    ex.lancer("animations_reduites", || animations_reduites(&f));
    ex.lancer("plein_ecran", || plein_ecran(&f));
    fermer_sessions(&dossier.path().join("pupitre/fond.sock"));
    ex.conclure();
}

fn actif(f: &FenetrePupitre) -> Rc<Onglet> {
    f.onglet_actif().unwrap()
}

fn texte(o: &Onglet) -> String {
    o.terminal()
        .terminal()
        .text_format(vte4::Format::Text)
        .map(|s| s.to_string())
        .unwrap_or_default()
}

fn executer(o: &Onglet, commande: &str, motif: &str) -> bool {
    o.terminal()
        .terminal()
        .feed_child(format!("{commande}\n").as_bytes());
    attendre(Duration::from_secs(10), || texte(o).contains(motif))
}

fn touche(f: &FenetrePupitre, t: gdk::Key, code: u32, m: gdk::ModifierType) -> bool {
    let r = f.traiter_touche(t, code, m);
    patienter(Duration::from_millis(50));
    r
}

fn zoom(f: &FenetrePupitre) -> Resultat {
    let t = actif(f).terminal().terminal().clone();
    for _ in 0..20 {
        touche(f, gdk::Key::plus, 21, CTRL);
    }
    verifier((t.font_scale() - 2.0).abs() < 1e-9, || {
        format!("après 20 × Ctrl+plus : {}", t.font_scale())
    })?;
    touche(f, gdk::Key::_0, 19, CTRL);
    verifier((t.font_scale() - 1.0).abs() < 1e-9, || {
        format!("après Ctrl+0 : {}", t.font_scale())
    })?;
    for _ in 0..20 {
        touche(f, gdk::Key::minus, 20, CTRL);
    }
    verifier((t.font_scale() - 0.5).abs() < 1e-9, || {
        format!("après 20 × Ctrl+moins : {}", t.font_scale())
    })?;
    touche(f, gdk::Key::_0, 19, CTRL);
    Ok(())
}

fn ouvrir_onglets(f: &FenetrePupitre, n: usize) -> Resultat {
    while f.onglets().len() < n {
        let avant = f.onglets().len();
        touche(f, gdk::Key::T, 28, CTRL_MAJ);
        let ok = attendre(Duration::from_secs(10), || f.onglets().len() == avant + 1);
        verifier(ok, || "Ctrl+Maj+T n'a pas ouvert d'onglet".into())?;
    }
    Ok(())
}

fn indice_actif(f: &FenetrePupitre) -> usize {
    let a = actif(f);
    f.onglets().iter().position(|o| Rc::ptr_eq(o, &a)).unwrap()
}

fn alt_chiffre(f: &FenetrePupitre) -> Resultat {
    ouvrir_onglets(f, 3)?;
    // fr dvorak : la touche « 2 » donne « / » sans Maj.
    touche(f, gdk::Key::slash, 11, gdk::ModifierType::ALT_MASK);
    verifier(indice_actif(f) == 1, || {
        format!("onglet {}", indice_actif(f) + 1)
    })?;
    touche(f, gdk::Key::_1, 10, gdk::ModifierType::ALT_MASK);
    verifier(indice_actif(f) == 0, || {
        format!("onglet {}", indice_actif(f) + 1)
    })?;
    // Alt+9 au-delà du nombre d'onglets : rien ne change, rien ne casse.
    touche(f, gdk::Key::parenright, 18, gdk::ModifierType::ALT_MASK);
    verifier(indice_actif(f) == 0, || "Alt+9 a changé d'onglet".into())
}

/// Vraies frappes (XTEST) : chemin complet contrôleur de la fenêtre → raccourci.
fn vraies_frappes(f: &FenetrePupitre) -> Resultat {
    let xdotool = |args: &[&str]| {
        Command::new("xdotool")
            .args(args)
            .status()
            .is_ok_and(|s| s.success())
    };
    f.fenetre().present();
    patienter(Duration::from_millis(300));
    verifier(xdotool(&["key", "alt+3"]), || "xdotool".into())?;
    let ok = attendre(Duration::from_secs(3), || indice_actif(f) == 2);
    verifier(ok, || format!("Alt+3 → onglet {}", indice_actif(f) + 1))?;
    let avant = f.onglets().len();
    verifier(xdotool(&["key", "ctrl+shift+t"]), || "xdotool".into())?;
    let ok = attendre(Duration::from_secs(10), || f.onglets().len() == avant + 1);
    verifier(ok, || "Ctrl+Maj+T (vraie frappe) sans effet".into())?;
    touche(f, gdk::Key::W, 25, CTRL_MAJ);
    attendre(Duration::from_secs(5), || f.onglets().len() == avant);
    Ok(())
}

fn suivant_precedent(f: &FenetrePupitre) -> Resultat {
    ouvrir_onglets(f, 3)?;
    touche(f, gdk::Key::_1, 10, gdk::ModifierType::ALT_MASK);
    touche(f, gdk::Key::Page_Down, 117, CTRL);
    verifier(indice_actif(f) == 1, || {
        format!("suivant → {}", indice_actif(f) + 1)
    })?;
    touche(f, gdk::Key::Page_Up, 112, CTRL);
    touche(f, gdk::Key::Page_Up, 112, CTRL);
    // Du premier onglet, « précédent » revient au dernier.
    verifier(indice_actif(f) == f.onglets().len() - 1, || {
        format!("précédent depuis le premier → {}", indice_actif(f) + 1)
    })
}

fn recherche(f: &FenetrePupitre) -> Resultat {
    touche(f, gdk::Key::_1, 10, gdk::ModifierType::ALT_MASK);
    let o = actif(f);
    let t = o.terminal().terminal().clone();
    verifier(executer(&o, "echo aig''uille", "aiguille"), || {
        format!("echo sans résultat : {:?}", texte(&o))
    })?;
    touche(f, gdk::Key::F, 41, CTRL_MAJ);
    verifier(f.recherche_ouverte(), || "barre de recherche fermée".into())?;
    let entree = f.entree_recherche();
    // Focus de la fenêtre (la fenêtre peut être inactive sous un compositeur sans écran).
    let focus = GtkWindowExt::focus(f.fenetre());
    verifier(
        focus
            .as_ref()
            .is_some_and(|w| w == entree.upcast_ref::<gtk::Widget>() || w.is_ancestor(&entree)),
        || format!("focus sur {:?}", focus.map(|w| w.type_().name())),
    )?;
    entree.set_text("aiguille");
    entree.emit_by_name::<()>("activate", &[]);
    patienter(Duration::from_millis(200));
    verifier(t.has_selection(), || "« aiguille » non trouvé".into())?;
    // Expression régulière invalide : signalée, sans plantage.
    f.recherche_expression_reguliere(true);
    entree.set_text("aig(");
    patienter(Duration::from_millis(200));
    verifier(entree.has_css_class("error"), || {
        "erreur non signalée".into()
    })?;
    // Seule occurrence déjà sélectionnée : VTE en chercherait une autre. On repart de zéro.
    t.unselect_all();
    entree.set_text("ai.u+ille");
    entree.emit_by_name::<()>("activate", &[]);
    patienter(Duration::from_millis(200));
    verifier(!entree.has_css_class("error") && t.has_selection(), || {
        format!(
            "expression régulière valide : erreur={} sélection={} infobulle={:?}",
            entree.has_css_class("error"),
            t.has_selection(),
            entree.tooltip_text()
        )
    })?;
    f.recherche_expression_reguliere(false);
    // Rien trouvé : dit à l'utilisateur (et au lecteur d'écran).
    entree.set_text("introuvable-xyz");
    entree.emit_by_name::<()>("activate", &[]);
    patienter(Duration::from_millis(200));
    verifier(f.etat_recherche() == "Aucun résultat", || {
        format!("état « {} »", f.etat_recherche())
    })?;
    entree.set_text("aiguille");
    entree.emit_by_name::<()>("activate", &[]);
    patienter(Duration::from_millis(200));
    verifier(f.etat_recherche().is_empty(), || {
        format!("état resté « {} »", f.etat_recherche())
    })?;
    // Échap ferme et rend le focus au terminal.
    f.fermer_recherche();
    patienter(Duration::from_millis(200));
    let focus = GtkWindowExt::focus(f.fenetre());
    verifier(
        !f.recherche_ouverte() && focus.as_ref() == Some(t.upcast_ref::<gtk::Widget>()),
        || {
            format!(
                "Échap : barre ouverte ou focus sur {:?}",
                focus.map(|w| w.type_().name())
            )
        },
    )
}

fn copier(f: &FenetrePupitre) -> Resultat {
    let o = actif(f);
    let t = o.terminal().terminal().clone();
    verifier(executer(&o, "echo copie-$((7*6))", "copie-42"), || {
        "echo".into()
    })?;
    t.select_all();
    touche(f, gdk::Key::C, 54, CTRL_MAJ);
    let lu: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let l = lu.clone();
    t.clipboard()
        .read_text_async(gtk::gio::Cancellable::NONE, move |r| {
            *l.borrow_mut() = Some(r.ok().flatten().map(|s| s.to_string()).unwrap_or_default());
        });
    attendre(Duration::from_secs(3), || lu.borrow().is_some());
    let contenu = lu.borrow().clone().unwrap_or_default();
    t.unselect_all();
    verifier(contenu.contains("copie-42"), || {
        format!("presse-papiers : {contenu:?}")
    })
}

/// Contenu texte du presse-papiers (vide si illisible).
fn lire_presse_papiers(t: &vte4::Terminal) -> String {
    let lu: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let l = lu.clone();
    t.clipboard()
        .read_text_async(gtk::gio::Cancellable::NONE, move |r| {
            *l.borrow_mut() = Some(r.ok().flatten().map(|s| s.to_string()).unwrap_or_default());
        });
    attendre(Duration::from_secs(3), || lu.borrow().is_some());
    let contenu = lu.borrow().clone().unwrap_or_default();
    contenu
}

/// La recherche sélectionne ce qu'elle trouve : cela ne doit pas remplacer le presse-papiers.
fn recherche_sans_copie(f: &FenetrePupitre) -> Resultat {
    let o = actif(f);
    let t = o.terminal().terminal().clone();
    t.clipboard().set_text("temoin-recherche");
    verifier(
        executer(&o, "echo trouvaille-$((3*3))", "trouvaille-9"),
        || "echo".into(),
    )?;
    touche(f, gdk::Key::F, 41, CTRL_MAJ);
    let entree = f.entree_recherche();
    entree.set_text("trouvaille-9");
    entree.emit_by_name::<()>("activate", &[]);
    let selection = attendre(Duration::from_secs(3), || {
        if !t.has_selection() {
            entree.emit_by_name::<()>("activate", &[]);
        }
        t.has_selection()
    });
    f.fermer_recherche();
    t.unselect_all();
    let contenu = lire_presse_papiers(&t);
    verifier(selection, || "la recherche n'a rien sélectionné".into())?;
    verifier(contenu == "temoin-recherche", || {
        format!("presse-papiers remplacé par la recherche : {contenu:?}")
    })
}

/// Vrai glisser de la souris sur tout le terminal (xdotool) : avec `copie_selection`, le
/// texte sélectionné arrive dans le presse-papiers ; sans, le presse-papiers ne change pas.
fn selection_souris(f: &FenetrePupitre) -> Resultat {
    let xdotool = |args: &[&str]| {
        Command::new("xdotool")
            .args(args)
            .status()
            .is_ok_and(|s| s.success())
    };
    let o = actif(f);
    let t = o.terminal().terminal().clone();
    verifier(executer(&o, "echo souris-$((8*8))", "souris-64"), || {
        "echo".into()
    })?;
    f.fenetre().present();
    patienter(Duration::from_millis(300));
    let racine = t.root().ok_or("terminal sans fenêtre")?;
    let haut = t
        .compute_point(&racine, &gtk::graphene::Point::new(2.0, 2.0))
        .ok_or("position du terminal")?;
    let bas = t
        .compute_point(
            &racine,
            &gtk::graphene::Point::new(t.width() as f32 - 2.0, t.height() as f32 - 2.0),
        )
        .ok_or("position du terminal")?;
    let glisser = || {
        let (x0, y0) = (haut.x().round().to_string(), haut.y().round().to_string());
        let (x1, y1) = (bas.x().round().to_string(), bas.y().round().to_string());
        let pid = std::process::id().to_string();
        let fenetre = Command::new("xdotool")
            .args(["search", "--onlyvisible", "--pid", &pid])
            .output()
            .ok()
            .and_then(|s| String::from_utf8(s.stdout).ok())
            .and_then(|s| s.lines().next().map(str::to_owned))
            .unwrap_or_default();
        xdotool(&["mousemove", "--window", &fenetre, &x0, &y0])
            && xdotool(&["mousedown", "1"])
            && xdotool(&["mousemove", "--window", &fenetre, &x1, &y1])
            && xdotool(&["mouseup", "1"])
    };

    t.clipboard().set_text("temoin-souris");
    verifier(glisser(), || "xdotool".into())?;
    let ok = attendre(Duration::from_secs(3), || {
        lire_presse_papiers(&t).contains("souris-64")
    });
    let contenu = lire_presse_papiers(&t);
    t.unselect_all();
    verifier(ok, || format!("sélection non copiée : {contenu:?}"))?;

    o.terminal().appliquer(
        &Config {
            copie_selection: false,
            ..Config::default()
        },
        &pupitre_config::theme_sombre(),
    );
    t.clipboard().set_text("temoin-souris");
    let res = (|| {
        verifier(glisser(), || "xdotool".into())?;
        patienter(Duration::from_millis(500));
        let selection = t.has_selection();
        let contenu = lire_presse_papiers(&t);
        verifier(selection, || "le glisser n'a rien sélectionné".into())?;
        verifier(contenu == "temoin-souris", || {
            format!("copie malgré copie_selection = false : {contenu:?}")
        })
    })();
    t.unselect_all();
    o.terminal()
        .appliquer(&Config::default(), &pupitre_config::theme_sombre());
    res
}

fn coller(f: &FenetrePupitre) -> Resultat {
    let o = actif(f);
    let t = o.terminal().terminal().clone();
    t.clipboard().set_text("echo colle-$((5*5))");
    touche(f, gdk::Key::V, 55, CTRL_MAJ);
    patienter(Duration::from_millis(300));
    t.feed_child(b"\n");
    let ok = attendre(Duration::from_secs(10), || texte(&o).contains("colle-25"));
    verifier(ok, || {
        format!("collage non reçu par le shell : {:?}", texte(&o))
    })
}

fn cloches(f: &FenetrePupitre) -> Resultat {
    let o = actif(f);
    // Dix cloches d'un coup : un seul flash.
    let avant = o.terminal().nombre_flashs();
    o.terminal().on_output(&[7u8; 10]);
    patienter(Duration::from_millis(400));
    let n = o.terminal().nombre_flashs() - avant;
    verifier(n == 1, || format!("{n} flashs pour 10 cloches simultanées"))?;
    // Dix cloches étalées : jamais plus d'un flash par 350 ms (≤ 3 par seconde).
    let avant = o.terminal().nombre_flashs();
    let debut = std::time::Instant::now();
    for _ in 0..10 {
        o.terminal().on_output(b"\x07");
        patienter(Duration::from_millis(100));
    }
    let duree = debut.elapsed().as_millis() as u32;
    let n = o.terminal().nombre_flashs() - avant;
    let maximum = 1 + duree / 350;
    verifier((1..=maximum).contains(&n), || {
        format!("{n} flashs pour 10 cloches en {duree} ms (maximum {maximum})")
    })?;
    // Cloche dans un onglet en arrière-plan : indicateur ● (et texte accessible).
    let autre = f
        .onglets()
        .into_iter()
        .find(|x| !Rc::ptr_eq(x, &o))
        .ok_or("un seul onglet")?;
    autre.terminal().on_output(b"\x07");
    patienter(Duration::from_millis(100));
    verifier(autre.signale(), || {
        "pas d'indicateur sur l'onglet en arrière-plan".into()
    })?;
    verifier(autre.titre_affiche().starts_with("● "), || {
        format!("étiquette « {} »", autre.titre_affiche())
    })?;
    // L'onglet devenu actif perd son indicateur.
    let i = f
        .onglets()
        .iter()
        .position(|x| Rc::ptr_eq(x, &autre))
        .unwrap();
    let chiffre = gdk::Key::from_name((i + 1).to_string()).ok_or("touche")?;
    touche(f, chiffre, 10 + i as u32, gdk::ModifierType::ALT_MASK);
    verifier(!autre.signale(), || {
        "indicateur resté après activation".into()
    })
}

fn lien(f: &FenetrePupitre) -> Resultat {
    let o = actif(f);
    let ouverts: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let ou = ouverts.clone();
    o.terminal().definir_lanceur(Rc::new(move |uri: &str| {
        ou.borrow_mut().push(uri.to_string())
    }));
    let t = o.terminal().terminal().clone();
    o.terminal()
        .on_output(b"\x1b[2J\x1b[Hvoir https://exemple.org/page?x=1 ici\r\n");
    attendre(Duration::from_secs(3), || texte(&o).contains("exemple.org"));
    patienter(Duration::from_millis(200));
    let (cw, ch) = (t.char_width() as f64, t.char_height() as f64);
    // Milieu de la cellule du « e » de « exemple » (colonne 13), première ligne.
    let (x, y) = (13.5 * cw, 0.5 * ch);
    let ouvert = o.terminal().clic(x, y, gdk::ModifierType::empty());
    verifier(!ouvert && ouverts.borrow().is_empty(), || {
        format!("ouvert sans Ctrl : {:?}", ouverts.borrow())
    })?;
    let ouvert = o.terminal().clic(x, y, CTRL);
    verifier(
        ouvert && ouverts.borrow().as_slice() == ["https://exemple.org/page?x=1"],
        || format!("Ctrl+clic : {:?}", ouverts.borrow()),
    )?;
    // Ctrl+clic hors d'un lien : rien.
    let ouvert = o.terminal().clic(0.5 * cw, 0.5 * ch, CTRL);
    verifier(!ouvert && ouverts.borrow().len() == 1, || {
        "ouverture hors lien".into()
    })
}

fn animations_reduites(f: &FenetrePupitre) -> Resultat {
    let reglages = gtk::Settings::default().ok_or("pas de réglages GTK")?;
    let o = actif(f);
    // Même avec le clignotement demandé, le curseur reste fixe quand le bureau demande
    // moins d'animations.
    o.terminal().appliquer(
        &Config {
            curseur_clignote: true,
            ..Config::default()
        },
        &pupitre_config::theme_sombre(),
    );
    verifier(
        o.terminal().terminal().cursor_blink_mode() == vte4::CursorBlinkMode::On,
        || "clignotement demandé non appliqué".into(),
    )?;
    reglages.set_gtk_enable_animations(false);
    patienter(Duration::from_millis(100));
    let res = (|| {
        verifier(
            o.terminal().terminal().cursor_blink_mode() == vte4::CursorBlinkMode::Off,
            || "curseur clignotant malgré la réduction des animations".into(),
        )?;
        let avant = o.terminal().nombre_flashs();
        o.terminal().on_output(b"\x07");
        patienter(Duration::from_millis(400));
        verifier(o.terminal().nombre_flashs() == avant, || {
            "flash malgré la réduction".into()
        })?;
        verifier(o.signale(), || {
            "pas d'indicateur à la place du flash".into()
        })
    })();
    reglages.set_gtk_enable_animations(true);
    o.terminal()
        .appliquer(&Config::default(), &pupitre_config::theme_sombre());
    res
}

fn plein_ecran(f: &FenetrePupitre) -> Resultat {
    if std::env::var("GDK_BACKEND").is_ok_and(|b| b == "x11") {
        // Sous X11, le plein écran est accordé par le gestionnaire de fenêtres ; Xvfb n'en
        // a pas. Vérifié sous Wayland (Weston).
        println!("  (X11 sans gestionnaire de fenêtres : vérifié sous Wayland seulement)");
        return Ok(());
    }
    touche(f, gdk::Key::F11, 95, gdk::ModifierType::empty());
    let ok = attendre(Duration::from_secs(3), || f.fenetre().is_fullscreen());
    verifier(ok, || "F11 : pas de plein écran".into())?;
    touche(f, gdk::Key::F11, 95, gdk::ModifierType::empty());
    let ok = attendre(Duration::from_secs(3), || !f.fenetre().is_fullscreen());
    verifier(ok, || "F11 : plein écran non quitté".into())
}
