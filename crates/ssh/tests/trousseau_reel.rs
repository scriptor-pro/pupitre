// SPDX-License-Identifier: GPL-3.0-or-later
//! Tests contre un vrai Secret Service. Ils écrivent dans le trousseau : on ne
//! les lance que par `scripts/test-x11.sh` ou `scripts/test-wayland.sh`, qui
//! démarrent un gnome-keyring jetable (`scripts/trousseau-test.sh`).

use pupitre_ssh::trousseau::{Coffre, CoffreSecretService, EtatTrousseau};
use secret_service::blocking::SecretService;
use secret_service::EncryptionType;
use std::collections::HashMap;
use std::path::Path;

/// Vrai seulement si `marque` (valeur de `PUPITRE_TROUSSEAU_JETABLE`, posée par
/// `scripts/trousseau-test.sh`) est le PID du démon qui possède `org.freedesktop.secrets`
/// sur le bus courant : une variable égarée (shell lancé sous le script, ancienne valeur
/// « 1 ») ne suffit jamais à approcher un trousseau.
fn trousseau_jetable_verifie(marque: Option<String>) -> bool {
    let Some(pid) = marque.filter(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
    else {
        return false;
    };
    std::process::Command::new("busctl")
        .args(["--user", "status", "org.freedesktop.secrets"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .is_some_and(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .any(|l| l.strip_prefix("PID=") == Some(pid.as_str()))
        })
}

/// Garde : ces tests écrivent dans le trousseau, on refuse de les lancer hors
/// du trousseau jetable de `scripts/trousseau-test.sh`, vérifié par son PID.
fn jetable() -> bool {
    let ok = trousseau_jetable_verifie(std::env::var("PUPITRE_TROUSSEAU_JETABLE").ok());
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
