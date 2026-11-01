// SPDX-License-Identifier: GPL-3.0-or-later
//! Un bus qui accepte la connexion mais ne répond jamais ne bloque pas Pupitre :
//! le délai du trousseau est borné. Fichier à part (il modifie l'environnement).

use pupitre_ssh::trousseau::{
    Coffre, CoffreSecretService, ErreurTrousseau, EtatTrousseau, DELAI_TROUSSEAU,
};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::time::{Duration, Instant};

#[test]
fn bus_muet_donne_absent_en_temps_borne() {
    let dossier = tempfile::tempdir().unwrap();
    let socket = dossier.path().join("bus");
    let ecoute = UnixListener::bind(&socket).unwrap();
    // Accepte et garde les connexions ouvertes sans jamais répondre.
    let _garde = std::thread::spawn(move || {
        let mut ouvertes = Vec::new();
        for flux in ecoute.incoming().flatten() {
            ouvertes.push(flux);
        }
    });
    std::env::set_var(
        "DBUS_SESSION_BUS_ADDRESS",
        format!("unix:path={}", socket.display()),
    );
    let coffre = CoffreSecretService::ouvrir();
    let cle = Path::new("/tmp/cle");
    let debut = Instant::now();
    assert_eq!(coffre.etat(), EtatTrousseau::Absent);
    assert!(
        debut.elapsed() < DELAI_TROUSSEAU + Duration::from_secs(1),
        "{:?}",
        debut.elapsed()
    );
    assert!(coffre.lire(cle).is_none());
    assert!(coffre.cles().is_empty());
    assert_eq!(coffre.ecrire(cle, "x"), Err(ErreurTrousseau::Absent));
    assert_eq!(coffre.oublier(cle), Err(ErreurTrousseau::Absent));
}
