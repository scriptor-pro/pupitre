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

/// Environnement commun des tests : ni l'agent SSH réel, ni le dossier d'exécution ou le
/// `/tmp` de l'utilisateur ne sont visibles du processus de fond (posé une seule fois).
fn isoler_ssh() -> &'static std::path::Path {
    use std::os::unix::fs::PermissionsExt;
    static BASE: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    BASE.get_or_init(|| {
        let b = tempfile::tempdir().unwrap();
        for v in ["SSH_AUTH_SOCK", "SSH_ASKPASS", "SSH_ASKPASS_REQUIRE"] {
            std::env::remove_var(v);
        }
        let (run, tmp) = (b.path().join("run"), b.path().join("tmp"));
        std::fs::create_dir_all(&run).unwrap();
        std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("XDG_RUNTIME_DIR", &run);
        std::env::set_var("TMPDIR", &tmp);
        // Le fond charge les clés mémorisées du trousseau : hors du trousseau jetable des
        // scripts de test, il ne doit voir aucun bus (donc jamais le vrai trousseau).
        if std::env::var_os("PUPITRE_TROUSSEAU_JETABLE").is_none() {
            std::env::set_var(
                "DBUS_SESSION_BUS_ADDRESS",
                "unix:path=/nonexistent/pupitre-test",
            );
        }
        b
    })
    .path()
}

fn dossier() -> (tempfile::TempDir, PathBuf) {
    binaire_fond();
    isoler_ssh();
    // Garde : le fond de ces tests ne voit jamais le trousseau réel de l'utilisateur.
    assert!(
        std::env::var_os("PUPITRE_TROUSSEAU_JETABLE").is_some()
            || std::env::var("DBUS_SESSION_BUS_ADDRESS").is_ok_and(|a| a.contains("/nonexistent/")),
        "bus D-Bus réel visible par le fond des tests"
    );
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
    // Attendre que CE processus de fond écoute ; sinon le client en lancerait un second
    // et la mort du premier ne couperait pas la connexion.
    let limite = Instant::now() + Duration::from_secs(10);
    while std::os::unix::net::UnixStream::connect(&socket).is_err() {
        assert!(Instant::now() < limite, "le processus de fond n'écoute pas");
        std::thread::sleep(Duration::from_millis(20));
    }
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

#[test]
fn client_v2_negocie_2() {
    let (_d, socket) = dossier();
    let (c, rx) = connect_or_spawn_at(&socket).unwrap();
    assert_eq!(c.version(), 2);
    let v = attendre(&rx, |m| match m {
        ServerMsg::Welcome { version } => Some(*version),
        _ => None,
    });
    assert_eq!(v, 2);
}

/// Faux processus de fond de la version 1 : refuse `Hello { 2 }` avec le texte exact du
/// vrai serveur puis ferme ; accepte `Hello { 1 }` à la connexion suivante.
#[test]
fn repli_v1_face_a_un_fond_ancien() {
    use pupitre_proto::{read_frame, write_frame};
    use std::os::unix::fs::DirBuilderExt;
    use std::os::unix::net::UnixListener;

    let d = tempfile::tempdir().unwrap();
    let dossier = d.path().join("pupitre");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&dossier)
        .unwrap();
    let socket = dossier.join("fond.sock");
    let ecoute = UnixListener::bind(&socket).unwrap();
    let serveur = std::thread::spawn(move || {
        let mut vus = Vec::new();
        for _ in 0..2 {
            let (mut f, _) = ecoute.accept().unwrap();
            let ClientMsg::Hello { version } = read_frame(&mut f).unwrap() else {
                panic!("Hello attendu");
            };
            vus.push(version);
            if version == 1 {
                write_frame(&mut f, &ServerMsg::Welcome { version: 1 }).unwrap();
                // Garde la connexion ouverte jusqu'à la fin du client.
                let _ = read_frame::<_, ClientMsg>(&mut f);
            } else {
                write_frame(
                    &mut f,
                    &ServerMsg::Error(format!(
                        "version du protocole incompatible : interface {version}, processus de fond 1"
                    )),
                )
                .unwrap();
            }
        }
        vus
    });
    let (c, rx) = pupitre::client::connect_or_spawn_with(&socket, None).unwrap();
    assert_eq!(c.version(), 1);
    attendre(&rx, |m| {
        matches!(m, ServerMsg::Welcome { version: 1 }).then_some(())
    });
    drop(c);
    assert_eq!(serveur.join().unwrap(), vec![2, 1]);
}

#[test]
fn shell_du_fond_voit_un_agent_isole() {
    let (d, socket) = dossier();
    let (c, rx) = connect_or_spawn_at(&socket).unwrap();
    c.send(ClientMsg::Open {
        argv: Some(vec![
            "/bin/sh".into(),
            "-c".into(),
            "printf 'VAL=%s\\n' \"$SSH_AUTH_SOCK\"; sleep 1".into(),
        ]),
        cwd: None,
        cols: 80,
        rows: 24,
    })
    .unwrap();
    let mut tout = Vec::new();
    let val = attendre(&rx, |m| match m {
        ServerMsg::Output { data, .. } => {
            tout.extend(data);
            let t = String::from_utf8_lossy(&tout).into_owned();
            let debut = t.find("VAL=")? + 4;
            let fin = t[debut..].find('\n')?;
            Some(t[debut..debut + fin].trim_end().to_string())
        }
        _ => None,
    });
    // Jamais l'agent réel : vide (pas de ssh-agent) ou propre à ce test.
    assert!(
        val.is_empty() || val.starts_with(d.path().to_str().unwrap()),
        "SSH_AUTH_SOCK inattendu : {val}"
    );
}
