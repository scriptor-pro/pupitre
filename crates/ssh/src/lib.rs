// SPDX-License-Identifier: GPL-3.0-or-later
//! Support SSH de Pupitre : recherche et lancement de l'agent, puis (plus
//! tard) trousseau, analyse des invites OpenSSH et lecture de `~/.ssh/config`.

pub mod agent;
pub mod invite;
pub mod trousseau;
