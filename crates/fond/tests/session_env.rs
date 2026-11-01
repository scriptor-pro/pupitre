// SPDX-License-Identifier: GPL-3.0-or-later
//! Le mode « trousseau » de `pupitre-askpass` n'est jamais hérité par les shells : un seul
//! test, dans son propre binaire (il modifie l'environnement du processus).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pupitre_fond::session::{Session, SessionEvent};
use std::time::{Duration, Instant};

#[test]
fn mode_askpass_retire_des_shells() {
    std::env::set_var("PUPITRE_ASKPASS_MODE", "trousseau");
    let argv: Vec<String> = [
        "/bin/sh",
        "-c",
        "echo \"[${PUPITRE_ASKPASS_MODE-absente}]\"",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let (_s, rx) = Session::open(&argv, None, 80, 24, &[]).unwrap();
    let mut sortie = Vec::new();
    let limite = Instant::now() + Duration::from_secs(10);
    while Instant::now() < limite {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(SessionEvent::Output(d)) => sortie.extend_from_slice(&d),
            Ok(SessionEvent::Exited(_)) => break,
            Err(_) => {}
        }
    }
    let texte = String::from_utf8_lossy(&sortie);
    assert!(texte.contains("[absente]"), "{texte:?}");
}
