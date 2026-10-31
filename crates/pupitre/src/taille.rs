// SPDX-License-Identifier: GPL-3.0-or-later
//! Taille de la fenêtre à l'ouverture : moitié de la largeur de l'écran, hauteur au nombre d'or.

use gtk::prelude::*;

/// Nombre d'or : rapport largeur / hauteur de la fenêtre à l'ouverture.
const NOMBRE_D_OR: f64 = 1.618;
/// Largeur d'écran supposée si aucun écran n'est connu (ou s'il annonce une taille absurde).
const LARGEUR_PAR_DEFAUT: i32 = 1920;
/// Plus petite largeur d'écran prise en compte.
const LARGEUR_MIN: i32 = 640;

/// `(largeur, hauteur)` de la fenêtre pour un écran large de `largeur_ecran` pixels logiques.
pub fn taille_ouverture(largeur_ecran: i32) -> (i32, i32) {
    let l = if largeur_ecran < LARGEUR_MIN {
        LARGEUR_PAR_DEFAUT
    } else {
        largeur_ecran
    };
    let largeur = f64::from(l) * 0.5;
    (
        largeur.round() as i32,
        (largeur / NOMBRE_D_OR).round() as i32,
    )
}

/// Largeur (pixels logiques) du premier écran connu de l'affichage par défaut.
/// Sous Wayland, l'écran de la future fenêtre n'est pas connu avant son affichage :
/// le premier écran est la meilleure estimation.
pub fn largeur_ecran() -> Option<i32> {
    let affichage = gtk::gdk::Display::default()?;
    let ecrans = affichage.monitors();
    let ecran = ecrans.item(0)?.downcast::<gtk::gdk::Monitor>().ok()?;
    Some(ecran.geometry().width())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taille_nombre_d_or() {
        assert_eq!(taille_ouverture(1920), (960, 593));
        assert_eq!(taille_ouverture(2560), (1280, 791));
    }

    #[test]
    fn ecran_absurde_donne_la_taille_par_defaut() {
        assert_eq!(taille_ouverture(0), (960, 593));
        assert_eq!(taille_ouverture(-5), (960, 593));
    }
}
