// SPDX-License-Identifier: GPL-3.0-or-later
//! Tests de la Task 3 : couleurs, thèmes, contraste, fichier de configuration.
#![allow(clippy::unwrap_used)]

use pupitre_config::*;
use std::error::Error;
use std::path::PathBuf;

type R = Result<(), Box<dyn Error>>;

#[test]
fn ratios_de_reference() -> R {
    assert!((contrast_ratio("#ffffff".parse()?, "#000000".parse()?) - 21.0).abs() < 0.01);
    assert!((contrast_ratio("#8a8ea6".parse()?, "#1e1e2e".parse()?) - 5.08).abs() < 0.01);
    assert!((contrast_ratio("#252630".parse()?, "#8a8ea6".parse()?) - 4.65).abs() < 0.01);
    // Le ratio ne dépend pas de l'ordre.
    let (a, b) = ("#123456".parse::<Rgb>()?, "#fedcba".parse::<Rgb>()?);
    assert_eq!(contrast_ratio(a, b), contrast_ratio(b, a));
    Ok(())
}

#[test]
fn themes_integres_conformes() {
    assert_eq!(check_theme(&theme_sombre()), vec![]);
    assert_eq!(check_theme(&theme_clair()), vec![]);
}

#[test]
fn mocha_d_origine_signale_le_gris() -> R {
    let mut t = theme_sombre();
    t.palette[0] = "#45475a".parse()?;
    t.palette[8] = "#585b70".parse()?;
    t.palette[15] = "#a6adc8".parse()?;
    let p = check_theme(&t);
    assert!(p.iter().any(
        |x| x.description == "couleur 8 (#585b70) sur le fond (#1e1e2e)"
            && (x.ratio - 2.46).abs() < 0.01
    ));
    assert_eq!(p.len(), 7, "{p:#?}"); // 1/0, 4/0, 9/0, 12/0, 15/0, 8/fond, 8/0
    Ok(())
}

#[test]
fn curseur_peu_visible_signale() -> R {
    let mut t = theme_sombre();
    t.curseur = "#2a2a3a".parse()?;
    let p = check_theme(&t);
    assert_eq!(p.len(), 1);
    assert!(p[0].description.starts_with("curseur"));
    assert_eq!(p[0].minimum, 3.0);
    Ok(())
}

#[test]
fn couleur_invalide_refusee() {
    for mauvais in [
        "#12010", "bleu", "", "#", "#1234567", "123456", "#12345g", "#ééé", "#1é345",
    ] {
        assert!(mauvais.parse::<Rgb>().is_err(), "{mauvais:?} accepté");
    }
}

#[test]
fn couleur_lue_et_affichee() -> R {
    let c: Rgb = "#A6E3a1".parse()?;
    assert_eq!(
        c,
        Rgb {
            r: 0xa6,
            g: 0xe3,
            b: 0xa1
        }
    );
    assert_eq!(c.to_string(), "#a6e3a1");
    Ok(())
}

#[test]
fn config_absente_donne_defauts() -> R {
    let d = tempfile::tempdir()?;
    let chemin = d.path().join("sous/dossier/pupitre.toml");
    assert!(creer_defaut_si_absent(&chemin)?);
    assert!(
        !creer_defaut_si_absent(&chemin)?,
        "ne recrée pas un fichier existant"
    );
    let c = charger(&chemin).map_err(|e| e.message)?;
    assert_eq!(c.police, "B612 Mono 11");
    assert_eq!(c.historique_lignes, 10_000);
    assert_eq!(c, Config::default());
    Ok(())
}

#[test]
fn fichier_vide_donne_defauts() -> R {
    let d = tempfile::tempdir()?;
    let chemin = d.path().join("pupitre.toml");
    std::fs::write(&chemin, "# rien\n")?;
    assert_eq!(charger(&chemin).map_err(|e| e.message)?, Config::default());
    Ok(())
}

#[test]
fn valeurs_lues() -> R {
    let d = tempfile::tempdir()?;
    let chemin = d.path().join("pupitre.toml");
    std::fs::write(
        &chemin,
        "police = \"Iosevka 13\"\ntheme = \"clair\"\ncurseur_clignote = true\nhistorique_lignes = 500\nzoom_max_pourcent = 300\n",
    )?;
    let c = charger(&chemin).map_err(|e| e.message)?;
    assert_eq!(
        c,
        Config {
            police: "Iosevka 13".into(),
            theme: ThemeChoix::Clair,
            curseur_clignote: true,
            historique_lignes: 500,
            zoom_max_pourcent: 300,
        }
    );
    std::fs::write(&chemin, "theme = \"~/themes/nuit.toml\"\n")?;
    assert!(matches!(
        charger(&chemin).map_err(|e| e.message)?.theme,
        ThemeChoix::Fichier(_)
    ));
    std::fs::write(&chemin, "theme = \"/etc/pupitre/nuit.toml\"\n")?;
    assert_eq!(
        charger(&chemin).map_err(|e| e.message)?.theme,
        ThemeChoix::Fichier(PathBuf::from("/etc/pupitre/nuit.toml"))
    );
    Ok(())
}

fn erreur(contenu: &str) -> ErreurConfig {
    let d = tempfile::tempdir().unwrap();
    let chemin = d.path().join("pupitre.toml");
    std::fs::write(&chemin, contenu).unwrap();
    charger(&chemin).unwrap_err()
}

#[test]
fn toml_invalide_donne_ligne() {
    let e = erreur("police = \"B612 Mono 11\"\nhistorique_lignes = \"beaucoup\"\n");
    assert_eq!(e.ligne, Some(2), "{e:?}");
    assert_eq!(erreur("a = = 1\n").ligne, Some(1));
}

#[test]
fn valeurs_hors_bornes_ou_inconnues_refusees() {
    assert_eq!(erreur("\n\ntheme = \"violet\"\n").ligne, Some(3));
    assert_eq!(erreur("zoom_max_pourcent = 50\n").ligne, Some(1));
    assert_eq!(erreur("police = \"  \"\n").ligne, Some(1));
    assert_eq!(
        erreur("polise = \"B612 Mono 11\"\n").ligne,
        Some(1),
        "clé inconnue (faute de frappe)"
    );
    assert!(erreur("historique_lignes = -3\n").ligne.is_some());
}

#[test]
fn messages_en_francais() {
    let m = |c: &str| erreur(c).message;
    assert_eq!(
        m("historique_lignes = \"beaucoup\"\n"),
        "type incorrect : le texte \"beaucoup\" au lieu de un nombre entier"
    );
    assert_eq!(
        m("curseur_clignote = 3\n"),
        "type incorrect : le nombre 3 au lieu de true ou false"
    );
    assert!(m("polise = \"x\"\n").starts_with("clé inconnue « polise »"));
    assert_eq!(
        m("zoom_max_pourcent = 99999999\n"),
        "zoom maximum 99999999 % hors limites (de 100 à 500)"
    );
    assert_eq!(
        m("historique_lignes = -3\n"),
        "historique de -3 lignes hors limites (de 0 à 1000000)"
    );
    assert!(m("police = \"a\"\npolice = \"b\"\n").starts_with("clé en double"));
    assert!(m("a = = 1\n").starts_with("syntaxe TOML incorrecte"));
    assert!(m("police = \" \"\n").starts_with("police vide"));
    assert!(m("theme = \"violet\"\n").starts_with("thème « violet » inconnu"));
}

#[test]
fn fichier_illisible_donne_erreur_sans_ligne() {
    let d = tempfile::tempdir().unwrap();
    let e = charger(&d.path().join("absent.toml")).unwrap_err();
    assert_eq!(e.ligne, None);
    std::fs::write(d.path().join("binaire.toml"), [0xff, 0xfe, 0x00]).unwrap();
    assert!(charger(&d.path().join("binaire.toml")).is_err());
}

#[test]
fn defaut_commente_est_valide() {
    // Le fichier fourni à l'utilisateur doit donner exactement les valeurs par défaut.
    let c: Config = toml::from_str(CONFIG_DEFAUT).unwrap();
    assert_eq!(c, Config::default());
}

#[test]
fn ecriture_atomique_garde_precedent() -> R {
    let d = tempfile::tempdir()?;
    let f = d.path().join("pupitre.toml");
    ecrire_atomique(&f, b"a")?;
    ecrire_atomique(&f, b"b")?;
    assert_eq!(std::fs::read(&f)?, b"b");
    assert_eq!(
        std::fs::read(d.path().join("pupitre.toml.precedent"))?,
        b"a"
    );
    let mut noms: Vec<_> = std::fs::read_dir(d.path())?
        .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
        .collect::<Result<_, _>>()?;
    noms.sort();
    assert_eq!(
        noms,
        ["pupitre.toml", "pupitre.toml.precedent"],
        "aucun fichier temporaire restant"
    );
    Ok(())
}

#[test]
fn ecriture_atomique_suit_le_lien_symbolique() -> R {
    // Un fichier de config géré par un outil de dotfiles est souvent un lien : il doit rester un lien.
    let d = tempfile::tempdir()?;
    let vrai = d.path().join("vrai.toml");
    let lien = d.path().join("pupitre.toml");
    std::fs::write(&vrai, "a")?;
    std::os::unix::fs::symlink(&vrai, &lien)?;
    ecrire_atomique(&lien, b"b")?;
    assert!(std::fs::symlink_metadata(&lien)?.file_type().is_symlink());
    assert_eq!(std::fs::read(&vrai)?, b"b");
    Ok(())
}

#[test]
fn ecriture_atomique_garde_les_droits() -> R {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir()?;
    let f = d.path().join("secret.toml");
    std::fs::write(&f, "a")?;
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600))?;
    ecrire_atomique(&f, b"b")?;
    assert_eq!(std::fs::metadata(&f)?.permissions().mode() & 0o777, 0o600);
    Ok(())
}

#[test]
fn ecriture_dans_dossier_absent_echoue_proprement() {
    let d = tempfile::tempdir().unwrap();
    assert!(ecrire_atomique(&d.path().join("absent/x.toml"), b"a").is_err());
}

#[test]
fn chemin_config_suit_xdg() {
    // Seul test qui touche à l'environnement du processus.
    std::env::set_var("XDG_CONFIG_HOME", "/tmp/xdg-essai");
    assert_eq!(
        chemin_config(),
        PathBuf::from("/tmp/xdg-essai/pupitre/pupitre.toml")
    );
    std::env::set_var("XDG_CONFIG_HOME", "relatif/ignore");
    assert!(chemin_config().ends_with(".config/pupitre/pupitre.toml"));
    std::env::remove_var("XDG_CONFIG_HOME");
}
