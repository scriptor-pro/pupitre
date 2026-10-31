// SPDX-License-Identifier: GPL-3.0-or-later
//! Texte atténué (SGR 2). VTE l'affiche en multipliant la couleur du texte par 2/3 (mesuré,
//! VTE 0.80). Si cela fait passer une couleur de texte du thème sous 4,5:1, l'atténuation
//! est désactivée : le paramètre 2 est retiré des séquences SGR avant `feed()` et le texte
//! s'affiche dans sa couleur normale (conception, section 7).

use pupitre_config::{contrast_ratio, Rgb, Role, Theme};

/// Contraste minimum du texte (WCAG AA).
const MIN_TEXTE: f64 = 4.5;
/// Au-delà, une séquence CSI est transmise telle quelle sans être analysée.
const CSI_MAX: usize = 256;
const ESC: u8 = 0x1b;

/// Couleur rendue par VTE pour un texte atténué.
pub fn couleur_attenuee(c: Rgb) -> Rgb {
    let f = |v: u8| ((u16::from(v) * 2 + 1) / 3) as u8;
    Rgb {
        r: f(c.r),
        g: f(c.g),
        b: f(c.b),
    }
}

/// Vrai si, atténuées, la couleur du texte et toutes les couleurs de texte de la palette
/// restent à au moins 4,5:1 du fond.
pub fn attenuation_lisible(theme: &Theme) -> bool {
    std::iter::once(theme.texte)
        .chain(
            theme
                .palette
                .iter()
                .zip(theme.roles.iter())
                .filter(|(_, &r)| r != Role::Fond)
                .map(|(&c, _)| c),
        )
        .all(|c| contrast_ratio(couleur_attenuee(c), theme.fond) >= MIN_TEXTE)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Etat {
    Texte,
    /// ESC reçu.
    Echap,
    /// Dans `ESC [` … (octets accumulés dans `en_cours`).
    Csi,
}

/// Filtre à états, sûr quel que soit le découpage du flux en morceaux.
#[derive(Debug)]
pub struct FiltreAttenue {
    actif: bool,
    etat: Etat,
    en_cours: Vec<u8>,
}

impl Default for FiltreAttenue {
    fn default() -> Self {
        FiltreAttenue {
            actif: false,
            etat: Etat::Texte,
            en_cours: Vec::new(),
        }
    }
}

impl FiltreAttenue {
    /// Active ou non le retrait de l'atténuation (prend effet pour la suite du flux).
    pub fn activer(&mut self, actif: bool) {
        self.actif = actif;
    }

    /// Ajoute à `sortie` la version filtrée de `entree`. Une séquence coupée en fin de
    /// morceau est gardée jusqu'au morceau suivant.
    pub fn filtrer(&mut self, entree: &[u8], sortie: &mut Vec<u8>) {
        if !self.actif && self.etat == Etat::Texte {
            sortie.extend_from_slice(entree);
            return;
        }
        for &o in entree {
            match self.etat {
                Etat::Texte => {
                    if o == ESC {
                        self.etat = Etat::Echap;
                    } else {
                        sortie.push(o);
                    }
                }
                Etat::Echap => {
                    if o == b'[' {
                        self.etat = Etat::Csi;
                        self.en_cours.clear();
                    } else {
                        sortie.push(ESC);
                        if o == ESC {
                            continue; // reste en Echap
                        }
                        sortie.push(o);
                        self.etat = Etat::Texte;
                    }
                }
                Etat::Csi => {
                    self.en_cours.push(o);
                    if (0x40..=0x7e).contains(&o) {
                        self.terminer_csi(sortie);
                    } else if !(0x20..=0x3f).contains(&o) || self.en_cours.len() > CSI_MAX {
                        // Octet hors séquence (ou séquence démesurée) : tout rendre tel quel.
                        self.rendre_brut(sortie);
                    }
                }
            }
        }
    }

    fn rendre_brut(&mut self, sortie: &mut Vec<u8>) {
        sortie.extend_from_slice(&[ESC, b'[']);
        sortie.append(&mut self.en_cours);
        self.etat = Etat::Texte;
    }

    fn terminer_csi(&mut self, sortie: &mut Vec<u8>) {
        let seq = std::mem::take(&mut self.en_cours);
        self.etat = Etat::Texte;
        let (params, finale) = seq.split_at(seq.len() - 1);
        let sgr_simple = finale == b"m" && params.iter().all(|o| (0x30..=0x3b).contains(o));
        if !self.actif || !sgr_simple {
            sortie.extend_from_slice(&[ESC, b'[']);
            sortie.extend_from_slice(&seq);
            return;
        }
        match sans_attenuation(params) {
            Some(p) if p.is_empty() => {} // la séquence ne faisait qu'atténuer
            Some(p) => {
                sortie.extend_from_slice(&[ESC, b'[']);
                sortie.extend_from_slice(&p);
                sortie.push(b'm');
            }
            None => {
                sortie.extend_from_slice(&[ESC, b'[']);
                sortie.extend_from_slice(&seq);
            }
        }
    }
}

/// Paramètres SGR sans le `2` (atténué). `None` = rien à changer. Les arguments des
/// couleurs étendues `38;5;n`, `38;2;r;g;b` (et 48, 58) ne sont pas pris pour des `2`.
fn sans_attenuation(params: &[u8]) -> Option<Vec<u8>> {
    let morceaux: Vec<&[u8]> = params.split(|&o| o == b';').collect();
    let mut gardes: Vec<&[u8]> = Vec::with_capacity(morceaux.len());
    let mut retire = false;
    let mut i = 0;
    while i < morceaux.len() {
        let m = morceaux[i];
        match m {
            b"38" | b"48" | b"58" => {
                let n = match morceaux.get(i + 1).copied() {
                    Some(b"5") => 2,
                    Some(b"2") => 4,
                    _ => 0,
                };
                let fin = (i + 1 + n).min(morceaux.len());
                gardes.extend_from_slice(&morceaux[i..fin]);
                i = fin;
                continue;
            }
            b"2" | b"02" => retire = true,
            _ => gardes.push(m),
        }
        i += 1;
    }
    retire.then(|| gardes.join(&b';'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pupitre_config::{theme_clair, theme_sombre};

    fn filtre(entree: &[u8]) -> Vec<u8> {
        let mut f = FiltreAttenue::default();
        f.activer(true);
        let mut s = Vec::new();
        f.filtrer(entree, &mut s);
        s
    }

    #[test]
    fn couleur_attenuee_comme_vte() {
        // Valeurs mesurées sur le rendu de VTE 0.80.
        let c = couleur_attenuee(Rgb {
            r: 0xcd,
            g: 0xd6,
            b: 0xf4,
        });
        assert_eq!((c.r, c.g, c.b), (0x89, 0x8f, 0xa3));
        let c = couleur_attenuee(Rgb {
            r: 0x8a,
            g: 0x8e,
            b: 0xa6,
        });
        assert_eq!((c.r, c.g, c.b), (0x5c, 0x5f, 0x6f));
    }

    #[test]
    fn lisibilite_par_theme() {
        assert!(!attenuation_lisible(&theme_sombre()));
        assert!(attenuation_lisible(&theme_clair()));
    }

    #[test]
    fn retire_le_seul_parametre() {
        assert_eq!(filtre(b"a\x1b[2mb\x1b[0m"), b"ab\x1b[0m");
    }

    #[test]
    fn retire_parmi_d_autres() {
        assert_eq!(filtre(b"\x1b[1;2;31m"), b"\x1b[1;31m");
        assert_eq!(filtre(b"\x1b[2;34m"), b"\x1b[34m");
        assert_eq!(filtre(b"\x1b[02;4m"), b"\x1b[4m");
    }

    #[test]
    fn couleurs_etendues_intactes() {
        assert_eq!(filtre(b"\x1b[38;2;2;2;2m"), b"\x1b[38;2;2;2;2m");
        assert_eq!(filtre(b"\x1b[48;5;2;2m"), b"\x1b[48;5;2m");
        assert_eq!(filtre(b"\x1b[38:2::2:2:2;2m"), b"\x1b[38:2::2:2:2m");
    }

    #[test]
    fn autres_sequences_intactes() {
        for s in [
            &b"\x1b[22m"[..],
            b"\x1b[12m",
            b"\x1b[2J",
            b"\x1b[?2004h",
            b"\x1b[>4;2m",
            b"\x1b]0;titre 2m\x07",
            b"\x1b(B",
            b"\x1b\x1b[0m",
            b"\x1b[m",
        ] {
            assert_eq!(filtre(s), s, "{s:?}");
        }
    }

    #[test]
    fn inactif_ne_change_rien() {
        let mut f = FiltreAttenue::default();
        let mut s = Vec::new();
        f.filtrer(b"\x1b[2mx", &mut s);
        assert_eq!(s, b"\x1b[2mx");
    }

    #[test]
    fn decoupage_quelconque() {
        let entree = b"a\x1b[1;2;38;5;2mb\x1b[2mc\x1b[0m\xc3\xa9";
        let attendu = b"a\x1b[1;38;5;2mbc\x1b[0m\xc3\xa9".to_vec();
        assert_eq!(filtre(entree), attendu);
        for coupe in 0..=entree.len() {
            let mut f = FiltreAttenue::default();
            f.activer(true);
            let mut s = Vec::new();
            f.filtrer(&entree[..coupe], &mut s);
            f.filtrer(&entree[coupe..], &mut s);
            assert_eq!(s, attendu, "coupe à {coupe}");
        }
    }

    #[test]
    fn sequence_demesuree_rendue_telle_quelle() {
        let mut e = b"\x1b[".to_vec();
        e.extend(std::iter::repeat_n(b'1', 1000));
        e.extend(b"mfin");
        assert_eq!(filtre(&e), e);
    }

    #[test]
    fn octets_quelconques_sans_panique() {
        // Petit fuzz déterministe : aucune panique, et sans aucun ESC rien ne change.
        let mut x: u32 = 12345;
        for _ in 0..2000 {
            let n = (x % 64) as usize;
            let mut e = Vec::with_capacity(n);
            for _ in 0..n {
                x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
                e.push((x >> 16) as u8);
            }
            let s = filtre(&e);
            if !e.contains(&ESC) {
                assert_eq!(s, e);
            }
        }
    }
}
