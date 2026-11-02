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
