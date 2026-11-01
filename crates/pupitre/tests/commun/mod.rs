// SPDX-License-Identifier: GPL-3.0-or-later
//! Outils partagés par les tests graphiques (`harness = false`).
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use gtk::glib;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

pub type Resultat = Result<(), String>;

/// Vrai si les tests graphiques doivent tourner (option donnée par les scripts de test).
pub fn actives() -> bool {
    if std::env::args().any(|a| a == "--include-ignored") {
        return true;
    }
    println!("tests graphiques ignorés (lancer scripts/test-x11.sh ou scripts/test-wayland.sh)");
    false
}

/// Relance ce programme de test dans son propre bus D-Bus de session (services activés,
/// noms réservés et bus d'accessibilité ne se mélangent pas d'un programme de test à
/// l'autre). Ne revient que dans le processus relancé.
pub fn bus_prive() {
    const MARQUE: &str = "PUPITRE_TEST_BUS_PRIVE";
    if std::env::var_os(MARQUE).is_some() {
        return;
    }
    let exe = std::env::current_exe().unwrap();
    let statut = Command::new("dbus-run-session")
        .arg("--")
        .arg(exe)
        .args(std::env::args_os().skip(1))
        .env(MARQUE, "1")
        // Le trousseau jetable des scripts vit sur l'ancien bus, pas sur ce bus privé :
        // sans ce retrait, le fond activerait un autre trousseau sur le bus privé.
        .env_remove("PUPITRE_TROUSSEAU_JETABLE")
        .status()
        .expect("dbus-run-session (paquet dbus)");
    std::process::exit(statut.code().unwrap_or(1));
}

pub fn racine() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Compile `pupitre`, `pupitre-fond` et `pupitre-askpass`, et indique au client où trouver `pupitre-fond`.
pub fn compiler() {
    let ok = Command::new(env!("CARGO"))
        .args([
            "build",
            "-q",
            "-p",
            "pupitre-fond",
            "-p",
            "pupitre-ssh",
            "-p",
            "pupitre",
            "--bins",
        ])
        .current_dir(racine())
        .status()
        .unwrap()
        .success();
    assert!(ok, "compilation des binaires");
    std::env::set_var("PUPITRE_FOND", fond_sans_trousseau());
}

/// Chemin du `pupitre-fond` à lancer pour les tests. Le fond charge les clés mémorisées du
/// trousseau, mais le processus de test a besoin de son bus D-Bus (GTK, accessibilité) :
/// hors du trousseau jetable des scripts, on passe par un petit script qui retire le bus
/// au fond seul, pour qu'il ne puisse jamais approcher le vrai trousseau.
pub fn fond_sans_trousseau() -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let reel = racine().join("target/debug/pupitre-fond");
    if std::env::var_os("PUPITRE_TROUSSEAU_JETABLE").is_some() {
        return reel;
    }
    static DOSSIER: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    let d = DOSSIER.get_or_init(|| tempfile::tempdir().unwrap());
    let script = d.path().join("pupitre-fond");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nexport DBUS_SESSION_BUS_ADDRESS=unix:path=/nonexistent/pupitre-test\nexec '{}' \"$@\"\n",
            reel.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    script
}

/// Dossier personnel et dossiers XDG vides : rien de l'utilisateur n'est lu ni écrit.
pub fn isoler_dossiers() -> tempfile::TempDir {
    let perso = tempfile::tempdir().unwrap();
    for (var, sous) in [
        ("HOME", ""),
        ("XDG_DATA_HOME", ".local/share"),
        ("XDG_CONFIG_HOME", ".config"),
        ("XDG_CACHE_HOME", ".cache"),
        ("XDG_STATE_HOME", ".local/state"),
    ] {
        let d = perso.path().join(sous);
        std::fs::create_dir_all(&d).unwrap();
        std::env::set_var(var, &d);
    }
    isoler_ssh(perso.path());
    perso
}
/// Isole de l'agent SSH réel et du `/tmp` de l'utilisateur tout processus de fond lancé
/// ensuite : plus de `SSH_AUTH_SOCK` hérité, `XDG_RUNTIME_DIR` (0700) et `TMPDIR` propres
/// à `base`. Le socket Wayland, relatif à l'ancien `XDG_RUNTIME_DIR`, est rendu absolu.
pub fn isoler_ssh(base: &Path) {
    use std::os::unix::fs::PermissionsExt;
    for v in ["SSH_AUTH_SOCK", "SSH_ASKPASS", "SSH_ASKPASS_REQUIRE"] {
        std::env::remove_var(v);
    }
    if let (Some(rt), Some(wd)) = (
        std::env::var_os("XDG_RUNTIME_DIR"),
        std::env::var_os("WAYLAND_DISPLAY"),
    ) {
        let wd = PathBuf::from(wd);
        if wd.is_relative() {
            std::env::set_var("WAYLAND_DISPLAY", PathBuf::from(rt).join(wd));
        }
    }
    let run = base.join("run");
    let tmp = base.join("tmp");
    std::fs::create_dir_all(&run).unwrap();
    std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::create_dir_all(&tmp).unwrap();
    std::env::set_var("XDG_RUNTIME_DIR", &run);
    std::env::set_var("TMPDIR", &tmp);
}

/// Fait tourner la boucle GTK jusqu'à ce que `cond` soit vraie (ou délai écoulé).
pub fn attendre(delai: Duration, mut cond: impl FnMut() -> bool) -> bool {
    let ctx = glib::MainContext::default();
    let limite = Instant::now() + delai;
    while Instant::now() < limite {
        while ctx.iteration(false) {}
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

/// Laisse tourner la boucle GTK pendant `d`.
pub fn patienter(d: Duration) {
    attendre(d, || false);
}

/// Chemin de la sonde AT-SPI (`tests/a11y/sonde.py`, à la racine du dépôt).
pub fn sonde_a11y() -> PathBuf {
    racine().join("tests/a11y/sonde.py")
}

/// Lance la sonde AT-SPI sur l'application `nom` sans bloquer la boucle GTK (l'application
/// sondée peut être ce processus même). Renvoie l'arbre « rôle\tnom\ttexte\tétats ».
pub fn sonder(nom: &str) -> Option<String> {
    let mut enfant = Command::new("python3")
        .arg(sonde_a11y())
        .arg("--app")
        .arg(nom)
        .stdout(std::process::Stdio::piped())
        .spawn()
        .ok()?;
    let mut fini = false;
    attendre(Duration::from_secs(20), || {
        fini = matches!(enfant.try_wait(), Ok(Some(_)));
        fini
    });
    if !fini {
        let _ = enfant.kill();
    }
    let sortie = enfant.wait_with_output().ok()?;
    sortie
        .status
        .success()
        .then(|| String::from_utf8_lossy(&sortie.stdout).into_owned())
}

/// Sonde jusqu'à ce que `cond(arbre)` soit vraie ; renvoie le dernier arbre lu.
pub fn sonder_jusqu_a(nom: &str, delai: Duration, cond: impl Fn(&str) -> bool) -> (bool, String) {
    let limite = Instant::now() + delai;
    let mut arbre = String::new();
    while Instant::now() < limite {
        if let Some(a) = sonder(nom) {
            arbre = a;
            if cond(&arbre) {
                return (true, arbre);
            }
        }
        patienter(Duration::from_millis(200));
    }
    (false, arbre)
}

/// Ferme toutes les sessions du processus de fond de `socket` (il s'arrête ensuite).
/// Sans cela, les sessions d'une interface de test arrêtée survivraient, comme après un
/// plantage, avec leurs shells.
pub fn fermer_sessions(socket: &Path) {
    use pupitre_proto::{ClientMsg, ServerMsg};
    let Ok((c, rx)) = pupitre::client::connect_or_spawn_at(socket) else {
        return;
    };
    let _ = c.send(ClientMsg::List);
    let limite = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < limite {
        match rx.try_recv() {
            Ok(ServerMsg::Sessions(v)) => {
                for s in v {
                    let _ = c.send(ClientMsg::Close { id: s.id });
                }
                std::thread::sleep(Duration::from_millis(300));
                return;
            }
            _ => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}

/// Exécute les tests nommés et termine le processus en échec si l'un échoue.
pub struct Executeur {
    echecs: usize,
}

impl Executeur {
    pub fn new() -> Self {
        Executeur { echecs: 0 }
    }

    pub fn lancer(&mut self, nom: &str, test: impl FnOnce() -> Resultat) {
        match test() {
            Ok(()) => println!("test {nom} ... ok"),
            Err(e) => {
                println!("test {nom} ... ÉCHEC : {e}");
                self.echecs += 1;
            }
        }
    }

    pub fn conclure(self) {
        if self.echecs > 0 {
            println!("{} test(s) graphique(s) en échec", self.echecs);
            std::process::exit(1);
        }
        println!("tests graphiques : tous réussis");
    }
}

/// `Err(message)` si `cond` est fausse.
pub fn verifier(cond: bool, message: impl FnOnce() -> String) -> Resultat {
    if cond {
        Ok(())
    } else {
        Err(message())
    }
}
