// SPDX-License-Identifier: GPL-3.0-or-later
//! Shells de Pupitre : détection, niveau de prise en charge, installation, lancement. Sans GTK.

mod detection;
mod installation;
mod lancement;

pub use detection::{
    niveau_de, shells_installes, Niveau, ShellConnu, ShellInstalle, SHELLS_CONNUS,
};
pub use installation::{commande_installation, distribution, Distribution};
pub use lancement::{lancement, Lancement, ModeInvite, Ressources};
