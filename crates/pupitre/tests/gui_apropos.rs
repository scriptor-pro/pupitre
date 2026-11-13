// SPDX-License-Identifier: GPL-3.0-or-later
//! Tests graphiques de « À propos de Pupitre » et du bouton « + » de la barre d'onglets.
//! Lancés par `scripts/test-x11.sh` et `scripts/test-wayland.sh` ; ignorés sinon.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod commun;

use commun::*;
use gtk::glib;
use gtk::prelude::*;
use pupitre::fenetre::{FenetrePupitre, Onglet, Pupitre};
use pupitre::polices::{dossier_polices, enregistrer_polices_embarquees, POLICES_EMBARQUEES};
use pupitre_config::Config;
use std::rc::Rc;
use std::time::Duration;
use vte4::prelude::*;

const TITRE: &str = "À propos de Pupitre";

fn main() {
    if !actives() {
        return;
    }
    bus_prive();
    compiler();
    let perso = isoler_dossiers();
    assert!(enregistrer_polices_embarquees(&dossier_polices().unwrap()));
    glib::set_prgname(Some("pupitre-test-apropos"));
    glib::set_application_name("pupitre-test-apropos");
    gtk::init().unwrap();
    pupitre_i18n::init(&["fr".parse().unwrap()]);

    let dossier = tempfile::tempdir().unwrap();
    let socket = dossier.path().join("pupitre/fond.sock");
    let app = gtk::Application::builder()
        .application_id("fr.pupitre.TestApropos")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gtk::gio::Cancellable::NONE).unwrap();
    let pupitre = Pupitre::demarrer(&app, socket.clone(), Config::default());

    let mut ex = Executeur::new();
    ex.lancer("apropos_contenu", || apropos_contenu(&pupitre));
    ex.lancer("apropos_unique", || apropos_unique(&pupitre));
    ex.lancer("apropos_echap_ferme", || apropos_echap(&pupitre));
    ex.lancer("menu_se_termine_par_a_propos", || menu_fin(&pupitre));
    ex.lancer("bouton_nouvel_onglet", || bouton(&pupitre));
    ex.lancer("bouton_dans_le_cycle_ctrl_tab", || cycle(&pupitre));
    drop(pupitre);
    fermer_sessions(&socket);
    drop(perso);
    ex.conclure();
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

fn fenetres_apropos() -> Vec<gtk::Window> {
    gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Window>().ok())
        .filter(|w| w.is_visible() && w.title().as_deref() == Some(TITRE))
        .collect()
}

fn fermer_apropos() {
    for w in fenetres_apropos() {
        w.destroy();
    }
}

fn ouvrir(f: &FenetrePupitre) -> Result<gtk::Window, String> {
    WidgetExt::activate_action(f.fenetre(), "win.a-propos", None).map_err(|e| e.to_string())?;
    attendre(Duration::from_secs(10), || !fenetres_apropos().is_empty());
    fenetres_apropos()
        .into_iter()
        .next()
        .ok_or_else(|| "fenêtre « À propos de Pupitre » absente".into())
}

/// Tous les textes des étiquettes de la fenêtre, réunis.
fn textes(w: &gtk::Widget, sortie: &mut String) {
    if let Some(l) = w.downcast_ref::<gtk::Label>() {
        sortie.push_str(&l.text());
        sortie.push('\n');
    }
    let mut e = w.first_child();
    while let Some(x) = e {
        e = x.next_sibling();
        textes(&x, sortie);
    }
}

/// Montre la page « Crédits » : GTK n'en construit le contenu qu'à ce moment.
fn afficher_credits(w: &gtk::Widget) {
    if let Some(pile) = w.downcast_ref::<gtk::Stack>() {
        if pile.child_by_name("credits").is_some() {
            pile.set_visible_child_name("credits");
        }
    }
    let mut e = w.first_child();
    while let Some(x) = e {
        e = x.next_sibling();
        afficher_credits(&x);
    }
}

fn apropos_contenu(p: &Rc<Pupitre>) -> Resultat {
    fermer_apropos();
    let f = premiere_fenetre(p)?;
    let w = ouvrir(&f)?;
    let d = w
        .downcast::<gtk::AboutDialog>()
        .map_err(|_| "pas un GtkAboutDialog")?;
    verifier(d.version().as_deref() == Some("0.1.0"), || {
        format!("version : {:?}", d.version())
    })?;
    verifier(
        d.website().as_deref() == Some("https://github.com/scriptor-pro/pupitre"),
        || format!("site : {:?}", d.website()),
    )?;
    verifier(d.license_type() == gtk::License::Gpl30, || {
        "licence : pas GPL 3 ou ultérieure".into()
    })?;
    verifier(
        d.comments().as_deref() == Some("Terminal à onglets accessible"),
        || format!("commentaire : {:?}", d.comments()),
    )?;
    verifier(d.transient_for().is_some(), || {
        "pas rattachée à la fenêtre".into()
    })?;
    // Les polices et leur licence sont dans une section de crédits (ouverte par un bouton) :
    // on les cherche dans le texte de la fenêtre, après avoir ouvert les crédits.
    afficher_credits(d.upcast_ref());
    patienter(Duration::from_millis(300));
    let mut t = String::new();
    textes(d.upcast_ref(), &mut t);
    for pol in POLICES_EMBARQUEES {
        verifier(t.contains(pol.famille), || {
            format!("police {} absente de : {t}", pol.famille)
        })?;
    }
    verifier(t.contains("Polices"), || {
        "section « Polices » absente".into()
    })?;
    verifier(t.contains("SIL Open Font License 1.1"), || {
        "licence des polices absente".into()
    })?;
    fermer_apropos();
    Ok(())
}

fn apropos_unique(p: &Rc<Pupitre>) -> Resultat {
    fermer_apropos();
    let f = premiere_fenetre(p)?;
    ouvrir(&f)?;
    WidgetExt::activate_action(f.fenetre(), "win.a-propos", None).map_err(|e| e.to_string())?;
    patienter(Duration::from_millis(300));
    verifier(fenetres_apropos().len() == 1, || {
        format!("{} fenêtres « À propos »", fenetres_apropos().len())
    })?;
    fermer_apropos();
    Ok(())
}

fn apropos_echap(p: &Rc<Pupitre>) -> Resultat {
    fermer_apropos();
    let f = premiere_fenetre(p)?;
    let w = ouvrir(&f)?;
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
    attendre(Duration::from_secs(2), || fenetres_apropos().is_empty());
    verifier(fenetres_apropos().is_empty(), || {
        "Échap n'a pas fermé la fenêtre".into()
    })
}

fn menu_fin(p: &Rc<Pupitre>) -> Resultat {
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
    let n = items.len();
    verifier(
        n >= 2
            && items[n - 2] == ("Aide".into(), "win.aide".into())
            && items[n - 1] == ("À propos de Pupitre".into(), "win.a-propos".into()),
        || format!("« À propos » devrait suivre « Aide » en fin de menu : {items:?}"),
    )
}

fn texte(o: &Onglet) -> String {
    o.terminal()
        .terminal()
        .text_format(vte4::Format::Text)
        .map(|s| s.to_string())
        .unwrap_or_default()
}

fn bouton(p: &Rc<Pupitre>) -> Resultat {
    let f = premiere_fenetre(p)?;
    let b = f.bouton_nouvel_onglet();
    let infobulle = b.tooltip_text().map(|s| s.to_string()).unwrap_or_default();
    verifier(infobulle == "Nouvel onglet (Ctrl+Maj+T)", || {
        format!("infobulle : {infobulle:?}")
    })?;
    let (ok, arbre) = sonder_jusqu_a("pupitre-test-apropos", Duration::from_secs(10), |a| {
        a.lines().any(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            c.len() == 4 && c[0] == "button" && c[1] == "Nouvel onglet"
        })
    });
    verifier(ok, || {
        format!("bouton « Nouvel onglet » absent de l'arbre AT-SPI : {arbre}")
    })?;
    // Même shell que l'onglet actif : un onglet sh, puis le bouton.
    let (_, cible) = f
        .entrees_nouvel_onglet_avec()
        .into_iter()
        .find(|(l, _)| l == "sh")
        .ok_or("/bin/sh absent du sous-menu")?;
    let avant = f.onglets().len();
    WidgetExt::activate_action(
        f.fenetre(),
        "win.nouvel-onglet-avec",
        Some(&cible.to_variant()),
    )
    .map_err(|e| e.to_string())?;
    let ok = attendre(Duration::from_secs(10), || f.onglets().len() == avant + 1);
    verifier(ok, || "onglet sh non ouvert".into())?;
    b.emit_clicked();
    let ok = attendre(Duration::from_secs(10), || f.onglets().len() == avant + 2);
    verifier(ok, || "le clic n'a pas ouvert d'onglet".into())?;
    let o = f.onglet_actif().ok_or("aucun onglet actif")?;
    o.terminal()
        .terminal()
        .feed_child(b"echo \"[$(ps -p $$ -o comm=)]\"\n");
    let ok = attendre(Duration::from_secs(10), || texte(&o).contains("[sh]"));
    verifier(ok, || format!("shell non repris : {:?}", texte(&o)))
}

fn cycle(p: &Rc<Pupitre>) -> Resultat {
    let f = premiere_fenetre(p)?;
    let b: gtk::Widget = f.bouton_nouvel_onglet().upcast();
    let menu: gtk::Widget = f.bouton_menu().upcast();
    let carnet: gtk::Widget = f.carnet().upcast();
    let ordre = f.elements_du_cycle();
    let pos = |w: &gtk::Widget| ordre.iter().position(|x| x == w);
    let (Some(ic), Some(ib), Some(im)) = (pos(&carnet), pos(&b), pos(&menu)) else {
        return Err(format!(
            "carnet, bouton + ou menu absent du cycle : {ordre:?}"
        ));
    };
    verifier(ic < ib && ib + 1 == im, || {
        format!("ordre : carnet {ic}, bouton {ib}, menu {im}")
    })
}
