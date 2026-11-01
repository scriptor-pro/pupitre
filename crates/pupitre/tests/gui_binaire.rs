// SPDX-License-Identifier: GPL-3.0-or-later
//! Le vrai programme `pupitre` démarre et tient debout. Les autres tests graphiques
//! construisent l'interface dans leur propre processus et ne passent jamais par `main` :
//! une panique au démarrage (appel GTK avant l'initialisation) leur échappait.
//! Lancé par `scripts/test-x11.sh` et `scripts/test-wayland.sh` ; ignoré sinon.
//!
//! Dossiers personnels et d'exécution temporaires ; le fond est celui des tests (sans
//! accès au trousseau). À la fin, les sessions sont fermées pour que le fond s'arrête.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod commun;

use commun::*;
use pupitre_proto::{read_frame, write_frame, ClientMsg, ServerMsg, VERSION};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

fn main() {
    if !actives() {
        return;
    }
    bus_prive();
    compiler();
    let t = tempfile::tempdir().unwrap();
    let runtime = t.path().join("run");
    std::fs::create_dir(&runtime).unwrap();
    std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = runtime.join("pupitre").join("fond.sock");

    let mut commande = Command::new(racine().join("target/debug/pupitre"));
    // Le socket Wayland vit dans le vrai XDG_RUNTIME_DIR : son chemin complet est donné,
    // puisque XDG_RUNTIME_DIR change ci-dessous.
    if let (Some(rt), Some(wd)) = (
        std::env::var_os("XDG_RUNTIME_DIR"),
        std::env::var_os("WAYLAND_DISPLAY"),
    ) {
        commande.env("WAYLAND_DISPLAY", Path::new(&rt).join(wd));
    }
    let mut pupitre = commande
        .env("HOME", t.path())
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("XDG_CONFIG_HOME", t.path().join("config"))
        .env("XDG_CACHE_HOME", t.path().join("cache"))
        .env("XDG_DATA_HOME", t.path().join("data"))
        .env("TMPDIR", t.path())
        .env("SHELL", "/bin/sh")
        .env_remove("SSH_AUTH_SOCK")
        .spawn()
        .expect("lancement de pupitre");

    // Le programme doit encore tourner après quelques secondes, et avoir joint son fond.
    let limite = Instant::now() + Duration::from_secs(5);
    while Instant::now() < limite {
        if let Some(statut) = pupitre.try_wait().unwrap() {
            fermer_sessions(&socket);
            panic!("pupitre s'est arrêté au démarrage : {statut}");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let fond_joint = socket.exists();
    let _ = pupitre.kill();
    let _ = pupitre.wait();
    fermer_sessions(&socket);
    assert!(fond_joint, "pupitre n'a pas lancé son processus de fond");
    println!("test binaire_demarre ... ok");
}

/// Ferme toutes les sessions du fond, puis attend qu'il s'arrête (socket retiré).
fn fermer_sessions(socket: &Path) {
    let Ok(mut flux) = UnixStream::connect(socket) else {
        return;
    };
    flux.set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    write_frame(&mut flux, &ClientMsg::Hello { version: VERSION }).unwrap();
    write_frame(&mut flux, &ClientMsg::List).unwrap();
    let sessions = loop {
        match read_frame::<_, ServerMsg>(&mut flux) {
            Ok(ServerMsg::Sessions(s)) => break s,
            Ok(_) => {}
            Err(_) => return,
        }
    };
    for s in sessions {
        let _ = write_frame(&mut flux, &ClientMsg::Close { id: s.id });
    }
    drop(flux);
    let limite = Instant::now() + Duration::from_secs(30);
    while socket.exists() && Instant::now() < limite {
        std::thread::sleep(Duration::from_millis(200));
    }
    assert!(!socket.exists(), "le processus de fond ne s'est pas arrêté");
}
