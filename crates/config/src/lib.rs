// SPDX-License-Identifier: GPL-3.0-or-later
//! Configuration, couleurs et thèmes de Pupitre.

mod atomique;
mod connexions;
mod couleur;
mod edition;
mod fichier;
mod theme;
mod theme_fichier;

pub use atomique::ecrire_atomique;
pub use connexions::{
    analyser_port, argv_ssh, charger_connexions, chemin_connexions, enregistrer_connexions,
    valider_connexions, ChampConnexion, Connexion, ErreurValidation,
};
pub use couleur::{contrast_ratio, ErreurCouleur, Rgb};
pub use edition::{enregistrer_reglage, enregistrer_reglage_dans, modifier_cle, ErreurEdition};
pub use fichier::{
    charger, chemin_config, config_defaut, creer_defaut_si_absent, dossier_etat, langue_configuree,
    AnnonceFin, BandeauClavier, Config, ErreurConfig, Invite, ReglageRaccourci,
    ReglagesNotifications, ShellDeclare, ThemeChoix, ACTIONS_RACCOURCIS, CONFIG_DEFAUT,
    LANGUES_CONFIG,
};
pub use theme::{check_theme, theme_clair, theme_sombre, ProblemeContraste, Role, Theme};
pub use theme_fichier::charger_theme;
