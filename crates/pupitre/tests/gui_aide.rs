// SPDX-License-Identifier: GPL-3.0-or-later
//! Tests graphiques de l'aide intégrée (Ctrl+Maj+H, menu « Aide »). Lancés par
//! `scripts/test-x11.sh` et `scripts/test-wayland.sh` ; ignorés sinon.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod commun;

use commun::*;
use gtk::glib;
use gtk::prelude::*;
use pupitre::aide::{ouvrir_aide, sections};
use pupitre::fenetre::{FenetrePupitre, Pupitre};
use pupitre::polices::{dossier_polices, enregistrer_polices_embarquees};
use pupitre_config::Config;
use std::rc::Rc;
use std::time::Duration;

const TITRE: &str = "Aide de Pupitre";

fn main() {
    if !actives() {
        return;
    }
    bus_prive();
    compiler();
    let perso = isoler_dossiers();
    assert!(enregistrer_polices_embarquees(&dossier_polices().unwrap()));
    glib::set_prgname(Some("pupitre-test-aide"));
    glib::set_application_name("pupitre-test-aide");
    gtk::init().unwrap();
    pupitre_i18n::init(&["fr".parse().unwrap()]);

    let dossier = tempfile::tempdir().unwrap();
    let socket = dossier.path().join("pupitre/fond.sock");
    let app = gtk::Application::builder()
        .application_id("fr.pupitre.TestAide")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gtk::gio::Cancellable::NONE).unwrap();
    let pupitre = Pupitre::demarrer(&app, socket.clone(), Config::default());

    let mut ex = Executeur::new();
    ex.lancer("aide_ouverte_par_le_menu", || {
        aide_ouverte_par_le_menu(&pupitre, &app)
    });
    ex.lancer("raccourci_ouvre_l_aide", || {
        raccourci_ouvre_l_aide(&pupitre)
    });
    ex.lancer("aide_unique", || aide_unique(&pupitre, &app));
    ex.lancer("aide_echap_ferme", || aide_echap_ferme(&pupitre));
    ex.lancer("section_choisie_affiche_son_texte", || {
        section_choisie(&pupitre, &app)
    });
    ex.lancer("recherche_filtre_les_sections", || recherche(&pupitre));
    ex.lancer("ctrl_tab_passe_d_une_zone_a_l_autre", || zones(&pupitre));
    ex.lancer("menu_se_termine_par_l_aide", || menu_aide(&pupitre));
    drop(pupitre);
    fermer_sessions(&socket);
    drop(perso);
    ex.conclure();
}

fn fenetres_aide() -> Vec<gtk::Window> {
    gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Window>().ok())
        .filter(|w| w.is_visible() && w.title().as_deref() == Some(TITRE))
        .collect()
}

fn fermer_aides() {
    for w in fenetres_aide() {
        w.destroy();
    }
}

fn attendre_aide() -> Result<gtk::Window, String> {
    attendre(Duration::from_secs(10), || !fenetres_aide().is_empty());
    let f = fenetres_aide()
        .into_iter()
        .next()
        .ok_or("fenêtre « Aide de Pupitre » absente")?;
    verifier(f.application().is_some(), || {
        "fenêtre non rattachée à l'application".into()
    })?;
    Ok(f)
}

fn descendants(w: &gtk::Widget, v: &mut Vec<gtk::Widget>) {
    let mut e = w.first_child();
    while let Some(x) = e {
        e = x.next_sibling();
        v.push(x.clone());
        descendants(&x, v);
    }
}

fn par_nom(f: &gtk::Window, nom: &str) -> Result<gtk::Widget, String> {
    let mut v = Vec::new();
    descendants(f.upcast_ref(), &mut v);
    v.into_iter()
        .find(|w| w.widget_name() == nom)
        .ok_or_else(|| format!("widget « {nom} » absent"))
}

fn lignes_visibles(f: &gtk::Window) -> Result<Vec<gtk::ListBoxRow>, String> {
    let liste = par_nom(f, "liste")?;
    let mut v = Vec::new();
    let mut e = liste.first_child();
    while let Some(x) = e {
        e = x.next_sibling();
        if let Ok(l) = x.downcast::<gtk::ListBoxRow>() {
            if l.is_child_visible() && l.is_mapped() {
                v.push(l);
            }
        }
    }
    Ok(v)
}

fn texte_affiche(f: &gtk::Window) -> Result<String, String> {
    let t = par_nom(f, "texte")?
        .downcast::<gtk::TextView>()
        .map_err(|_| "« texte » n'est pas un GtkTextView")?;
    let b = t.buffer();
    Ok(b.text(&b.start_iter(), &b.end_iter(), false).to_string())
}

fn focus_dans(f: &gtk::Window, w: &gtk::Widget) -> bool {
    GtkWindowExt::focus(f).is_some_and(|x| x == *w || x.is_ancestor(w))
}

fn touche(f: &gtk::Window, t: gtk::gdk::Key, mods: gtk::gdk::ModifierType) -> bool {
    let c = f.observe_controllers();
    (0..c.n_items())
        .filter_map(|i| c.item(i))
        .filter_map(|c| c.downcast::<gtk::EventControllerKey>().ok())
        .any(|c| c.emit_by_name::<bool>("key-pressed", &[&t, &0u32, &mods]))
}

fn premiere_fenetre(p: &Rc<Pupitre>) -> Result<FenetrePupitre, String> {
    let ok = attendre(Duration::from_secs(10), || !p.fenetres().is_empty());
    verifier(ok, || "pas de fenêtre".into())?;
    Ok(p.fenetres()[0].clone())
}

fn aide_ouverte_par_le_menu(p: &Rc<Pupitre>, app: &gtk::Application) -> Resultat {
    fermer_aides();
    let f = premiere_fenetre(p)?;
    WidgetExt::activate_action(f.fenetre(), "win.aide", None).map_err(|e| e.to_string())?;
    let w = attendre_aide()?;
    verifier(w.application().as_ref() == Some(app), || {
        "pas rattachée à l'application de Pupitre".into()
    })?;
    let n = sections(pupitre_i18n::aide()).len();
    let l = lignes_visibles(&w)?;
    verifier(!l.is_empty() && l.len() >= n.min(5), || {
        format!("liste : {} lignes pour {n} sections", l.len())
    })?;
    let liste = par_nom(&w, "liste")?;
    verifier(
        attendre(Duration::from_secs(2), || focus_dans(&w, &liste)),
        || "le focus n'est pas sur la liste".into(),
    )?;
    let t = par_nom(&w, "texte")?
        .downcast::<gtk::TextView>()
        .map_err(|_| "pas un GtkTextView")?;
    verifier(!t.is_editable() && t.is_cursor_visible(), || {
        "texte : lecture seule avec curseur visible attendu".into()
    })?;
    fermer_aides();
    Ok(())
}

fn raccourci_ouvre_l_aide(p: &Rc<Pupitre>) -> Resultat {
    fermer_aides();
    let f = premiere_fenetre(p)?;
    let cs = gtk::gdk::ModifierType::CONTROL_MASK | gtk::gdk::ModifierType::SHIFT_MASK;
    verifier(f.traiter_touche(gtk::gdk::Key::H, 43, cs), || {
        "Ctrl+Maj+H non reconnu".into()
    })?;
    attendre_aide()?;
    // F1 reste au terminal.
    verifier(
        !f.traiter_touche(gtk::gdk::Key::F1, 67, gtk::gdk::ModifierType::empty()),
        || "F1 ne doit pas être pris".into(),
    )?;
    fermer_aides();
    Ok(())
}

fn aide_unique(p: &Rc<Pupitre>, app: &gtk::Application) -> Resultat {
    fermer_aides();
    let f = premiere_fenetre(p)?;
    ouvrir_aide(app, Some(f.fenetre().upcast_ref()), None);
    attendre_aide()?;
    ouvrir_aide(app, Some(f.fenetre().upcast_ref()), None);
    WidgetExt::activate_action(f.fenetre(), "win.aide", None).map_err(|e| e.to_string())?;
    patienter(Duration::from_millis(300));
    verifier(fenetres_aide().len() == 1, || {
        format!("{} fenêtres d'aide", fenetres_aide().len())
    })?;
    fermer_aides();
    // Refermée, elle peut être rouverte.
    ouvrir_aide(app, Some(f.fenetre().upcast_ref()), None);
    attendre_aide()?;
    fermer_aides();
    Ok(())
}

fn aide_echap_ferme(p: &Rc<Pupitre>) -> Resultat {
    fermer_aides();
    let f = premiere_fenetre(p)?;
    f.executer(pupitre::raccourcis::Raccourci::Aide);
    let w = attendre_aide()?;
    verifier(
        touche(&w, gtk::gdk::Key::Escape, gtk::gdk::ModifierType::empty()),
        || "Échap non traité".into(),
    )?;
    verifier(
        attendre(Duration::from_secs(2), || fenetres_aide().is_empty()),
        || "Échap ne ferme pas l'aide".into(),
    )
}

fn section_choisie(p: &Rc<Pupitre>, app: &gtk::Application) -> Resultat {
    fermer_aides();
    let f = premiere_fenetre(p)?;
    ouvrir_aide(
        app,
        Some(f.fenetre().upcast_ref()),
        Some("Raccourcis clavier"),
    );
    let w = attendre_aide()?;
    let ok = attendre(Duration::from_secs(2), || {
        texte_affiche(&w).is_ok_and(|t| t.contains("Nouvel onglet : Ctrl+Maj+T"))
    });
    verifier(ok, || {
        format!("texte de la section : {:?}", texte_affiche(&w))
    })?;
    verifier(!texte_affiche(&w)?.contains('|'), || {
        "tableau non converti en lignes".into()
    })?;
    // Rouvrir sur une autre section change la section de la même fenêtre.
    ouvrir_aide(app, None, Some("Polices"));
    let ok = attendre(Duration::from_secs(2), || {
        texte_affiche(&w).is_ok_and(|t| t.contains("B612"))
    });
    verifier(ok && fenetres_aide().len() == 1, || {
        "la section n'a pas changé dans la fenêtre existante".into()
    })?;
    fermer_aides();
    Ok(())
}

fn recherche(p: &Rc<Pupitre>) -> Resultat {
    fermer_aides();
    let f = premiere_fenetre(p)?;
    f.executer(pupitre::raccourcis::Raccourci::Aide);
    let w = attendre_aide()?;
    let tout = lignes_visibles(&w)?.len();
    let champ = par_nom(&w, "recherche")?
        .dynamic_cast::<gtk::Editable>()
        .map_err(|_| "recherche : pas un champ de saisie")?;
    champ.set_text("trousseau");
    let ok = attendre(Duration::from_secs(2), || {
        lignes_visibles(&w).is_ok_and(|l| !l.is_empty() && l.len() < tout)
    });
    verifier(ok, || {
        format!(
            "filtre sans effet : {:?} / {tout}",
            lignes_visibles(&w).map(|l| l.len())
        )
    })?;
    champ.set_text("zzzzqqqq");
    let ok = attendre(Duration::from_secs(2), || {
        lignes_visibles(&w).is_ok_and(|l| l.is_empty())
    });
    verifier(ok, || "aucune section attendue".into())?;
    champ.set_text("");
    let ok = attendre(Duration::from_secs(2), || {
        lignes_visibles(&w).is_ok_and(|l| l.len() == tout)
    });
    verifier(ok, || {
        "toutes les sections attendues après effacement".into()
    })?;
    fermer_aides();
    Ok(())
}

fn zones(p: &Rc<Pupitre>) -> Resultat {
    fermer_aides();
    let f = premiere_fenetre(p)?;
    f.executer(pupitre::raccourcis::Raccourci::Aide);
    let w = attendre_aide()?;
    let (recherche, liste, texte) = (
        par_nom(&w, "recherche")?,
        par_nom(&w, "liste")?,
        par_nom(&w, "texte")?,
    );
    attendre(Duration::from_secs(2), || focus_dans(&w, &liste));
    let ctrl = gtk::gdk::ModifierType::CONTROL_MASK;
    let cs = ctrl | gtk::gdk::ModifierType::SHIFT_MASK;
    verifier(touche(&w, gtk::gdk::Key::Tab, ctrl), || {
        "Ctrl+Tab non traité".into()
    })?;
    verifier(focus_dans(&w, &texte), || {
        "Ctrl+Tab : le texte devrait avoir le focus".into()
    })?;
    touche(&w, gtk::gdk::Key::Tab, ctrl);
    verifier(focus_dans(&w, &recherche), || {
        "Ctrl+Tab : la recherche devrait avoir le focus".into()
    })?;
    touche(&w, gtk::gdk::Key::ISO_Left_Tab, cs);
    verifier(focus_dans(&w, &texte), || {
        "Ctrl+Maj+Tab : le texte devrait avoir le focus".into()
    })?;
    touche(&w, gtk::gdk::Key::ISO_Left_Tab, cs);
    verifier(focus_dans(&w, &liste), || {
        "Ctrl+Maj+Tab : la liste devrait avoir le focus".into()
    })?;
    fermer_aides();
    Ok(())
}

fn menu_aide(p: &Rc<Pupitre>) -> Resultat {
    let f = premiere_fenetre(p)?;
    fn parcourir(m: &gtk::gio::MenuModel, v: &mut Vec<(String, String)>) {
        for i in 0..m.n_items() {
            let attr = |nom: &str| {
                m.item_attribute_value(i, nom, None)
                    .and_then(|x| x.str().map(String::from))
                    .unwrap_or_default()
            };
            let (libelle, action) = (attr("label"), attr("action"));
            if !libelle.is_empty() {
                v.push((libelle, action));
            }
            if let Some(s) = m.item_link(i, "section") {
                parcourir(&s, v);
            }
        }
    }
    let modele = f.bouton_menu().menu_model().ok_or("pas de menu")?;
    let mut items = Vec::new();
    parcourir(&modele, &mut items);
    verifier(
        items.iter().rev().nth(1) == Some(&("Aide".into(), "win.aide".into())),
        || format!("« Aide » devrait précéder « À propos » en fin de menu : {items:?}"),
    )
}
