// SPDX-License-Identifier: GPL-3.0-or-later
//! Fuzzing : des octets quelconques reçus sur le socket ne doivent jamais faire paniquer
//! la lecture des trames, ni d'un côté ni de l'autre.
#![no_main]

use libfuzzer_sys::fuzz_target;
use pupitre_proto::{read_frame, ClientMsg, ServerMsg};
use std::io::Cursor;

fuzz_target!(|donnees: &[u8]| {
    // Lit toutes les trames successives jusqu'à la première erreur.
    let mut r = Cursor::new(donnees);
    while read_frame::<_, ClientMsg>(&mut r).is_ok() {}
    let mut r = Cursor::new(donnees);
    while read_frame::<_, ServerMsg>(&mut r).is_ok() {}
});
