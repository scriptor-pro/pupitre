// SPDX-License-Identifier: GPL-3.0-or-later
//! Un bus qui accepte la connexion mais ne répond jamais ne bloque pas Pupitre :
//! le délai du trousseau est borné, et il est signalé comme tel (« le trousseau ne
//! répond pas »), jamais comme une entrée ou un trousseau absents (sauf `etat()`, dont
//! le type n'a que trois valeurs). Fichier à part (il modifie l'environnement).

use pupitre_ssh::trousseau::{
    Coffre, CoffreSecretService, ErreurTrousseau, EtatTrousseau, DELAI_TROUSSEAU,
};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::time::{Duration, Instant};

#[test]
fn bus_muet_signale_le_delai_en_temps_borne() {
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
    assert_eq!(
        coffre.lire(cle).map(|p| p.is_some()),
        Err(ErreurTrousseau::Delai)
    );
    assert!(coffre.cles().is_empty());
    assert_eq!(coffre.ecrire(cle, "x"), Err(ErreurTrousseau::Delai));
    assert_eq!(coffre.oublier(cle), Err(ErreurTrousseau::Delai));
}
