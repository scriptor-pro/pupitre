// SPDX-License-Identifier: GPL-3.0-or-later
//! Tests contre un vrai Secret Service. Ils écrivent dans le trousseau : on ne
//! les lance que par `scripts/test-x11.sh` ou `scripts/test-wayland.sh`, qui
//! démarrent un gnome-keyring jetable (`scripts/trousseau-test.sh`).

use pupitre_ssh::trousseau::{Coffre, CoffreSecretService, EtatTrousseau};
use secret_service::blocking::SecretService;
use secret_service::EncryptionType;
use std::collections::HashMap;
use std::path::Path;

/// Garde : ces tests écrivent dans le trousseau, on refuse de les lancer hors
/// du trousseau jetable de `scripts/trousseau-test.sh`.
fn jetable() -> bool {
    let ok = std::env::var_os("PUPITRE_TROUSSEAU_JETABLE").is_some();
    if !ok {
        println!("ignoré : trousseau jetable absent");
    }
    ok
}

#[test]
#[ignore = "demande un Secret Service jetable (scripts/trousseau-test.sh)"]
fn cycle_complet() {
    if !jetable() {
        return;
    }
    let coffre = CoffreSecretService::ouvrir();
    assert_eq!(coffre.etat(), EtatTrousseau::Disponible);
    let cle = Path::new("/tmp/pupitre-test/cle-cycle");
    coffre.ecrire(cle, "phrase é secrète").unwrap();
    assert_eq!(
        coffre.lire(cle).unwrap().as_deref().map(String::as_str),
        Some("phrase é secrète")
    );
    assert!(coffre.cles().contains(&cle.to_path_buf()));
    // Réécrire remplace.
    coffre.ecrire(cle, "autre").unwrap();
    assert_eq!(
        coffre.lire(cle).unwrap().as_deref().map(String::as_str),
        Some("autre")
    );
    coffre.oublier(cle).unwrap();
    assert!(coffre.lire(cle).unwrap().is_none());
    assert!(!coffre.cles().contains(&cle.to_path_buf()));
    // Oublier une clé inconnue n'est pas une erreur.
    coffre.oublier(cle).unwrap();
}

#[test]
#[ignore = "demande un Secret Service jetable (scripts/trousseau-test.sh)"]
fn element_d_une_autre_application_intact() {
    if !jetable() {
        return;
    }
    let coffre = CoffreSecretService::ouvrir();
    let cle = Path::new("/tmp/pupitre-test/cle-etrangere");
    let ss = SecretService::connect(EncryptionType::Dh).unwrap();
    let col = ss.get_default_collection().unwrap();
    assert!(!col.is_locked().unwrap(), "trousseau verrouillé");
    let mut attrs: HashMap<&str, &str> = HashMap::new();
    attrs.insert("application", "autre-programme");
    attrs.insert("type", "phrase-cle-ssh");
    attrs.insert("cle", cle.to_str().unwrap());
    let etranger = col
        .create_item("Étranger", attrs.clone(), b"pas-a-moi", true, "text/plain")
        .unwrap();

    assert!(coffre.lire(cle).unwrap().is_none());
    assert!(!coffre.cles().contains(&cle.to_path_buf()));
    coffre.ecrire(cle, "a-moi").unwrap();
    assert_eq!(
        coffre.lire(cle).unwrap().as_deref().map(String::as_str),
        Some("a-moi")
    );
    coffre.oublier(cle).unwrap();
    assert!(coffre.lire(cle).unwrap().is_none());
    // L'élément étranger existe toujours, avec son secret.
    let restants = col.search_items(attrs).unwrap();
    assert_eq!(restants.len(), 1);
    assert_eq!(restants[0].get_secret().unwrap(), b"pas-a-moi");
    etranger.delete().unwrap();
}
