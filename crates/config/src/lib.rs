// SPDX-License-Identifier: GPL-3.0-or-later
//! Configuration, couleurs et thèmes de Pupitre.

mod atomique;
mod couleur;
mod fichier;
mod theme;
mod theme_fichier;

pub use atomique::ecrire_atomique;
pub use couleur::{contrast_ratio, ErreurCouleur, Rgb};
pub use fichier::{
    charger, chemin_config, creer_defaut_si_absent, Config, ErreurConfig, ThemeChoix, CONFIG_DEFAUT,
};
pub use theme::{check_theme, theme_clair, theme_sombre, ProblemeContraste, Role, Theme};
pub use theme_fichier::charger_theme;
