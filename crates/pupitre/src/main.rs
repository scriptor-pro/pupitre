// SPDX-License-Identifier: GPL-3.0-or-later
//! Binaire pupitre : la fenêtre GTK. Étape 1 / Task 8 : une fenêtre, un terminal relié à
//! une session du processus de fond (les onglets arrivent avec la Task 9).

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use pupitre::apparence::{suivre_preference_systeme, theme_systeme, Preference};
use pupitre::client::{connect_or_spawn, Client, CONNEXION_PERDUE};
use pupitre::polices::{dossier_polices, enregistrer_polices_embarquees};
use pupitre::taille::{largeur_ecran, taille_ouverture};
use pupitre::terminal::TerminalOnglet;
use pupitre_config::{
    charger, chemin_config, creer_defaut_si_absent, theme_clair, theme_sombre, Config, Theme,
    ThemeChoix,
};
use pupitre_proto::{ClientMsg, ServerMsg};
use std::cell::RefCell;
use std::rc::Rc;

const ID_APPLICATION: &str = "fr.pupitre.Pupitre";
/// Grille annoncée à l'ouverture ; corrigée dès la première allocation de la fenêtre.
const GRILLE_INITIALE: (u16, u16) = (80, 24);

fn main() -> glib::ExitCode {
    // Avant toute création de widget : les polices doivent être connues de fontconfig.
    match dossier_polices() {
        Some(d) if enregistrer_polices_embarquees(&d) => {}
        _ => eprintln!("pupitre: polices embarquées introuvables ; police du système utilisée"),
    }
    glib::set_application_name("Pupitre");
    let app = gtk::Application::builder()
        .application_id(ID_APPLICATION)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(ouvrir_fenetre);
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

/// État partagé de la fenêtre.
struct Etat {
    config: Config,
    theme: RefCell<Theme>,
    onglet: RefCell<Option<TerminalOnglet>>,
}

impl Etat {
    fn changer_theme(&self, theme: Theme) {
        // Les éléments GTK (barre de titre…) suivent le même clair/sombre que le terminal.
        if let Some(r) = gtk::Settings::default() {
            r.set_gtk_application_prefer_dark_theme(theme.nom != "clair");
        }
        if let Some(o) = self.onglet.borrow().as_ref() {
            o.appliquer(&self.config, &theme);
        }
        *self.theme.borrow_mut() = theme;
    }
}

fn ouvrir_fenetre(app: &gtk::Application) {
    let config = lire_config();
    let (largeur, hauteur) = taille_ouverture(largeur_ecran().unwrap_or(0));
    let fenetre = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Pupitre")
        .default_width(largeur)
        .default_height(hauteur)
        .build();

    let theme_initial = match &config.theme {
        ThemeChoix::Sombre | ThemeChoix::Systeme => theme_sombre(),
        ThemeChoix::Clair => theme_clair(),
        ThemeChoix::Fichier(chemin) => {
            // Les thèmes en fichier arrivent avec le rechargement de configuration (Task 10).
            eprintln!(
                "pupitre: thème {} pas encore pris en charge ; thème sombre utilisé",
                chemin.display()
            );
            theme_sombre()
        }
    };
    let suivre = config.theme == ThemeChoix::Systeme;
    let etat = Rc::new(Etat {
        config,
        theme: RefCell::new(theme_initial.clone()),
        onglet: RefCell::new(None),
    });
    etat.changer_theme(theme_initial);
    if suivre {
        let e = etat.clone();
        suivre_preference_systeme(move |p: Preference| e.changer_theme(theme_systeme(p)));
    }

    match connect_or_spawn() {
        Ok((client, recus)) => {
            let client = Rc::new(client);
            let (cols, rows) = GRILLE_INITIALE;
            if let Err(e) = client.send(ClientMsg::Open {
                argv: None,
                cwd: None,
                cols,
                rows,
            }) {
                afficher_erreur(&fenetre, &e.to_string());
            } else {
                recevoir(fenetre.clone(), etat, client, recus);
            }
        }
        Err(e) => afficher_erreur(&fenetre, &e.to_string()),
    }
    fenetre.present();
}

/// Traite les messages du processus de fond sur la boucle GTK.
fn recevoir(
    fenetre: gtk::ApplicationWindow,
    etat: Rc<Etat>,
    client: Rc<Client>,
    recus: async_channel::Receiver<ServerMsg>,
) {
    glib::spawn_future_local(async move {
        while let Ok(m) = recus.recv().await {
            match m {
                ServerMsg::Opened { id } if etat.onglet.borrow().is_none() => {
                    let o =
                        TerminalOnglet::new(client.clone(), id, &etat.config, &etat.theme.borrow());
                    fenetre.set_child(Some(o.widget()));
                    o.widget().grab_focus();
                    *etat.onglet.borrow_mut() = Some(o);
                }
                ServerMsg::Output { id, data } => {
                    if let Some(o) = etat.onglet.borrow().as_ref().filter(|o| o.id() == id) {
                        o.on_output(&data);
                    }
                }
                ServerMsg::Exited { id, code } => {
                    if let Some(o) = etat.onglet.borrow().as_ref().filter(|o| o.id() == id) {
                        let code = code.map_or_else(|| "signal".to_string(), |c| c.to_string());
                        o.on_output(
                            format!("\r\n[Le shell s'est terminé (code {code})]\r\n").as_bytes(),
                        );
                    }
                }
                ServerMsg::OpenFailed { error } => {
                    afficher_erreur(
                        &fenetre,
                        &format!("le shell n'a pas pu être lancé : {error}"),
                    );
                }
                ServerMsg::Error(e) if e == CONNEXION_PERDUE => {
                    let message = "\r\n[Connexion au processus de fond perdue]\r\n";
                    match etat.onglet.borrow().as_ref() {
                        Some(o) => o.on_output(message.as_bytes()),
                        None => afficher_erreur(&fenetre, message.trim()),
                    }
                    break;
                }
                ServerMsg::Error(e) => eprintln!("pupitre: processus de fond : {e}"),
                _ => {}
            }
        }
    });
}

/// Remplace le contenu de la fenêtre par un message d'erreur lisible (et lu par Orca).
fn afficher_erreur(fenetre: &gtk::ApplicationWindow, message: &str) {
    eprintln!("pupitre: {message}");
    let etiquette = gtk::Label::new(Some(&format!("Pupitre ne peut pas démarrer : {message}")));
    etiquette.set_wrap(true);
    etiquette.set_selectable(true);
    etiquette.set_margin_start(24);
    etiquette.set_margin_end(24);
    fenetre.set_child(Some(&etiquette));
}
