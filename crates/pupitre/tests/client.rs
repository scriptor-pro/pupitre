// SPDX-License-Identifier: GPL-3.0-or-later
//! Tests du client : démarrage automatique du processus de fond, partage, perte de connexion.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pupitre::client::{connect_or_spawn_at, Client};
use pupitre_proto::{ClientMsg, ServerMsg, SessionId};
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::Once;
use std::time::{Duration, Instant};

/// Compile pupitre-fond (une fois) et indique son chemin au client.
fn binaire_fond() -> PathBuf {
    static COMPILATION: Once = Once::new();
    let racine = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    COMPILATION.call_once(|| {
        let ok = Command::new(env!("CARGO"))
            .args(["build", "-q", "-p", "pupitre-fond", "--bin", "pupitre-fond"])
            .current_dir(&racine)
            .status()
            .unwrap()
            .success();
        assert!(ok, "compilation de pupitre-fond");
    });
    let b = racine.join("target/debug/pupitre-fond");
    std::env::set_var("PUPITRE_FOND", &b);
    b
}

fn dossier() -> (tempfile::TempDir, PathBuf) {
    binaire_fond();
    let d = tempfile::tempdir().unwrap();
    let socket = d.path().join("pupitre/fond.sock");
    (d, socket)
}

/// Attend un message accepté par `f` sur le canal du client.
fn attendre<T>(
    rx: &async_channel::Receiver<ServerMsg>,
    mut f: impl FnMut(&ServerMsg) -> Option<T>,
) -> T {
    let limite = Instant::now() + Duration::from_secs(10);
    while Instant::now() < limite {
        match rx.try_recv() {
            Ok(m) => {
                if let Some(t) = f(&m) {
                    return t;
                }
            }
            Err(_) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    panic!("message attendu non reçu");
}

fn ouvrir(c: &Client, rx: &async_channel::Receiver<ServerMsg>) -> SessionId {
    c.send(ClientMsg::Open {
        argv: Some(vec!["/bin/sh".into()]),
        cwd: None,
        cols: 80,
        rows: 24,
    })
    .unwrap();
    attendre(rx, |m| {
        if let ServerMsg::Opened { id } = m {
            Some(*id)
        } else {
            None
        }
    })
}

#[test]
fn demarre_le_fond_si_absent() {
    let (_d, socket) = dossier();
    assert!(!socket.exists());
    let (_c, rx) = connect_or_spawn_at(&socket).unwrap();
    attendre(&rx, |m| {
        matches!(m, ServerMsg::Welcome { .. }).then_some(())
    });
    assert!(socket.exists());
}

#[test]
fn deux_clients_voient_les_memes_sessions() {
    let (_d, socket) = dossier();
    let (c1, rx1) = connect_or_spawn_at(&socket).unwrap();
    let id = ouvrir(&c1, &rx1);
    let (c2, rx2) = connect_or_spawn_at(&socket).unwrap();
    c2.send(ClientMsg::List).unwrap();
    let v = attendre(&rx2, |m| {
        if let ServerMsg::Sessions(v) = m {
            Some(v.clone())
        } else {
            None
        }
    });
    assert_eq!(v.iter().map(|s| s.id).collect::<Vec<_>>(), vec![id]);
    // Sortie diffusée aux deux clients.
    c2.send(ClientMsg::Input {
        id,
        data: b"echo par''tage\n".to_vec(),
    })
    .unwrap();
    for rx in [&rx1, &rx2] {
        let mut tout = Vec::new();
        attendre(rx, |m| match m {
            ServerMsg::Output { data, .. } => {
                tout.extend(data);
                String::from_utf8_lossy(&tout)
                    .contains("partage")
                    .then_some(())
            }
            _ => None,
        });
    }
    c1.send(ClientMsg::Close { id }).unwrap();
    attendre(&rx1, |m| {
        matches!(m, ServerMsg::Exited { .. }).then_some(())
    });
}

#[test]
fn perte_de_connexion_signalee() {
    let (_d, socket) = dossier();
    let mut fond: Child = Command::new(binaire_fond())
        .arg("--socket")
        .arg(&socket)
        .spawn()
        .unwrap();
    let (c, rx) = connect_or_spawn_at(&socket).unwrap();
    fond.kill().unwrap();
    fond.wait().unwrap();
    attendre(&rx, |m| {
        matches!(m, ServerMsg::Error(e) if e == "connexion perdue").then_some(())
    });
    // Envoyer après la perte : une erreur, jamais une panique ni un blocage.
    let limite = Instant::now() + Duration::from_secs(2);
    while c.send(ClientMsg::List).is_ok() {
        assert!(Instant::now() < limite, "l'envoi aurait dû échouer");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn fond_introuvable_donne_une_erreur() {
    let (_d, socket) = dossier();
    // Variable propre à ce test : le chemin du binaire est passé explicitement.
    let r = pupitre::client::connect_or_spawn_with(
        &socket,
        Some(PathBuf::from("/nexiste/pupitre-fond")),
    );
    assert!(r.is_err());
}
