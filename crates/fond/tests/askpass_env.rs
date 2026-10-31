// SPDX-License-Identifier: GPL-3.0-or-later
//! `chemin_askpass` lit l'environnement : un seul test, dans son propre binaire.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pupitre_fond::ssh::chemin_askpass;

#[test]
fn chemin_askpass_selon_l_environnement() {
    std::env::set_var("PUPITRE_ASKPASS", "/bin/true");
    assert_eq!(chemin_askpass(), Some("/bin/true".into()));
    std::env::set_var("PUPITRE_ASKPASS", "/nonexistent/askpass");
    assert_eq!(chemin_askpass(), None);
    std::env::set_var("PUPITRE_ASKPASS", "");
    // Vide : repli à côté de l'exécutable de test, où il n'y en a pas.
    assert_eq!(chemin_askpass(), None);
}
