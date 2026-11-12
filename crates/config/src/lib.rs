// SPDX-License-Identifier: GPL-3.0-or-later
//! Configuration, couleurs et thèmes de Pupitre.

mod atomique;
mod connexions;
mod couleur;
mod fichier;
mod theme;
mod theme_fichier;

pub use atomique::ecrire_atomique;
pub use connexions::{
    analyser_port, argv_ssh, charger_connexions, chemin_connexions, enregistrer_connexions,
    valider_connexions, ChampConnexion, Connexion, ErreurValidation,
};
pub use couleur::{contrast_ratio, ErreurCouleur, Rgb};
pub use fichier::{
    charger, chemin_config, config_defaut, creer_defaut_si_absent, dossier_etat, langue_configuree,
    AnnonceFin, Config, ErreurConfig, Invite, ShellDeclare, ThemeChoix, CONFIG_DEFAUT,
    LANGUES_CONFIG,
};
pub use theme::{check_theme, theme_clair, theme_sombre, ProblemeContraste, Role, Theme};
pub use theme_fichier::charger_theme;
