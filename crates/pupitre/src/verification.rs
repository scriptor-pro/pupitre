// SPDX-License-Identifier: GPL-3.0-or-later
//! `pupitre check [--theme CHEMIN]` : vérifie la configuration et le contraste des thèmes,
//! sans ouvrir de fenêtre et sans rien écrire.

use pupitre_config::{
    charger, charger_theme, check_theme, chemin_config, theme_clair, theme_sombre, Config, Theme,
    ThemeChoix,
};
use pupitre_i18n::{t, ta, ArgI18n};
use std::io::Write;
use std::path::PathBuf;

/// Aide de la ligne de commande, dans la langue active.
pub fn usage() -> String {
    format!("{}\n", t("usage"))
}

/// Codes de sortie : 0 = aucun problème, 1 = problème trouvé, 2 = commande mal formée.
pub fn executer(args: &[String], sortie: &mut impl Write) -> i32 {
    match args {
        [] => verifier_config(sortie),
        [opt, chemin] if opt == "--theme" => match charger_theme(&PathBuf::from(chemin)) {
            Ok(t) => rapporter(&t, sortie),
            Err(e) => {
                let _ = writeln!(
                    sortie,
                    "{}",
                    ta(
                        "theme-illisible",
                        &[
                            ("chemin", ArgI18n::from(chemin.as_str())),
                            ("erreur", ArgI18n::from(e.to_string()))
                        ]
                    )
                );
                1
            }
        },
        _ => {
            let _ = write!(sortie, "{}\n{}", t("arguments-incorrects"), usage());
            2
        }
    }
}

fn verifier_config(sortie: &mut impl Write) -> i32 {
    let chemin = chemin_config();
    let config = if chemin.exists() {
        match charger(&chemin) {
            Ok(c) => c,
            Err(e) => {
                let _ = writeln!(
                    sortie,
                    "{}",
                    ta(
                        "check-config-invalide",
                        &[
                            ("chemin", ArgI18n::from(chemin.display().to_string())),
                            ("erreur", ArgI18n::from(e.to_string()))
                        ]
                    )
                );
                return 1;
            }
        }
    } else {
        let _ = writeln!(
            sortie,
            "{}",
            ta(
                "check-pas-de-fichier",
                &[("chemin", ArgI18n::from(chemin.display().to_string()))]
            )
        );
        Config::default()
    };
    let themes: Vec<Result<Theme, String>> = match &config.theme {
        ThemeChoix::Sombre => vec![Ok(theme_sombre())],
        ThemeChoix::Clair => vec![Ok(theme_clair())],
        ThemeChoix::Systeme => vec![Ok(theme_sombre()), Ok(theme_clair())],
        ThemeChoix::Fichier(p) => {
            vec![charger_theme(p).map_err(|e| {
                ta(
                    "theme-illisible",
                    &[
                        ("chemin", ArgI18n::from(p.display().to_string())),
                        ("erreur", ArgI18n::from(e.to_string())),
                    ],
                )
            })]
        }
    };
    let mut code = 0;
    for t in themes {
        let c = match t {
            Ok(t) => rapporter(&t, sortie),
            Err(e) => {
                let _ = writeln!(sortie, "{e}");
                1
            }
        };
        code = code.max(c);
    }
    code
}

/// Affiche les problèmes de contraste du thème ; 1 s'il y en a.
fn rapporter(t: &Theme, sortie: &mut impl Write) -> i32 {
    let problemes = check_theme(t);
    if problemes.is_empty() {
        let _ = writeln!(
            sortie,
            "{}",
            ta("check-theme-ok", &[("nom", ArgI18n::from(t.nom.as_str()))])
        );
        return 0;
    }
    let _ = writeln!(
        sortie,
        "{}",
        ta(
            "check-theme-problemes",
            &[
                ("nom", ArgI18n::from(t.nom.as_str())),
                ("n", ArgI18n::from(problemes.len()))
            ]
        )
    );
    for p in problemes {
        let _ = writeln!(sortie, "  - {p}");
    }
    1
}
