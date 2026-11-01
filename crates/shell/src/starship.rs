// SPDX-License-Identifier: GPL-3.0-or-later
//! Starship du système et invite par défaut de Pupitre. Rien n'est compilé ni copié :
//! Pupitre utilise le `starship` du `PATH`.

use pupitre_config::ecrire_atomique;
use std::ffi::OsStr;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Invite par défaut de Pupitre (contenu de `assets/shell/starship.toml`).
pub const CONFIG_STARSHIP_DEFAUT: &str = include_str!("../../../assets/shell/starship.toml");

/// Vrai si `chemin` est un fichier ordinaire exécutable.
fn est_executable(chemin: &Path) -> bool {
    std::fs::metadata(chemin)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// Cherche le Starship du système. Ordre : `PUPITRE_STARSHIP` (variable d'essai, ignorée si elle
/// ne désigne pas un exécutable), puis `starship` dans les dossiers de `path`, sinon `None`.
/// Ne renvoie jamais d'erreur : un Starship introuvable n'est pas bloquant.
pub fn chercher_starship(env_pupitre: Option<&OsStr>, path: &OsStr) -> Option<PathBuf> {
    if let Some(v) = env_pupitre.filter(|v| !v.is_empty()) {
        let p = PathBuf::from(v);
        if est_executable(&p) {
            return Some(p);
        }
    }
    std::env::split_paths(path)
        .filter(|d| !d.as_os_str().is_empty())
        .map(|d| d.join("starship"))
        .find(|p| est_executable(p))
}

/// Écrit l'invite par défaut dans `chemin` (normalement `$XDG_CONFIG_HOME/pupitre/starship.toml`)
/// si rien n'existe à cet endroit. Un fichier existant (même vide), un lien symbolique (même
/// cassé) ne sont jamais modifiés. Les dossiers manquants sont créés.
pub fn creer_config_starship_si_absente(chemin: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(chemin) {
        Ok(_) => return Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    if let Some(dossier) = chemin.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dossier)?;
    }
    ecrire_atomique(chemin, CONFIG_STARSHIP_DEFAUT.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    fn faux_executable(dossier: &Path, nom: &str) -> PathBuf {
        let p = dossier.join(nom);
        std::fs::write(&p, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    #[test]
    fn chercher_starship_ordre() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let essai = faux_executable(a.path(), "mon-starship");
        let dans_b = faux_executable(b.path(), "starship");
        let path = std::env::join_paths([a.path(), b.path()]).unwrap();
        // 1. la variable d'essai l'emporte ;
        assert_eq!(
            chercher_starship(Some(essai.as_os_str()), &path),
            Some(essai.clone())
        );
        // 2. sinon le PATH, dans l'ordre ;
        assert_eq!(chercher_starship(None, &path), Some(dans_b.clone()));
        // variable vide, inexistante ou non exécutable : on passe au PATH.
        assert_eq!(
            chercher_starship(Some(OsStr::new("")), &path),
            Some(dans_b.clone())
        );
        assert_eq!(
            chercher_starship(Some(OsStr::new("/inexistant/starship")), &path),
            Some(dans_b.clone())
        );
        let non_exec = a.path().join("pas-exec");
        std::fs::write(&non_exec, "x").unwrap();
        assert_eq!(
            chercher_starship(Some(non_exec.as_os_str()), &path),
            Some(dans_b.clone())
        );
        // un dossier nommé starship n'est pas un exécutable.
        let c = tempfile::tempdir().unwrap();
        std::fs::create_dir(c.path().join("starship")).unwrap();
        assert_eq!(chercher_starship(None, c.path().as_os_str()), None);
        // 3. sinon rien.
        assert_eq!(chercher_starship(None, OsStr::new("")), None);
        assert_eq!(chercher_starship(None, OsStr::new("/inexistant")), None);
    }

    #[test]
    fn config_defaut_sans_couleur_exacte() {
        let c: Vec<char> = CONFIG_STARSHIP_DEFAUT.chars().collect();
        for (i, ch) in c.iter().enumerate() {
            if *ch == '#' {
                let suite: String = c.iter().skip(i + 1).take(6).collect();
                assert!(
                    !(suite.len() == 6 && suite.chars().all(|x| x.is_ascii_hexdigit())),
                    "couleur exacte : #{suite}"
                );
            }
        }
        assert!(CONFIG_STARSHIP_DEFAUT.starts_with("# SPDX-License-Identifier"));
    }

    #[test]
    fn config_defaut_valide() {
        let Some(starship) = chercher_starship(
            std::env::var_os("PUPITRE_STARSHIP").as_deref(),
            &std::env::var_os("PATH").unwrap_or_default(),
        ) else {
            eprintln!("SAUTÉ : Starship n'est pas installé");
            return;
        };
        let home = tempfile::tempdir().unwrap();
        let depot = tempfile::tempdir().unwrap();
        let config = home.path().join("starship.toml");
        std::fs::write(&config, CONFIG_STARSHIP_DEFAUT).unwrap();
        let git = |args: &[&str]| {
            let s = Command::new("git")
                .args(args)
                .current_dir(depot.path())
                .env("HOME", home.path())
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .status();
            matches!(s, Ok(s) if s.success())
        };
        let avec_git = git(&["init", "-q", "-b", "main"])
            && {
                std::fs::write(depot.path().join("suivi"), "a").unwrap();
                git(&["add", "suivi"])
            }
            && git(&[
                "-c",
                "user.email=a@b.c",
                "-c",
                "user.name=essai",
                "commit",
                "-q",
                "-m",
                "i",
            ]);
        if avec_git {
            std::fs::write(depot.path().join("suivi"), "b").unwrap();
            std::fs::write(depot.path().join("nouveau"), "c").unwrap();
        } else {
            eprintln!("git absent : module git non exercé");
        }
        let sortie = Command::new(&starship)
            .arg("prompt")
            .current_dir(depot.path())
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", home.path())
            .env("XDG_CONFIG_HOME", home.path().join(".config"))
            .env("XDG_CACHE_HOME", home.path().join(".cache"))
            .env("STARSHIP_CONFIG", &config)
            .env("USER", "essai")
            .output()
            .unwrap();
        let sortant = String::from_utf8_lossy(&sortie.stdout).into_owned();
        let erreur = String::from_utf8_lossy(&sortie.stderr).into_owned();
        assert!(
            sortie.status.success(),
            "code {:?} : {erreur}",
            sortie.status
        );
        assert!(sortant.contains('λ'), "pas de λ : {sortant:?}");
        for flux in [&sortant, &erreur] {
            assert!(
                !flux.lines().any(|l| l.contains("WARN")),
                "avertissement de Starship : {sortant:?} / {erreur:?}"
            );
        }
        if avec_git {
            assert!(sortant.contains("git:main"), "{sortant:?}");
            assert!(
                sortant.contains("Δ") && sortant.contains("○"),
                "{sortant:?}"
            );
        }
    }

    #[test]
    fn config_existante_jamais_ecrasee() {
        let t = tempfile::tempdir().unwrap();
        let chemin = t.path().join("pupitre/starship.toml");
        // Dossier manquant : créé ; fichier manquant : écrit avec l'invite par défaut.
        creer_config_starship_si_absente(&chemin).unwrap();
        assert_eq!(
            std::fs::read_to_string(&chemin).unwrap(),
            CONFIG_STARSHIP_DEFAUT
        );
        // Fichier de l'utilisateur : intact, y compris vide.
        for contenu in ["format = \"mien> \"\n", ""] {
            std::fs::write(&chemin, contenu).unwrap();
            creer_config_starship_si_absente(&chemin).unwrap();
            assert_eq!(std::fs::read_to_string(&chemin).unwrap(), contenu);
        }
        // Lien symbolique valide : suivi, jamais remplacé.
        let cible = t.path().join("ailleurs.toml");
        std::fs::write(&cible, "x = 1\n").unwrap();
        let lien = t.path().join("lien.toml");
        std::os::unix::fs::symlink(&cible, &lien).unwrap();
        creer_config_starship_si_absente(&lien).unwrap();
        assert_eq!(std::fs::read_to_string(&cible).unwrap(), "x = 1\n");
        assert!(std::fs::symlink_metadata(&lien).unwrap().is_symlink());
        // Lien cassé : on n'écrit pas à travers, on ne plante pas non plus.
        let casse = t.path().join("casse.toml");
        std::os::unix::fs::symlink(t.path().join("absent.toml"), &casse).unwrap();
        let _ = creer_config_starship_si_absente(&casse);
        assert!(std::fs::symlink_metadata(&casse).unwrap().is_symlink());
    }

    #[test]
    fn starship_n_ouvre_pas_la_config_utilisateur() {
        let Some(starship) = chercher_starship(
            std::env::var_os("PUPITRE_STARSHIP").as_deref(),
            &std::env::var_os("PATH").unwrap_or_default(),
        ) else {
            eprintln!("SAUTÉ : Starship n'est pas installé");
            return;
        };
        let strace_ok = Command::new("strace")
            .arg("-V")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !strace_ok {
            eprintln!("SAUTÉ : strace n'est pas installé");
            return;
        }
        let home = tempfile::tempdir().unwrap();
        let config = home.path().join("pupitre-starship.toml");
        std::fs::write(&config, CONFIG_STARSHIP_DEFAUT).unwrap();
        let trace = home.path().join("trace.txt");
        let sortie = Command::new("strace")
            .args(["-f", "-e", "trace=openat", "-o"])
            .arg(&trace)
            .arg(&starship)
            .arg("prompt")
            .current_dir(home.path())
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", home.path())
            .env("XDG_CONFIG_HOME", home.path().join(".config"))
            .env("XDG_CACHE_HOME", home.path().join(".cache"))
            .env("STARSHIP_CONFIG", &config)
            .output()
            .unwrap();
        let trace_txt = std::fs::read_to_string(&trace).unwrap_or_default();
        if !sortie.status.success() && trace_txt.is_empty() {
            eprintln!("SAUTÉ : strace inutilisable ici (ptrace refusé ?)");
            return;
        }
        assert!(
            trace_txt.contains("pupitre-starship.toml"),
            "la configuration de Pupitre n'a pas été ouverte : {trace_txt}"
        );
        assert!(
            !trace_txt.contains(".config/starship.toml"),
            "Starship a ouvert une configuration utilisateur : {trace_txt}"
        );
    }
}
