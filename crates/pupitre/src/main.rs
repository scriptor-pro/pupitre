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
use pupitre::verification;
use pupitre_config::{charger, chemin_config, creer_defaut_si_absent, Config};
use pupitre_fond::serveur::socket_path;
use std::cell::RefCell;
use std::rc::Rc;

const ID_APPLICATION: &str = "fr.pupitre.Pupitre";

fn main() -> glib::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => {}
        Some("check") => {
            let code = verification::executer(&args[1..], &mut std::io::stdout());
            return glib::ExitCode::from(u8::try_from(code).unwrap_or(1));
        }
        Some("-h" | "--help" | "aide") => {
            print!("{}", verification::USAGE);
            return glib::ExitCode::SUCCESS;
        }
        Some(autre) => {
            eprint!("Argument inconnu « {autre} ».\n{}", verification::USAGE);
            return glib::ExitCode::from(2);
        }
    }
    // Avant toute création de widget : les polices doivent être connues de fontconfig.
    match dossier_polices() {
        Some(d) if enregistrer_polices_embarquees(&d) => {}
        _ => eprintln!("pupitre: polices embarquées introuvables ; police du système utilisée"),
    }
    glib::set_application_name("Pupitre");
    // Icône des fenêtres (X11) : celle du thème installée par scripts/installer-bureau.sh.
    gtk::Window::set_default_icon_name(ID_APPLICATION);
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
                let chemin = chemin_config();
                if let Err(e) = creer_defaut_si_absent(&chemin) {
                    eprintln!("pupitre: impossible de créer {} : {e}", chemin.display());
                }
                let p = Pupitre::demarrer(app, socket_path(), Config::default());
                // Invalide : valeurs par défaut gardées, message avec la ligne fautive.
                p.appliquer_config(charger(&chemin));
                p.surveiller_config(chemin);
                *pupitre.borrow_mut() = Some(p);
            }
        }
    });
    // Arguments déjà traités ci-dessus : GTK n'en reçoit aucun.
    app.run_with_args::<&str>(&[])
}
