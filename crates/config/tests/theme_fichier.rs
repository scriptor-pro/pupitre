// SPDX-License-Identifier: GPL-3.0-or-later
//! Thèmes en fichier TOML : héritage d'un thème intégré, couleurs, rôles, erreurs avec ligne.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pupitre_config::{charger_theme, check_theme, theme_clair, theme_sombre, Rgb, Role};
use std::io::Write;

fn fichier(contenu: &str) -> tempfile::NamedTempFile {
    let mut f = tempfile::NamedTempFile::new().unwrap();
    f.write_all(contenu.as_bytes()).unwrap();
    f
}

fn rgb(s: &str) -> Rgb {
    s.parse().unwrap()
}

#[test]
fn fichier_vide_donne_le_sombre() {
    let f = fichier("");
    let t = charger_theme(f.path()).unwrap();
    let mut attendu = theme_sombre();
    attendu.nom = t.nom.clone();
    assert_eq!(t, attendu);
}

#[test]
fn base_clair_et_couleur_modifiee() {
    let f = fichier(
        "nom = \"mien\"\nbase = \"clair\"\nfond = \"#ffffff\"\n[couleurs]\n8 = \"#585b70\"\n",
    );
    let t = charger_theme(f.path()).unwrap();
    assert_eq!(t.nom, "mien");
    assert_eq!(t.fond, rgb("#ffffff"));
    assert_eq!(t.palette[8], rgb("#585b70"));
    assert_eq!(t.palette[1], theme_clair().palette[1]);
    assert_eq!(t.roles, theme_clair().roles);
}

#[test]
fn palette_complete_et_roles() {
    let couleurs: Vec<String> = (0..16).map(|i| format!("\"#0000{i:02x}\"")).collect();
    let texte = format!(
        "palette = [{}]\n[roles]\n0 = \"texte\"\n7 = \"fond\"\n8 = \"texte et fond\"\n",
        couleurs.join(", ")
    );
    let t = charger_theme(fichier(&texte).path()).unwrap();
    assert_eq!(t.palette[15], rgb("#00000f"));
    assert_eq!(t.roles[0], Role::Texte);
    assert_eq!(t.roles[7], Role::Fond);
    assert_eq!(t.roles[8], Role::TexteEtFond);
}

#[test]
fn mocha_d_origine_echoue_au_controle() {
    let f = fichier("[couleurs]\n8 = \"#585b70\"\n");
    let t = charger_theme(f.path()).unwrap();
    let p = check_theme(&t);
    assert!(p.iter().any(
        |p| p.description == "couleur 8 (#585b70) sur le fond (#1e1e2e)"
            && (p.ratio - 2.46).abs() < 0.01
    ));
}

#[test]
fn erreurs_avec_ligne() {
    let cas = [
        ("nom = \"x\"\nfond = \"rouge\"\n", 2, "couleur invalide"),
        ("palette = [\"#000000\"]\n", 1, "16 couleurs"),
        ("[couleurs]\n16 = \"#000000\"\n", 2, "de 0 à 15"),
        ("[roles]\n3 = \"decor\"\n", 2, "rôle"),
        ("base = \"rose\"\n", 1, "base"),
        ("inconnu = 1\n", 1, "inconnu"),
    ];
    for (texte, ligne, motif) in cas {
        let e = charger_theme(fichier(texte).path()).unwrap_err();
        assert_eq!(e.ligne, Some(ligne), "{texte:?} → {e}");
        assert!(e.message.contains(motif), "{texte:?} → {e}");
    }
}

#[test]
fn fichier_absent_sans_panique() {
    let e = charger_theme(std::path::Path::new("/nexiste/pas/theme.toml")).unwrap_err();
    assert_eq!(e.ligne, None);
}

#[test]
fn probleme_affiche_en_francais() {
    let f = fichier("[couleurs]\n8 = \"#585b70\"\n");
    let t = charger_theme(f.path()).unwrap();
    let p = check_theme(&t);
    let textes: Vec<String> = p.iter().map(|p| p.to_string()).collect();
    assert!(
        textes.contains(
            &"couleur 8 (#585b70) sur le fond (#1e1e2e) : 2,46:1, minimum 4,5:1".to_string()
        ),
        "{textes:?}"
    );
    let mut curseur = theme_sombre();
    curseur.curseur = rgb("#252630");
    let p = check_theme(&curseur);
    assert!(
        p.iter().any(|p| p.to_string().ends_with(", minimum 3:1")),
        "{p:?}"
    );
}
