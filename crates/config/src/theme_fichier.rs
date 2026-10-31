// SPDX-License-Identifier: GPL-3.0-or-later
//! Thèmes de l'utilisateur en fichier TOML. Tout est facultatif : le thème part d'un thème
//! intégré (`base`, sombre par défaut) et ne change que ce qui est indiqué.
//!
//! ```toml
//! nom = "mon thème"          # sinon : nom du fichier
//! base = "sombre"            # ou "clair"
//! fond = "#1e1e2e"
//! texte = "#cdd6f4"
//! curseur = "#f5e0dc"
//! selection_fond = "#45475a"
//! selection_texte = "#cdd6f4"
//! palette = ["#...", …]      # exactement 16 couleurs
//! [couleurs]                 # ou couleur par couleur
//! 8 = "#8a8ea6"
//! [roles]                    # usage d'une couleur : "texte", "fond", "texte et fond"
//! 7 = "fond"
//! ```

use crate::couleur::Rgb;
use crate::fichier::{lire_toml, notre, ErreurConfig};
use crate::theme::{theme_clair, theme_sombre, Role, Theme};
use serde::{Deserialize, Deserializer};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ThemeToml {
    nom: Option<String>,
    base: Option<Base>,
    fond: Option<Couleur>,
    texte: Option<Couleur>,
    curseur: Option<Couleur>,
    selection_fond: Option<Couleur>,
    selection_texte: Option<Couleur>,
    palette: Option<Palette>,
    couleurs: Option<BTreeMap<Indice, Couleur>>,
    roles: Option<BTreeMap<Indice, RoleToml>>,
}

struct Couleur(Rgb);
struct Palette([Rgb; 16]);
#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct Indice(usize);
struct RoleToml(Role);
enum Base {
    Sombre,
    Clair,
}

impl<'de> Deserialize<'de> for Couleur {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = String::deserialize(d)?;
        v.trim()
            .parse()
            .map(Couleur)
            .map_err(|e: crate::couleur::ErreurCouleur| notre(e.to_string()))
    }
}

impl<'de> Deserialize<'de> for Palette {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Vec::<Couleur>::deserialize(d)?;
        let n = v.len();
        let couleurs: Vec<Rgb> = v.into_iter().map(|c| c.0).collect();
        couleurs.try_into().map(Palette).map_err(|_| {
            notre(format!(
                "la palette doit avoir exactement 16 couleurs (0 à 15), elle en a {n}"
            ))
        })
    }
}

impl<'de> Deserialize<'de> for Indice {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = String::deserialize(d)?;
        match v.trim().parse::<usize>() {
            Ok(i) if i < 16 => Ok(Indice(i)),
            _ => Err(notre(format!(
                "numéro de couleur « {v} » invalide : de 0 à 15"
            ))),
        }
    }
}

impl<'de> Deserialize<'de> for RoleToml {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = String::deserialize(d)?;
        match v.trim().to_lowercase().as_str() {
            "texte" => Ok(RoleToml(Role::Texte)),
            "fond" => Ok(RoleToml(Role::Fond)),
            "texte et fond" | "texte+fond" | "texte-et-fond" => Ok(RoleToml(Role::TexteEtFond)),
            _ => Err(notre(format!(
                "rôle « {v} » inconnu : « texte », « fond » ou « texte et fond »"
            ))),
        }
    }
}

impl<'de> Deserialize<'de> for Base {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = String::deserialize(d)?;
        match v.trim() {
            "sombre" => Ok(Base::Sombre),
            "clair" => Ok(Base::Clair),
            _ => Err(notre(format!(
                "base « {v} » inconnue : « sombre » ou « clair »"
            ))),
        }
    }
}

/// Lit un thème en fichier. Ne panique jamais ; l'erreur indique la ligne fautive.
/// Le contraste n'est pas vérifié ici (voir [`crate::check_theme`]).
pub fn charger_theme(chemin: &Path) -> Result<Theme, ErreurConfig> {
    let f: ThemeToml = lire_toml(chemin)?;
    let mut t = match f.base {
        Some(Base::Clair) => theme_clair(),
        Some(Base::Sombre) | None => theme_sombre(),
    };
    t.nom = f
        .nom
        .map(|n| n.trim().to_owned())
        .filter(|n| !n.is_empty())
        .or_else(|| chemin.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "personnel".to_owned());
    let champs = [
        (f.fond, &mut t.fond),
        (f.texte, &mut t.texte),
        (f.curseur, &mut t.curseur),
        (f.selection_fond, &mut t.selection_fond),
        (f.selection_texte, &mut t.selection_texte),
    ];
    for (valeur, cible) in champs {
        if let Some(Couleur(c)) = valeur {
            *cible = c;
        }
    }
    if let Some(Palette(p)) = f.palette {
        t.palette = p;
    }
    for (Indice(i), Couleur(c)) in f.couleurs.unwrap_or_default() {
        t.palette[i] = c;
    }
    for (Indice(i), RoleToml(r)) in f.roles.unwrap_or_default() {
        t.roles[i] = r;
    }
    Ok(t)
}
