// SPDX-License-Identifier: GPL-3.0-or-later
//! Couleurs RVB et ratio de contraste WCAG 2.x.

use pupitre_i18n::{ta, ArgI18n};
use std::fmt;
use std::str::FromStr;

/// Couleur sRGB sur 8 bits par canal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// Couleur illisible ; contient le texte fautif.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErreurCouleur(pub String);

impl fmt::Display for ErreurCouleur {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&ta(
            "couleur-invalide",
            &[("texte", ArgI18n::Texte(format!("{:?}", self.0)))],
        ))
    }
}

impl std::error::Error for ErreurCouleur {}

impl FromStr for Rgb {
    type Err = ErreurCouleur;

    /// Accepte `#rrggbb`, chiffres hexadécimaux en majuscules ou minuscules.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let erreur = || ErreurCouleur(s.to_owned());
        let hex = s.strip_prefix('#').ok_or_else(erreur)?;
        // Seulement de l'ASCII : on peut découper par octets sans risque.
        if hex.len() != 6 || !hex.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(erreur());
        }
        let canal = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| erreur());
        Ok(Rgb {
            r: canal(0)?,
            g: canal(2)?,
            b: canal(4)?,
        })
    }
}

impl fmt::Display for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

impl Rgb {
    /// Luminance relative WCAG 2.x (sRGB, seuil de linéarisation 0,03928).
    pub fn luminance(self) -> f64 {
        let lin = |c: u8| {
            let c = f64::from(c) / 255.0;
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(self.r) + 0.7152 * lin(self.g) + 0.0722 * lin(self.b)
    }
}

/// Ratio de contraste WCAG entre deux couleurs, de 1 à 21, indépendant de l'ordre.
pub fn contrast_ratio(a: Rgb, b: Rgb) -> f64 {
    let (la, lb) = (a.luminance(), b.luminance());
    let (clair, fonce) = if la >= lb { (la, lb) } else { (lb, la) };
    (clair + 0.05) / (fonce + 0.05)
}
