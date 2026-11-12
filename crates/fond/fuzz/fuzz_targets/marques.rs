// SPDX-License-Identifier: GPL-3.0-or-later
//! Fuzzing : quelles que soient les entrées et leur découpage en morceaux, le lecteur de
//! séquences OSC (133, 52, 9 et 777) ne panique pas, rend exactement les octets reçus, ne
//! garde pas plus de mémoire que sa borne et ne rend que des textes bornés et sans contrôles.
#![no_main]

use libfuzzer_sys::fuzz_target;
use std::time::Instant;

// Le module testé est le fichier du fond lui-même (il ne dépend que de `pupitre-proto`).
#[path = "../../src/marques.rs"]
#[allow(dead_code)]
mod marques;
use marques::{LecteurMarques, Morceau, NOTIFICATION_MAX, PRESSE_PAPIERS_MAX};

fuzz_target!(|donnees: &[u8]| {
    // Le premier octet donne la taille de base des morceaux ; les suivants la font varier.
    let Some((&graine, entree)) = donnees.split_first() else {
        return;
    };
    let mut lecteur = LecteurMarques::new();
    let maintenant = Instant::now();
    let mut rendu: Vec<u8> = Vec::with_capacity(entree.len());
    let mut reste = entree;
    let mut pas = usize::from(graine) % 17 + 1;
    while !reste.is_empty() {
        let n = pas.min(reste.len());
        let (morceau, suite) = reste.split_at(n);
        for m in lecteur.lire(morceau, maintenant) {
            match m {
                Morceau::Octets(o) => rendu.extend(o),
                Morceau::Marque(_) => {}
                Morceau::PressePapiers(t) => assert!(t.len() <= PRESSE_PAPIERS_MAX),
                Morceau::Notification { titre, corps } => {
                    for t in titre.iter().chain(Some(&corps)) {
                        assert!(t.chars().count() <= NOTIFICATION_MAX + 1);
                        assert!(!t.chars().any(char::is_control));
                    }
                }
            }
        }
        assert!(lecteur.memoire() <= 2 * PRESSE_PAPIERS_MAX);
        reste = suite;
        pas = (pas * 5 + usize::from(morceau.first().copied().unwrap_or(0))) % 4200 + 1;
    }
    assert_eq!(rendu, entree, "les octets relayés doivent être l'entrée");
});
