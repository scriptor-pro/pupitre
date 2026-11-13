// SPDX-License-Identifier: GPL-3.0-or-later
//! Tests graphiques de la fenêtre « Réglages de Pupitre ».
//! Lancés par `scripts/test-x11.sh` et `scripts/test-wayland.sh` ; ignorés sinon.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod commun;

use commun::*;
use gtk::glib;
use gtk::prelude::*;
use pupitre::fenetre::{FenetrePupitre, Pupitre};
use pupitre::polices::{dossier_polices, enregistrer_polices_embarquees};
use pupitre_config::{charger, theme_clair, theme_sombre, Rgb};
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;
use vte4::prelude::*;

const TITRE: &str = "Réglages de Pupitre";
const NOM_APPLI: &str = "pupitre-test-reglages";
const MESSAGE_INVALIDE: &str = "pupitre.toml contient une erreur (ligne 1) : corrigez-la d'abord.";
const COMMENTAIRE: &str = "# mon commentaire à garder\n";

/// Libellés attendus des champs, dans l'ordre visuel.
const CHAMPS: &[&str] = &[
    "Police",
    "Taille de la police",
    "Thème",
    "Shell des nouveaux onglets",
    "Invite de commande",
    "Annonce de fin de commande",
    "Copier le texte sélectionné à la souris",
    "Bandeau de disposition du clavier",
    "Fin de commande : indicateur sur l'onglet",
    "Fin de commande : bulle du bureau",
    "Notification d'un programme : indicateur sur l'onglet",
    "Notification d'un programme : bulle du bureau",
    "Copie demandée par un programme : message",
    "Cloche : indicateur sur l'onglet",
];

fn main() {
    if !actives() {
        return;
    }
    bus_prive();
    compiler();
    let perso = isoler_dossiers();
    assert!(enregistrer_polices_embarquees(&dossier_polices().unwrap()));
    glib::set_prgname(Some(NOM_APPLI));
    glib::set_application_name(NOM_APPLI);
    gtk::init().unwrap();
    pupitre_i18n::init(&["fr".parse().unwrap()]);

    let config = perso.path().join(".config/pupitre/pupitre.toml");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::fs::write(&config, valide()).unwrap();
    let dossier = tempfile::tempdir().unwrap();
    let socket = dossier.path().join("pupitre/fond.sock");
    let app = gtk::Application::builder()
        .application_id("fr.pupitre.TestReglages")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gtk::gio::Cancellable::NONE).unwrap();
    let pupitre = Pupitre::demarrer(&app, socket.clone(), charger(&config).unwrap());
    pupitre.surveiller_config(config.clone());

    let mut ex = Executeur::new();
    ex.lancer("reglages_champs_etiquetes", || {
        champs_etiquetes(&pupitre, &config)
    });
    ex.lancer("changement_applique", || changement(&pupitre, &config));
    ex.lancer("notification_ecrite_dans_sa_table", || {
        notification(&pupitre, &config)
    });
    ex.lancer("fichier_invalide_lecture_seule", || {
        lecture_seule(&pupitre, &config)
    });
    ex.lancer("fichier_devenu_invalide_plus_d_ecriture", || {
        devenu_invalide(&pupitre, &config)
    });
    ex.lancer("reglages_unique", || unique(&pupitre, &config));
    ex.lancer("echap_ferme", || echap(&pupitre, &config));
    ex.lancer("menu_prefs_commence_par_reglages", || menu(&pupitre));
    drop(pupitre);
    fermer_sessions(&socket);
    drop(perso);
    ex.conclure();
}

fn valide() -> String {
    format!("{COMMENTAIRE}theme = \"clair\"\n")
}

fn premiere_fenetre(p: &Rc<Pupitre>) -> Result<FenetrePupitre, String> {
    let ok = attendre(Duration::from_secs(10), || {
        p.fenetres()
            .first()
            .is_some_and(|f| !f.onglets().is_empty())
    });
    verifier(ok, || "pas de fenêtre avec un onglet".into())?;
    Ok(p.fenetres()[0].clone())
}

fn fenetres_reglages() -> Vec<gtk::Window> {
    gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Window>().ok())
        .filter(|w| w.is_visible() && w.title().as_deref() == Some(TITRE))
        .collect()
}

fn fermer_reglages() {
    for w in fenetres_reglages() {
        w.destroy();
    }
}

fn ouvrir(f: &FenetrePupitre) -> Result<gtk::Window, String> {
    WidgetExt::activate_action(f.fenetre(), "win.reglages", None).map_err(|e| e.to_string())?;
    attendre(Duration::from_secs(10), || !fenetres_reglages().is_empty());
    fenetres_reglages()
        .into_iter()
        .next()
        .ok_or_else(|| "fenêtre « Réglages de Pupitre » absente".into())
}

/// Prépare le fichier, ferme les fenêtres restantes et ouvre la fenêtre de réglages.
fn preparer(p: &Rc<Pupitre>, config: &Path, contenu: &str) -> Result<gtk::Window, String> {
    fermer_reglages();
    std::fs::write(config, contenu).unwrap();
    let f = premiere_fenetre(p)?;
    ouvrir(&f)
}

/// Tous les descendants de `w`, dans l'ordre du parcours (ordre visuel et de tabulation).
fn descendants(w: &gtk::Widget, v: &mut Vec<gtk::Widget>) {
    let mut e = w.first_child();
    while let Some(x) = e {
        e = x.next_sibling();
        v.push(x.clone());
        descendants(&x, v);
    }
}

fn tous(w: &gtk::Window) -> Vec<gtk::Widget> {
    let mut v = Vec::new();
    descendants(w.upcast_ref(), &mut v);
    v
}

/// Champs de saisie (listes, nombre, cases) dans l'ordre.
fn controles(w: &gtk::Window) -> Vec<gtk::Widget> {
    tous(w)
        .into_iter()
        .filter(|x| {
            x.is::<gtk::DropDown>() || x.is::<gtk::SpinButton>() || x.is::<gtk::CheckButton>()
        })
        .collect()
}

fn liste_pour(w: &gtk::Window, libelle: &str) -> Result<gtk::DropDown, String> {
    for x in tous(w) {
        if let Some(l) = x.downcast_ref::<gtk::Label>() {
            if l.text() == libelle {
                if let Some(d) = l
                    .mnemonic_widget()
                    .and_then(|m| m.downcast::<gtk::DropDown>().ok())
                {
                    return Ok(d);
                }
            }
        }
    }
    Err(format!("liste « {libelle} » absente"))
}

fn case_pour(w: &gtk::Window, libelle: &str) -> Result<gtk::CheckButton, String> {
    tous(w)
        .into_iter()
        .filter_map(|x| x.downcast::<gtk::CheckButton>().ok())
        .find(|c| c.label().as_deref() == Some(libelle))
        .ok_or_else(|| format!("case « {libelle} » absente"))
}

fn textes(w: &gtk::Window) -> String {
    tous(w)
        .iter()
        .filter_map(|x| x.downcast_ref::<gtk::Label>().map(|l| l.text().to_string()))
        .collect::<Vec<_>>()
        .join("\n")
}

fn champs_etiquetes(p: &Rc<Pupitre>, config: &Path) -> Resultat {
    let w = preparer(p, config, &valide())?;
    let c = controles(&w);
    verifier(c.len() == CHAMPS.len(), || {
        format!("{} champs au lieu de {}", c.len(), CHAMPS.len())
    })?;
    // Ordre de tabulation = ordre visuel : les champs se suivent du haut vers le bas.
    let mut y_precedent = f64::MIN;
    for x in &c {
        let y = x
            .compute_point(&w, &gtk::graphene::Point::new(0.0, 0.0))
            .map(|pt| f64::from(pt.y()))
            .ok_or("position d'un champ inconnue")?;
        verifier(y >= y_precedent, || {
            "ordre de tabulation ≠ ordre visuel".into()
        })?;
        y_precedent = y;
        verifier(x.is_focusable() || x.first_child().is_some(), || {
            "champ non focalisable".into()
        })?;
    }
    // Arbre AT-SPI : chaque champ a un nom accessible (celui de son étiquette).
    let (ok, arbre) = sonder_jusqu_a(NOM_APPLI, Duration::from_secs(10), |a| {
        CHAMPS.iter().all(|n| {
            a.lines().any(|l| {
                let c: Vec<&str> = l.split('\t').collect();
                c.len() == 4 && c[1] == *n && !c[0].contains("label")
            })
        })
    });
    verifier(ok, || {
        let manquants: Vec<&&str> = CHAMPS
            .iter()
            .filter(|n| !arbre.lines().any(|l| l.split('\t').nth(1) == Some(**n)))
            .collect();
        format!("champs sans nom accessible : {manquants:?}\n{arbre}")
    })?;
    fermer_reglages();
    Ok(())
}

fn fond(f: &FenetrePupitre) -> Rgb {
    let c = f.onglets()[0]
        .terminal()
        .terminal()
        .color_background_for_draw();
    let v = |x: f32| (x * 255.0).round() as u8;
    Rgb {
        r: v(c.red()),
        g: v(c.green()),
        b: v(c.blue()),
    }
}

fn changement(p: &Rc<Pupitre>, config: &Path) -> Resultat {
    let w = preparer(p, config, &valide())?;
    let f = premiere_fenetre(p)?;
    let ok = attendre(Duration::from_secs(5), || fond(&f) == theme_clair().fond);
    verifier(ok, || format!("fond de départ {}", fond(&f)))?;
    let d = liste_pour(&w, "Thème")?;
    let debut = std::time::Instant::now();
    d.set_selected(1); // sombre
    let ok = attendre(Duration::from_secs(1), || fond(&f) == theme_sombre().fond);
    verifier(ok, || {
        format!(
            "fond {} après {} ms (attendu {})",
            fond(&f),
            debut.elapsed().as_millis(),
            theme_sombre().fond
        )
    })?;
    let texte = std::fs::read_to_string(config).unwrap();
    verifier(
        texte == format!("{COMMENTAIRE}theme = \"sombre\"\n"),
        || format!("fichier : {texte:?}"),
    )?;
    verifier(config.with_extension("toml.bak").exists(), || {
        "pas de pupitre.toml.bak".into()
    })?;
    verifier(textes(&w).contains("Enregistré."), || {
        "message « Enregistré. » absent".into()
    })?;
    fermer_reglages();
    Ok(())
}

fn notification(p: &Rc<Pupitre>, config: &Path) -> Resultat {
    let w = preparer(p, config, &valide())?;
    let c = case_pour(&w, "Cloche : indicateur sur l'onglet")?;
    verifier(c.is_active(), || "case décochée au départ".into())?;
    c.set_active(false);
    let texte = std::fs::read_to_string(config).unwrap();
    let cfg = charger(config).map_err(|e| e.to_string())?;
    verifier(
        !cfg.notifications.cloche_onglet && cfg.notifications.fin_commande_bulle,
        || format!("fichier : {texte:?}"),
    )?;
    verifier(texte.starts_with(COMMENTAIRE), || {
        "commentaire perdu".into()
    })?;
    fermer_reglages();
    Ok(())
}

fn lecture_seule(p: &Rc<Pupitre>, config: &Path) -> Resultat {
    let invalide = "theme = \"inconnu\"\n";
    let w = preparer(p, config, invalide)?;
    verifier(controles(&w).is_empty(), || {
        format!(
            "{} champs modifiables dans une fenêtre en lecture seule",
            controles(&w).len()
        )
    })?;
    verifier(textes(&w).contains(MESSAGE_INVALIDE), || {
        format!("message absent : {:?}", textes(&w))
    })?;
    verifier(std::fs::read_to_string(config).unwrap() == invalide, || {
        "fichier modifié".into()
    })?;
    fermer_reglages();
    Ok(())
}

fn devenu_invalide(p: &Rc<Pupitre>, config: &Path) -> Resultat {
    let w = preparer(p, config, &valide())?;
    let invalide = "theme = \"inconnu\"\n";
    std::fs::write(config, invalide).unwrap();
    let c = case_pour(&w, "Cloche : indicateur sur l'onglet")?;
    c.set_active(false);
    verifier(std::fs::read_to_string(config).unwrap() == invalide, || {
        "fichier invalide écrasé".into()
    })?;
    verifier(textes(&w).contains(MESSAGE_INVALIDE), || {
        format!("message absent : {:?}", textes(&w))
    })?;
    verifier(controles(&w).iter().all(|x| !x.is_sensitive()), || {
        "champs encore modifiables".into()
    })?;
    fermer_reglages();
    Ok(())
}

fn unique(p: &Rc<Pupitre>, config: &Path) -> Resultat {
    let w = preparer(p, config, &valide())?;
    let _ = w;
    let f = premiere_fenetre(p)?;
    WidgetExt::activate_action(f.fenetre(), "win.reglages", None).map_err(|e| e.to_string())?;
    patienter(Duration::from_millis(300));
    verifier(fenetres_reglages().len() == 1, || {
        format!("{} fenêtres de réglages", fenetres_reglages().len())
    })?;
    fermer_reglages();
    Ok(())
}

fn echap(p: &Rc<Pupitre>, config: &Path) -> Resultat {
    let w = preparer(p, config, &valide())?;
    let c = w.observe_controllers();
    let traite = (0..c.n_items())
        .filter_map(|i| c.item(i))
        .filter_map(|c| c.downcast::<gtk::EventControllerKey>().ok())
        .any(|c| {
            c.emit_by_name::<bool>(
                "key-pressed",
                &[
                    &gtk::gdk::Key::Escape,
                    &0u32,
                    &gtk::gdk::ModifierType::empty(),
                ],
            )
        });
    verifier(traite, || "Échap non traité".into())?;
    attendre(Duration::from_secs(2), || fenetres_reglages().is_empty());
    verifier(fenetres_reglages().is_empty(), || {
        "Échap n'a pas fermé la fenêtre".into()
    })
}

fn menu(p: &Rc<Pupitre>) -> Resultat {
    let f = premiere_fenetre(p)?;
    let modele = f.bouton_menu().menu_model().ok_or("pas de menu")?;
    let mut trouve = None;
    for i in 0..modele.n_items() {
        let Some(section) = modele.item_link(i, "section") else {
            continue;
        };
        for j in 0..section.n_items() {
            if let Some(sous) = section.item_link(j, "submenu") {
                let premier = sous
                    .item_attribute_value(0, "label", None)
                    .and_then(|v| v.str().map(String::from));
                let action = sous
                    .item_attribute_value(0, "action", None)
                    .and_then(|v| v.str().map(String::from));
                if action.as_deref() == Some("win.reglages") {
                    trouve = premier;
                }
            }
        }
    }
    verifier(trouve.as_deref() == Some("Réglages…"), || {
        format!("tête du sous-menu Préférences : {trouve:?}")
    })
}
