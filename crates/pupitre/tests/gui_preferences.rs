// SPDX-License-Identifier: GPL-3.0-or-later
//! Tests graphiques du menu « Préférences ». Lancés par `scripts/test-x11.sh` et
//! `scripts/test-wayland.sh` ; ignorés sinon. Aucun éditeur réel n'est lancé : le lanceur est
//! simulé, dans un dossier de configuration temporaire.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod commun;

use commun::*;
use gtk::glib;
use gtk::prelude::*;
use pupitre::fenetre::{FenetrePupitre, Pupitre};
use pupitre::polices::{dossier_polices, enregistrer_polices_embarquees};
use pupitre::preferences::{chemin, FichierPrefs, Lanceur};
use pupitre_config::Config;
use std::rc::Rc;
use std::time::Duration;
use vte4::prelude::*;

fn main() {
    if !actives() {
        return;
    }
    bus_prive();
    compiler();
    let perso = isoler_dossiers();
    assert!(enregistrer_polices_embarquees(&dossier_polices().unwrap()));
    glib::set_prgname(Some("pupitre-test-preferences"));
    glib::set_application_name("pupitre-test-preferences");
    gtk::init().unwrap();
    pupitre_i18n::init(&["fr".parse().unwrap()]);

    let dossier = tempfile::tempdir().unwrap();
    let socket = dossier.path().join("pupitre/fond.sock");
    let app = gtk::Application::builder()
        .application_id("fr.pupitre.TestPreferences")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gtk::gio::Cancellable::NONE).unwrap();
    let pupitre = Pupitre::demarrer(&app, socket.clone(), Config::default());

    let mut ex = Executeur::new();
    ex.lancer("menu_preferences", || menu_preferences(&pupitre));
    ex.lancer("ouverture_impossible_message", || {
        ouverture_impossible(&pupitre)
    });
    ex.lancer("ouverture_reussie_message", || ouverture_reussie(&pupitre));
    ex.lancer("editeur_du_terminal_dans_un_onglet", || {
        editeur_terminal(&pupitre)
    });
    drop(pupitre);
    fermer_sessions(&socket);
    drop(perso);
    ex.conclure();
}

fn premiere_fenetre(p: &Rc<Pupitre>) -> Result<FenetrePupitre, String> {
    let ok = attendre(Duration::from_secs(10), || !p.fenetres().is_empty());
    verifier(ok, || "pas de fenêtre".into())?;
    Ok(p.fenetres()[0].clone())
}

fn descendants(w: &gtk::Widget, v: &mut Vec<gtk::Widget>) {
    let mut e = w.first_child();
    while let Some(x) = e {
        v.push(x.clone());
        descendants(&x, v);
        e = x.next_sibling();
    }
}

fn menu_preferences(p: &Rc<Pupitre>) -> Resultat {
    let f = premiere_fenetre(p)?;
    let bouton = f.bouton_menu();
    let modele = bouton.menu_model().ok_or("pas de menu")?;
    // Sous-menu « Préférences » juste avant la section « Aide ».
    let mut trouve = None;
    for i in 0..modele.n_items() {
        if let Some(s) = modele.item_link(i, "section") {
            for j in 0..s.n_items() {
                let l = s
                    .item_attribute_value(j, "label", None)
                    .and_then(|x| x.str().map(String::from));
                if l.as_deref() == Some("Préférences") {
                    trouve = s.item_link(j, "submenu").map(|m| (i, m));
                }
            }
        }
    }
    let (rang, sous) = trouve.ok_or("sous-menu « Préférences » absent")?;
    verifier(rang + 2 == modele.n_items(), || {
        "« Préférences » devrait être l'avant-dernière section".into()
    })?;
    let mut entrees = Vec::new();
    for j in 0..sous.n_items() {
        let a = |n: &str| {
            sous.item_attribute_value(j, n, None)
                .and_then(|x| x.str().map(String::from))
                .unwrap_or_default()
        };
        entrees.push((a("label"), a("action")));
    }
    let attendu: Vec<(String, String)> = [
        ("Modifier la configuration…", "win.prefs-configuration"),
        ("Modifier l'invite…", "win.prefs-invite"),
        ("Modifier les connexions…", "win.prefs-connexions"),
    ]
    .iter()
    .map(|(a, b)| (a.to_string(), b.to_string()))
    .collect();
    verifier(entrees == attendu, || format!("entrées : {entrees:?}"))?;
    for (_, action) in &entrees {
        let nom = action.trim_start_matches("win.");
        let a = f
            .fenetre()
            .lookup_action(nom)
            .ok_or(format!("action {nom} absente"))?;
        let _ = a;
        verifier(f.fenetre().is_action_enabled(nom), || {
            format!("{nom} inactive")
        })?;
    }
    // Accessibles au clavier : les boutons du menu ouvert prennent le focus.
    bouton.popup();
    let popover = bouton.popover().ok_or("pas de popover")?;
    let ok = attendre(Duration::from_secs(5), || popover.is_visible());
    verifier(ok, || "menu non ouvert".into())?;
    let mut tous = Vec::new();
    descendants(popover.upcast_ref(), &mut tous);
    let prefs = tous
        .iter()
        .find(|w| {
            w.type_().name() == "GtkModelButton" && w.property::<String>("text") == "Préférences"
        })
        .ok_or("bouton « Préférences » introuvable")?;
    let res = verifier(prefs.is_focusable(), || {
        "« Préférences » non focalisable".into()
    });
    bouton.popdown();
    res
}

fn lanceur_en_echec() -> Lanceur {
    Box::new(|_, fin| fin(Err("aucun éditeur".into())))
}

fn ouverture_impossible(p: &Rc<Pupitre>) -> Resultat {
    let f = premiere_fenetre(p)?;
    f.ouvrir_preference_avec(FichierPrefs::Connexions, lanceur_en_echec());
    let c = chemin(FichierPrefs::Connexions);
    let msgs = f.messages();
    verifier(
        msgs.iter().any(|m| m.contains(&c.display().to_string())),
        || format!("le chemin {c:?} devrait figurer dans {msgs:?}"),
    )?;
    verifier(c.exists(), || "le fichier devrait avoir été créé".into())
}

/// `$VISUAL` défini : le fichier s'ouvre avec lui dans un nouvel onglet de Pupitre (ici
/// `tail -f`, qui affiche le fichier et reste ouvert comme un éditeur : un programme qui se
/// termine normalement fermerait l'onglet), nommé d'après le fichier.
fn editeur_terminal(p: &Rc<Pupitre>) -> Resultat {
    let f = premiere_fenetre(p)?;
    let ok = attendre(Duration::from_secs(10), || !f.onglets().is_empty());
    verifier(ok, || "pas d'onglet au démarrage".into())?;
    let avant = f.onglets().len();
    let (visual, editor) = (std::env::var_os("VISUAL"), std::env::var_os("EDITOR"));
    std::env::set_var("VISUAL", "tail -n 200 -f");
    std::env::remove_var("EDITOR");
    f.ouvrir_preference(FichierPrefs::Configuration);
    for (v, a) in [("VISUAL", visual), ("EDITOR", editor)] {
        match a {
            Some(a) => std::env::set_var(v, a),
            None => std::env::remove_var(v),
        }
    }
    let ok = attendre(Duration::from_secs(10), || f.onglets().len() == avant + 1);
    verifier(ok, || "pas de nouvel onglet pour l'éditeur".into())?;
    let o = f.onglet_actif().ok_or("aucun onglet actif")?;
    let texte = || {
        o.terminal()
            .terminal()
            .text_format(vte4::Format::Text)
            .map(|s| s.to_string())
            .unwrap_or_default()
    };
    // La fin du pupitre.toml par défaut (le début a défilé hors de la vue).
    let ok = attendre(Duration::from_secs(10), || {
        texte().contains("# aide = \"Ctrl+Maj+H\"")
    });
    verifier(ok, || format!("fichier non affiché : {:?}", texte()))?;
    verifier(o.titre() == "pupitre.toml", || {
        format!("titre de l'onglet : {:?}", o.titre())
    })
}

fn ouverture_reussie(p: &Rc<Pupitre>) -> Resultat {
    let f = premiere_fenetre(p)?;
    f.ouvrir_preference_avec(FichierPrefs::Invite, Box::new(|_, fin| fin(Ok(()))));
    let msgs = f.messages();
    verifier(
        msgs.iter()
            .any(|m| m == "Les modifications s'appliquent dès l'enregistrement."),
        || format!("confirmation absente : {msgs:?}"),
    )
}
