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

/// Nature d'une invite posée par OpenSSH (ou git) à travers `SSH_ASKPASS`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Genre {
    /// Phrase de passe d'une clé ; `nouvel_essai` vrai après une phrase refusée.
    PhraseCle {
        /// Chemin absolu de la clé.
        cle: std::path::PathBuf,
        /// Vrai si la saisie précédente a été refusée.
        nouvel_essai: bool,
    },
    /// Question sur une empreinte d'hôte inconnu (oui/non).
    HoteInconnu,
    /// Demande de confirmation (`SSH_ASKPASS_PROMPT=confirm`).
    Confirmation,
    /// Simple information (`SSH_ASKPASS_PROMPT=none`).
    Information,
    /// Saisie libre ; `masque` vrai si elle ne doit pas s'afficher.
    Texte {
        /// Vrai pour un secret (mot de passe).
        masque: bool,
    },
}
