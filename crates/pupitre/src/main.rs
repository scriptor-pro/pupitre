// SPDX-License-Identifier: GPL-3.0-or-later
//! Binaire pupitre : l'interface GTK.
//!
//! L'application est unique par session de bureau : un second lancement ouvre une nouvelle
//! fenêtre dans l'interface déjà lancée. Au premier lancement, les sessions déjà présentes
//! dans le processus de fond sont donc celles d'une interface qui a planté (reprise).

use gtk::glib;
use gtk::prelude::*;
use pupitre::fenetre::Pupitre;
use pupitre::polices::{dossier_polices, enregistrer_polices_embarquees};
use pupitre_config::{charger, chemin_config, creer_defaut_si_absent, Config};
use pupitre_fond::serveur::socket_path;
use std::cell::RefCell;
use std::rc::Rc;

const ID_APPLICATION: &str = "fr.pupitre.Pupitre";

fn main() -> glib::ExitCode {
    // Avant toute création de widget : les polices doivent être connues de fontconfig.
    match dossier_polices() {
        Some(d) if enregistrer_polices_embarquees(&d) => {}
        _ => eprintln!("pupitre: polices embarquées introuvables ; police du système utilisée"),
    }
    glib::set_application_name("Pupitre");
    let app = gtk::Application::builder()
        .application_id(ID_APPLICATION)
        .build();
    let pupitre: Rc<RefCell<Option<Rc<Pupitre>>>> = Rc::new(RefCell::new(None));
    app.connect_activate(move |app| {
        let existant = pupitre.borrow().clone();
        match existant {
            Some(p) => {
                p.nouvelle_fenetre();
            }
            None => {
                *pupitre.borrow_mut() = Some(Pupitre::demarrer(app, socket_path(), lire_config()));
            }
        }
    });
    // Les arguments seront traités plus tard (`pupitre check`, Task 10).
    app.run_with_args::<&str>(&[])
}

/// Configuration de l'utilisateur ; en cas de problème, valeurs par défaut + message.
fn lire_config() -> Config {
    let chemin = chemin_config();
    if let Err(e) = creer_defaut_si_absent(&chemin) {
        eprintln!("pupitre: impossible de créer {} : {e}", chemin.display());
    }
    match charger(&chemin) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "pupitre: {} : {e} ; valeurs par défaut utilisées",
                chemin.display()
            );
            Config::default()
        }
    }
}
