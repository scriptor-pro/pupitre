// SPDX-License-Identifier: GPL-3.0-or-later
//! Thèmes de couleurs intégrés et vérificateur de contraste (WCAG AA).

use crate::couleur::{contrast_ratio, Rgb};
use pupitre_i18n::{t as tr, ta, ArgI18n};

/// Usage d'une couleur de la palette : c'est lui qui décide quelles paires sont vérifiées.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Utilisée pour écrire, sur le fond.
    Texte,
    /// Utilisée comme fond (sous du texte des autres couleurs).
    Fond,
    /// Utilisée des deux façons.
    TexteEtFond,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub nom: String,
    pub fond: Rgb,
    pub texte: Rgb,
    pub curseur: Rgb,
    pub selection_fond: Rgb,
    pub selection_texte: Rgb,
    pub palette: [Rgb; 16],
    pub roles: [Role; 16],
}

/// Une paire de couleurs sous le minimum requis.
#[derive(Debug, Clone, PartialEq)]
pub struct ProblemeContraste {
    /// En français, ex. « couleur 8 (#585b70) sur le fond (#1e1e2e) ».
    pub description: String,
    pub ratio: f64,
    pub minimum: f64,
}

impl std::fmt::Display for ProblemeContraste {
    /// Ex. « couleur 8 (#585b70) sur le fond (#1e1e2e) : 2,46:1, minimum 4,5:1 ».
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&ta(
            "contraste-ligne",
            &[
                ("description", ArgI18n::Texte(self.description.clone())),
                ("ratio", ArgI18n::Texte(nombre_fr(self.ratio, 2))),
                ("minimum", ArgI18n::Texte(nombre_fr(self.minimum, 1))),
            ],
        ))
    }
}

/// Nombre à virgule décimale (écriture française), sans « ,0 » final inutile.
fn nombre_fr(v: f64, decimales: usize) -> String {
    let mut s = format!("{v:.decimales$}");
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    s.replace('.', ",")
}

/// Contraste minimum du texte (WCAG AA, 1.4.3).
const MIN_TEXTE: f64 = 4.5;
/// Contraste minimum du curseur (élément non textuel, WCAG 1.4.11).
const MIN_CURSEUR: f64 = 3.0;

/// Les valeurs des thèmes intégrés sont des constantes vérifiées par les tests.
const fn rgb(v: u32) -> Rgb {
    Rgb {
        r: (v >> 16) as u8,
        g: (v >> 8) as u8,
        b: v as u8,
    }
}

/// Thème sombre : Catppuccin Mocha retouché (gris 0, 8 et 15 corrigés pour l'AA).
pub fn theme_sombre() -> Theme {
    let mut roles = [Role::Texte; 16];
    roles[0] = Role::Fond;
    roles[8] = Role::TexteEtFond;
    Theme {
        nom: "sombre".into(),
        fond: rgb(0x1e1e2e),
        texte: rgb(0xcdd6f4),
        curseur: rgb(0xf5e0dc),
        selection_fond: rgb(0x45475a),
        selection_texte: rgb(0xcdd6f4),
        palette: [
            rgb(0x252630),
            rgb(0xf38ba8),
            rgb(0xa6e3a1),
            rgb(0xf9e2af),
            rgb(0x89b4fa),
            rgb(0xf5c2e7),
            rgb(0x94e2d5),
            rgb(0xbac2de),
            rgb(0x8a8ea6),
            rgb(0xf38ba8),
            rgb(0xa6e3a1),
            rgb(0xf9e2af),
            rgb(0x89b4fa),
            rgb(0xf5c2e7),
            rgb(0x94e2d5),
            rgb(0xb2b8cf),
        ],
        roles,
    }
}

/// Thème clair : Catppuccin Latte ajusté pour l'AA.
pub fn theme_clair() -> Theme {
    let mut roles = [Role::Texte; 16];
    roles[7] = Role::Fond;
    roles[15] = Role::Fond;
    roles[8] = Role::TexteEtFond;
    Theme {
        nom: "clair".into(),
        fond: rgb(0xeff1f5),
        texte: rgb(0x4c4f69),
        curseur: rgb(0x4c4f69),
        selection_fond: rgb(0xccd0da),
        selection_texte: rgb(0x4c4f69),
        palette: [
            rgb(0x4c4f69),
            rgb(0xc30e35),
            rgb(0x2d701e),
            rgb(0x895712),
            rgb(0x0a54e7),
            rgb(0xb31b8a),
            rgb(0x116c71),
            rgb(0xdce0e8),
            rgb(0x5c5f77),
            rgb(0xc30e35),
            rgb(0x2d701e),
            rgb(0x895712),
            rgb(0x0a54e7),
            rgb(0xb31b8a),
            rgb(0x116c71),
            rgb(0xffffff),
        ],
        roles,
    }
}

/// Liste les paires de couleurs sous le minimum. Paires vérifiées : texte/fond ; sélection ;
/// curseur/fond ; chaque couleur `Texte` ou `TexteEtFond` sur le fond ; chaque `TexteEtFond`
/// sur chaque `Fond` ; chaque `Texte` sur chaque `Fond`. Une paire n'est signalée qu'une fois.
pub fn check_theme(t: &Theme) -> Vec<ProblemeContraste> {
    let mut problemes = Vec::new();
    let mut verifier = |avant: (String, Rgb), arriere: (String, Rgb), minimum: f64| {
        let ratio = contrast_ratio(avant.1, arriere.1);
        let description = ta(
            "contraste-description",
            &[
                ("avant", ArgI18n::Texte(avant.0)),
                ("couleur_avant", ArgI18n::Texte(avant.1.to_string())),
                ("arriere", ArgI18n::Texte(arriere.0)),
                ("couleur_arriere", ArgI18n::Texte(arriere.1.to_string())),
            ],
        );
        if ratio < minimum
            && !problemes
                .iter()
                .any(|p: &ProblemeContraste| p.description == description)
        {
            problemes.push(ProblemeContraste {
                description,
                ratio,
                minimum,
            });
        }
    };
    let fond = || (tr("contraste-le-fond"), t.fond);
    let numero = |i: usize| ArgI18n::Nombre(i64::try_from(i).unwrap_or(0));
    let couleur = |i: usize| (ta("contraste-couleur", &[("n", numero(i))]), t.palette[i]);
    let sur_couleur = |i: usize| {
        (
            ta("contraste-sur-couleur", &[("n", numero(i))]),
            t.palette[i],
        )
    };

    verifier((tr("contraste-texte"), t.texte), fond(), MIN_TEXTE);
    verifier(
        (tr("contraste-texte-selectionne"), t.selection_texte),
        (tr("contraste-la-selection"), t.selection_fond),
        MIN_TEXTE,
    );
    verifier((tr("contraste-curseur"), t.curseur), fond(), MIN_CURSEUR);

    for (i, role) in t.roles.iter().enumerate() {
        if matches!(role, Role::Texte | Role::TexteEtFond) {
            verifier(couleur(i), fond(), MIN_TEXTE);
        }
    }
    let indices = |r: Role| (0..16).filter(move |&i| t.roles[i] == r);
    for i in indices(Role::TexteEtFond) {
        for j in indices(Role::Fond) {
            verifier(couleur(i), sur_couleur(j), MIN_TEXTE);
        }
    }
    for j in indices(Role::Fond) {
        for i in indices(Role::Texte) {
            verifier(couleur(i), sur_couleur(j), MIN_TEXTE);
        }
    }
    problemes
}
