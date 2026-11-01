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
use pupitre::shells::chemin_starship;
use pupitre::verification;
use pupitre_config::{
    charger, chemin_config, creer_defaut_si_absent, langue_configuree, Config, Invite,
};
use pupitre_fond::serveur::socket_path;
use pupitre_i18n::{init, langues_demandees, ta, ArgI18n, LanguageIdentifier};
use pupitre_shell::creer_config_starship_si_absente;
use std::cell::RefCell;
use std::rc::Rc;

const ID_APPLICATION: &str = "fr.pupitre.Pupitre";

/// Langues demandées : le réglage `langue` de `pupitre.toml` s'il est renseigné, sinon celles
/// du système. Lecture tolérante : un fichier absent ou invalide donne la langue du système.
fn langues_choisies() -> Vec<LanguageIdentifier> {
    if let Some(l) = langue_configuree(&chemin_config()) {
        if let Ok(id) = l.parse() {
            return vec![id];
        }
    }
    langues_demandees(&|nom| std::env::var(nom).ok())
}

fn main() -> glib::ExitCode {
    init(&langues_choisies());
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => {}
        Some("check") => {
            let code = verification::executer(&args[1..], &mut std::io::stdout());
            return glib::ExitCode::from(u8::try_from(code).unwrap_or(1));
        }
        Some("shells") if args.len() == 1 => {
            let code = pupitre::shells::executer_shells(
                &pupitre::shells::path_courant(),
                &mut std::io::stdout(),
            );
            return glib::ExitCode::from(u8::try_from(code).unwrap_or(1));
        }
        Some("-h" | "--help" | "aide" | "help") => {
            print!("{}", verification::usage());
            return glib::ExitCode::SUCCESS;
        }
        Some(autre) => {
            eprint!(
                "{}\n{}",
                ta("argument-inconnu", &[("argument", ArgI18n::from(autre))]),
                verification::usage()
            );
            return glib::ExitCode::from(2);
        }
    }
    // Avant toute création de widget : les polices doivent être connues de fontconfig.
    match dossier_polices() {
        Some(d) if enregistrer_polices_embarquees(&d) => {}
        _ => eprintln!("pupitre: polices embarquées introuvables ; police du système utilisée"),
    }
    glib::set_application_name(&pupitre_i18n::t("fenetre-nom"));
    let app = gtk::Application::builder()
        .application_id(ID_APPLICATION)
        .build();
    // Icône des fenêtres (X11) : celle du thème installée par scripts/installer-bureau.sh.
    // Au démarrage de l'application seulement : avant, GTK n'est pas initialisé (panique).
    app.connect_startup(|_| gtk::Window::set_default_icon_name(ID_APPLICATION));
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
                let config = charger(&chemin);
                // Invite Starship de Pupitre (réglage par défaut) : son fichier est créé s'il
                // manque. En cas d'échec, l'invite du shell est gardée (voir `shells`).
                if config
                    .as_ref()
                    .map_or(true, |c| c.invite == Invite::Starship)
                {
                    let invite = chemin_starship();
                    if let Err(e) = creer_config_starship_si_absente(&invite) {
                        eprintln!(
                            "pupitre: {}",
                            ta(
                                "shell-starship-config-impossible",
                                &[
                                    ("chemin", ArgI18n::from(invite.display().to_string())),
                                    ("erreur", ArgI18n::from(e.to_string())),
                                ],
                            )
                        );
                    }
                }
                let p = Pupitre::demarrer(app, socket_path(), Config::default());
                // Invalide : valeurs par défaut gardées, message avec la ligne fautive.
                p.appliquer_config(config);
                p.surveiller_config(chemin);
                *pupitre.borrow_mut() = Some(p);
            }
        }
    });
    // Arguments déjà traités ci-dessus : GTK n'en reçoit aucun.
    app.run_with_args::<&str>(&[])
}
