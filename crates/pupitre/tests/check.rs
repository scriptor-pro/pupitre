// SPDX-License-Identifier: GPL-3.0-or-later
//! `pupitre check` : vérification des thèmes et de la configuration, sans interface.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::process::{Command, Output};

/// Lance `pupitre <args>` avec un dossier de configuration `config` (vide si absent) et
/// sans affichage : la commande ne doit pas ouvrir de fenêtre.
fn pupitre(config: &Path, args: &[&str]) -> (i32, String) {
    let o: Output = Command::new(env!("CARGO_BIN_EXE_pupitre"))
        .args(args)
        .env("XDG_CONFIG_HOME", config)
        .env("HOME", config)
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .unwrap();
    let mut texte = String::from_utf8_lossy(&o.stdout).into_owned();
    texte.push_str(&String::from_utf8_lossy(&o.stderr));
    (o.status.code().unwrap_or(-1), texte)
}

fn ecrire(chemin: &Path, contenu: &str) {
    std::fs::create_dir_all(chemin.parent().unwrap()).unwrap();
    std::fs::write(chemin, contenu).unwrap();
}

#[test]
fn check_themes_integres_ok() {
    let d = tempfile::tempdir().unwrap();
    let (code, sortie) = pupitre(d.path(), &["check"]);
    assert_eq!(code, 0, "{sortie}");
    assert!(
        sortie.contains("sombre") && sortie.contains("clair"),
        "{sortie}"
    );
    // La vérification n'écrit rien (pas de fichier par défaut créé).
    assert!(!d.path().join("pupitre").exists());
}

#[test]
fn check_theme_mocha_echoue() {
    let d = tempfile::tempdir().unwrap();
    let theme = d.path().join("mocha.toml");
    ecrire(&theme, "[couleurs]\n8 = \"#585b70\"\n");
    let (code, sortie) = pupitre(d.path(), &["check", "--theme", theme.to_str().unwrap()]);
    assert_eq!(code, 1, "{sortie}");
    assert!(sortie.contains("2,46:1"), "{sortie}");
    assert!(
        sortie.contains("couleur 8 (#585b70) sur le fond (#1e1e2e) : 2,46:1, minimum 4,5:1"),
        "{sortie}"
    );
}

#[test]
fn check_theme_illisible_donne_la_ligne() {
    let d = tempfile::tempdir().unwrap();
    let theme = d.path().join("casse.toml");
    ecrire(&theme, "nom = \"x\"\nfond = \"rouge\"\n");
    let (code, sortie) = pupitre(d.path(), &["check", "--theme", theme.to_str().unwrap()]);
    assert_eq!(code, 1, "{sortie}");
    assert!(sortie.contains("ligne 2"), "{sortie}");
}

#[test]
fn check_config_invalide() {
    let d = tempfile::tempdir().unwrap();
    ecrire(
        &d.path().join("pupitre/pupitre.toml"),
        "historique_lignes = \"x\"\n",
    );
    let (code, sortie) = pupitre(d.path(), &["check"]);
    assert_eq!(code, 1, "{sortie}");
    assert!(sortie.contains("ligne 1"), "{sortie}");
}

#[test]
fn check_suit_le_theme_de_la_config() {
    let d = tempfile::tempdir().unwrap();
    let theme = d.path().join("mien.toml");
    ecrire(&theme, "[couleurs]\n8 = \"#585b70\"\n");
    ecrire(
        &d.path().join("pupitre/pupitre.toml"),
        &format!("theme = \"{}\"\n", theme.display()),
    );
    let (code, sortie) = pupitre(d.path(), &["check"]);
    assert_eq!(code, 1, "{sortie}");
    assert!(
        sortie.contains("mien") && sortie.contains("2,46:1"),
        "{sortie}"
    );
}

#[test]
fn argument_inconnu_refuse() {
    let d = tempfile::tempdir().unwrap();
    let (code, sortie) = pupitre(d.path(), &["chek"]);
    assert_eq!(code, 2, "{sortie}");
    assert!(sortie.contains("pupitre check"), "{sortie}");
    let (code, _) = pupitre(d.path(), &["check", "--theme"]);
    assert_eq!(code, 2);
}

/// `pupitre shells` avec un `PATH` ne contenant que `zsh` (faux exécutable) et `sh`.
#[test]
fn shells_liste_chemin_niveau_et_installation() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let bin = d.path().join("bin");
    ecrire(&bin.join("zsh"), "#!/bin/sh\n");
    std::fs::set_permissions(bin.join("zsh"), std::fs::Permissions::from_mode(0o755)).unwrap();
    // Liste de shells maîtrisée : seulement `/bin/sh` et `nologin` (qui ne doit pas sortir).
    let etc = d.path().join("shells");
    ecrire(
        &etc,
        "# essai\n/bin/sh\n/usr/sbin/nologin\n/usr/bin/git-shell\n",
    );
    let o = Command::new(env!("CARGO_BIN_EXE_pupitre"))
        .arg("shells")
        .env("PUPITRE_ETC_SHELLS", &etc)
        .env("PATH", &bin)
        .env("XDG_CONFIG_HOME", d.path())
        .env("HOME", d.path())
        .env("LANGUAGE", "fr")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .unwrap();
    let sortie = String::from_utf8_lossy(&o.stdout).into_owned();
    assert_eq!(o.status.code(), Some(0), "{sortie}");
    let zsh = format!("{}", bin.join("zsh").display());
    assert!(
        sortie
            .lines()
            .any(|l| l.contains(&zsh) && l.contains("(complet)")),
        "{sortie}"
    );
    assert!(
        sortie.lines().any(|l| l.contains("fish")),
        "fish absent mais cité : {sortie}"
    );
    for faux in ["nologin", "git-shell"] {
        assert!(
            !sortie.contains(faux),
            "{faux} ne doit pas figurer : {sortie}"
        );
    }
    let os = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
    if os.contains("debian") || os.contains("ubuntu") {
        assert!(sortie.contains("sudo apt install fish"), "{sortie}");
    }
    // Rien n'est écrit chez l'utilisateur.
    assert!(!d.path().join("pupitre").exists());
}

#[test]
fn aide_complete_en_ligne_de_commande() {
    let d = tempfile::tempdir().unwrap();
    for commande in ["aide", "help"] {
        let (code, sortie) = pupitre(d.path(), &[commande]);
        assert_eq!(code, 0, "{sortie}");
        assert!(sortie.contains("Raccourcis clavier"), "{sortie}");
        // Tableaux lus ligne par ligne, sans Markdown.
        assert!(sortie.contains("Nouvel onglet : Ctrl+Maj+T"), "{sortie}");
        assert!(
            !sortie.contains("|---") && !sortie.contains("**"),
            "{sortie}"
        );
    }
}

#[test]
fn resume_des_commandes_avec_help_et_h() {
    let d = tempfile::tempdir().unwrap();
    for option in ["--help", "-h"] {
        let (code, sortie) = pupitre(d.path(), &[option]);
        assert_eq!(code, 0, "{sortie}");
        assert!(sortie.contains("pupitre check"), "{sortie}");
        assert!(sortie.contains("pupitre aide"), "{sortie}");
        assert!(!sortie.contains("Raccourcis clavier"), "{sortie}");
    }
}
