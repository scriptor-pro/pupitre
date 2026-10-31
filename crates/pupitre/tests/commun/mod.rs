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

pub fn racine() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Compile `pupitre` et `pupitre-fond`, et indique au client où trouver `pupitre-fond`.
pub fn compiler() {
    let ok = Command::new(env!("CARGO"))
        .args([
            "build",
            "-q",
            "-p",
            "pupitre-fond",
            "-p",
            "pupitre",
            "--bins",
        ])
        .current_dir(racine())
        .status()
        .unwrap()
        .success();
    assert!(ok, "compilation des binaires");
    std::env::set_var("PUPITRE_FOND", racine().join("target/debug/pupitre-fond"));
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
    perso
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

/// Lance la sonde AT-SPI sur l'application `nom` sans bloquer la boucle GTK (l'application
/// sondée peut être ce processus même). Renvoie l'arbre « rôle\tnom\ttexte ».
pub fn sonder(nom: &str) -> Option<String> {
    let sonde = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/sonde.py");
    let mut enfant = Command::new("python3")
        .arg(sonde)
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
