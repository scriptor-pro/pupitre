// SPDX-License-Identifier: GPL-3.0-or-later
//! « À propos de Pupitre » : version, site, licence et polices embarquées. Une seule fenêtre :
//! la rouvrir la ramène au premier plan. Échap la ferme.

use crate::polices::POLICES_EMBARQUEES;
use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use pupitre_i18n::{t, ta, ArgI18n};
use std::cell::RefCell;

/// Version de Pupitre (celle du paquet).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Site du projet (`repository` du `Cargo.toml`).
pub const DEPOT: &str = env!("CARGO_PKG_REPOSITORY");

thread_local! {
    static OUVERTE: RefCell<glib::WeakRef<gtk::AboutDialog>> =
        RefCell::new(glib::WeakRef::new());
}

/// Ouvre « À propos de Pupitre » devant `parent` ; si elle est déjà ouverte, la ramène au
/// premier plan.
pub fn ouvrir_apropos(parent: &gtk::Window) {
    // La fenêtre est sortie du `RefCell` avant d'être présentée : aucun emprunt pendant un
    // rappel ré-entrant.
    let existante = OUVERTE.with(|o| o.borrow().upgrade());
    if let Some(d) = existante.filter(|d| d.is_visible()) {
        d.present();
        return;
    }
    let d = gtk::AboutDialog::new();
    d.set_program_name(Some(&t("fenetre-nom")));
    // Après le nom du programme, qui donnerait sinon son propre titre.
    d.set_title(Some(&t("apropos-titre")));
    d.set_version(Some(VERSION));
    d.set_comments(Some(&t("apropos-commentaire")));
    d.set_website(Some(DEPOT));
    d.set_website_label(DEPOT);
    d.set_license_type(gtk::License::Gpl30);
    d.set_logo_icon_name(Some("fr.pupitre.Pupitre"));
    d.set_transient_for(Some(parent));
    d.set_destroy_with_parent(true);
    if let Some(app) = parent.application() {
        d.set_application(Some(&app));
    }
    let polices: Vec<String> = POLICES_EMBARQUEES
        .iter()
        .map(|p| {
            ta(
                "apropos-licence-polices",
                &[("famille", ArgI18n::from(p.famille))],
            )
        })
        .collect();
    let refs: Vec<&str> = polices.iter().map(String::as_str).collect();
    d.add_credit_section(&t("apropos-polices"), &refs);
    // Échap ferme (explicite pour ne pas dépendre du comportement par défaut de GTK).
    let clavier = gtk::EventControllerKey::new();
    let faible = d.downgrade();
    clavier.connect_key_pressed(move |_, touche, _, mods| {
        if touche == gdk::Key::Escape && mods.is_empty() {
            if let Some(d) = faible.upgrade() {
                d.close();
            }
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    d.add_controller(clavier);
    OUVERTE.with(|o| o.borrow().set(Some(&d)));
    d.present();
}
