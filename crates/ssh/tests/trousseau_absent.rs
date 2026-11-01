// SPDX-License-Identifier: GPL-3.0-or-later
//! Sans bus D-Bus joignable, le trousseau est « absent », vite et sans panique.
//! Fichier à part : il modifie l'environnement du processus, ce qui ne doit
//! pas gêner d'autres tests.

use pupitre_ssh::trousseau::{Coffre, CoffreSecretService, ErreurTrousseau, EtatTrousseau};
use std::path::Path;
use std::time::{Duration, Instant};

#[test]
fn sans_service_absent() {
    // Seul test de ce binaire : personne d'autre ne lit l'environnement en même temps.
    std::env::set_var(
        "DBUS_SESSION_BUS_ADDRESS",
        "unix:path=/nonexistent/pupitre-bus",
    );
    let debut = Instant::now();
    let coffre = CoffreSecretService::ouvrir();
    assert_eq!(coffre.etat(), EtatTrousseau::Absent);
    assert_eq!(
        coffre.lire(Path::new("/tmp/cle")).map(|p| p.is_some()),
        Err(ErreurTrousseau::Absent)
    );
    assert!(coffre.cles().is_empty());
    assert!(matches!(
        coffre.ecrire(Path::new("/tmp/cle"), "x"),
        Err(ErreurTrousseau::Absent)
    ));
    assert!(coffre.oublier(Path::new("/tmp/cle")).is_err());
    assert!(
        debut.elapsed() < Duration::from_secs(2),
        "{:?}",
        debut.elapsed()
    );
}
