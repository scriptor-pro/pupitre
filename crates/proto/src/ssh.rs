// SPDX-License-Identifier: GPL-3.0-or-later
//! Types SSH qui voyagent entre l'interface et le processus de fond.

use serde::{Deserialize, Serialize};

/// État du trousseau du système (Secret Service).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub enum EtatTrousseau {
    /// Trousseau présent et déverrouillé.
    Disponible,
    /// Trousseau présent mais verrouillé.
    Verrouille,
    /// Pas de trousseau utilisable (service absent ou erreur).
    Absent,
}
