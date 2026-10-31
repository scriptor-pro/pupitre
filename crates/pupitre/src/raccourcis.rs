// SPDX-License-Identifier: GPL-3.0-or-later
//! Raccourcis clavier de Pupitre (conception §12.1).
//!
//! Reconnus d'abord par le caractère produit (Ctrl+Maj+T là où se trouve « T »), avec repli
//! sur la position physique pour Alt+1…9 et Ctrl+0 : en fr dvorak par exemple, la rangée
//! du haut donne « = / - è \ ^ ( … » sans Maj ; Alt+« 2 » arrive donc en Alt+« / ».

use gtk::gdk;

/// Codes matériels (evdev + 8, identiques sous X11 et Wayland) des touches 1 à 0 de la
/// rangée du haut.
const CODE_TOUCHE_1: u32 = 10;
const CODE_TOUCHE_9: u32 = 18;
const CODE_TOUCHE_0: u32 = 19;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Raccourci {
    NouvelOnglet,
    FermerOnglet,
    NouvelleFenetre,
    Copier,
    Coller,
    Rechercher,
    OngletSuivant,
    OngletPrecedent,
    /// Onglet numéro 1 à 9.
    Onglet(u32),
    PleinEcran,
    ZoomPlus,
    ZoomMoins,
    ZoomNormal,
    /// Ctrl+Tab : quitter le terminal (qui garde Tab pour le shell) vers l'élément suivant.
    FocusSuivant,
    /// Ctrl+Maj+Tab : élément précédent.
    FocusPrecedent,
}

/// Raccourci correspondant à une touche (`code` = code matériel), s'il y en a un.
pub fn raccourci(touche: gdk::Key, code: u32, mods: gdk::ModifierType) -> Option<Raccourci> {
    use gdk::ModifierType as M;
    let utiles = M::CONTROL_MASK | M::SHIFT_MASK | M::ALT_MASK | M::SUPER_MASK | M::META_MASK;
    let m = mods & utiles;
    let ctrl = M::CONTROL_MASK;
    let ctrl_maj = M::CONTROL_MASK | M::SHIFT_MASK;
    let zoom = |t: gdk::Key| match t {
        gdk::Key::plus | gdk::Key::equal | gdk::Key::KP_Add => Some(Raccourci::ZoomPlus),
        gdk::Key::minus | gdk::Key::KP_Subtract => Some(Raccourci::ZoomMoins),
        gdk::Key::_0 | gdk::Key::KP_0 => Some(Raccourci::ZoomNormal),
        _ => None,
    };
    if m == ctrl_maj {
        if matches!(touche, gdk::Key::Tab | gdk::Key::ISO_Left_Tab) {
            return Some(Raccourci::FocusPrecedent);
        }
        return match touche.to_lower() {
            gdk::Key::t => Some(Raccourci::NouvelOnglet),
            gdk::Key::w => Some(Raccourci::FermerOnglet),
            gdk::Key::n => Some(Raccourci::NouvelleFenetre),
            gdk::Key::c => Some(Raccourci::Copier),
            gdk::Key::v => Some(Raccourci::Coller),
            gdk::Key::f => Some(Raccourci::Rechercher),
            // « + » et « 0 » demandent Maj sur certaines dispositions (fr, fr dvorak).
            t => zoom(t),
        };
    }
    if m == ctrl {
        return match touche {
            gdk::Key::Page_Down | gdk::Key::KP_Page_Down => Some(Raccourci::OngletSuivant),
            gdk::Key::Page_Up | gdk::Key::KP_Page_Up => Some(Raccourci::OngletPrecedent),
            gdk::Key::Tab => Some(Raccourci::FocusSuivant),
            gdk::Key::ISO_Left_Tab => Some(Raccourci::FocusPrecedent),
            t => zoom(t).or((code == CODE_TOUCHE_0).then_some(Raccourci::ZoomNormal)),
        };
    }
    if m == M::ALT_MASK {
        let par_caractere = touche
            .to_unicode()
            .and_then(|c| c.to_digit(10))
            .filter(|n| (1..=9).contains(n));
        let par_position = (CODE_TOUCHE_1..=CODE_TOUCHE_9)
            .contains(&code)
            .then(|| code - CODE_TOUCHE_1 + 1);
        return par_caractere.or(par_position).map(Raccourci::Onglet);
    }
    if m.is_empty() && touche == gdk::Key::F11 {
        return Some(Raccourci::PleinEcran);
    }
    None
}

/// Intercepte les raccourcis de la fenêtre avant le terminal (sinon VTE consomme les
/// touches et les envoie au programme).
pub fn installer_raccourcis(fenetre: &crate::fenetre::FenetrePupitre) {
    use gtk::prelude::*;
    let touches = gtk::EventControllerKey::new();
    touches.set_propagation_phase(gtk::PropagationPhase::Capture);
    // Référence faible : le contrôleur appartient à la fenêtre (pas de cycle).
    let faible = fenetre.faible();
    touches.connect_key_pressed(move |_, touche, code, mods| {
        let f = crate::fenetre::FenetrePupitre::depuis_faible(&faible);
        if f.is_some_and(|f| f.traiter_touche(touche, code, mods)) {
            gtk::glib::Propagation::Stop
        } else {
            gtk::glib::Propagation::Proceed
        }
    });
    fenetre.fenetre().add_controller(touches);
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdk::ModifierType as M;

    const CS: M = M::CONTROL_MASK.union(M::SHIFT_MASK);

    #[test]
    fn ctrl_maj_lettres() {
        assert_eq!(
            raccourci(gdk::Key::T, 28, CS),
            Some(Raccourci::NouvelOnglet)
        );
        assert_eq!(
            raccourci(gdk::Key::w, 25, CS),
            Some(Raccourci::FermerOnglet)
        );
        assert_eq!(
            raccourci(gdk::Key::N, 57, CS),
            Some(Raccourci::NouvelleFenetre)
        );
        assert_eq!(raccourci(gdk::Key::C, 54, CS), Some(Raccourci::Copier));
        assert_eq!(raccourci(gdk::Key::V, 55, CS), Some(Raccourci::Coller));
        assert_eq!(raccourci(gdk::Key::F, 41, CS), Some(Raccourci::Rechercher));
        // Ctrl seul : laissé au programme (Ctrl+C interrompt, Ctrl+W efface un mot…).
        assert_eq!(raccourci(gdk::Key::c, 54, M::CONTROL_MASK), None);
        assert_eq!(raccourci(gdk::Key::w, 25, M::CONTROL_MASK), None);
        // Verrouillage majuscules et pavé numérique ignorés ; Alt en plus refusé.
        let verr = CS | M::LOCK_MASK;
        assert_eq!(
            raccourci(gdk::Key::T, 28, verr),
            Some(Raccourci::NouvelOnglet)
        );
        assert_eq!(raccourci(gdk::Key::T, 28, CS | M::ALT_MASK), None);
    }

    #[test]
    fn alt_chiffres_qwerty_et_azerty() {
        // QWERTY : Alt+2 donne « 2 ».
        assert_eq!(
            raccourci(gdk::Key::_2, 11, M::ALT_MASK),
            Some(Raccourci::Onglet(2))
        );
        // AZERTY : Alt+« é » (touche 2) → repli sur la position.
        assert_eq!(
            raccourci(gdk::Key::eacute, 11, M::ALT_MASK),
            Some(Raccourci::Onglet(2))
        );
        assert_eq!(raccourci(gdk::Key::_0, 19, M::ALT_MASK), None);
    }

    #[test]
    fn alt_chiffres_fr_dvorak() {
        // fr dvorak, sans Maj : = / - è \ ^ ( ` ) sur les touches 1 à 9.
        let rangee = [
            gdk::Key::equal,
            gdk::Key::slash,
            gdk::Key::minus,
            gdk::Key::egrave,
            gdk::Key::backslash,
            gdk::Key::dead_circumflex,
            gdk::Key::parenleft,
            gdk::Key::ISO_Level3_Latch,
            gdk::Key::parenright,
        ];
        for (i, t) in rangee.into_iter().enumerate() {
            let n = i as u32 + 1;
            assert_eq!(
                raccourci(t, CODE_TOUCHE_1 + i as u32, M::ALT_MASK),
                Some(Raccourci::Onglet(n)),
                "touche {n}"
            );
        }
    }

    #[test]
    fn zoom() {
        let c = M::CONTROL_MASK;
        assert_eq!(raccourci(gdk::Key::plus, 21, c), Some(Raccourci::ZoomPlus));
        assert_eq!(
            raccourci(gdk::Key::KP_Add, 86, c),
            Some(Raccourci::ZoomPlus)
        );
        assert_eq!(
            raccourci(gdk::Key::minus, 20, c),
            Some(Raccourci::ZoomMoins)
        );
        assert_eq!(raccourci(gdk::Key::_0, 19, c), Some(Raccourci::ZoomNormal));
        // fr dvorak : « + » et « 0 » demandent Maj ; Ctrl seul sur la touche 0 donne « " ».
        assert_eq!(raccourci(gdk::Key::plus, 20, CS), Some(Raccourci::ZoomPlus));
        assert_eq!(raccourci(gdk::Key::_0, 19, CS), Some(Raccourci::ZoomNormal));
        assert_eq!(
            raccourci(gdk::Key::quotedbl, 19, c),
            Some(Raccourci::ZoomNormal)
        );
    }

    #[test]
    fn sortir_du_terminal() {
        let c = M::CONTROL_MASK;
        assert_eq!(
            raccourci(gdk::Key::Tab, 23, c),
            Some(Raccourci::FocusSuivant)
        );
        assert_eq!(
            raccourci(gdk::Key::ISO_Left_Tab, 23, CS),
            Some(Raccourci::FocusPrecedent)
        );
        assert_eq!(
            raccourci(gdk::Key::Tab, 23, CS),
            Some(Raccourci::FocusPrecedent)
        );
        // Tab seul reste au shell (complétion).
        assert_eq!(raccourci(gdk::Key::Tab, 23, M::empty()), None);
    }

    #[test]
    fn onglets_et_plein_ecran() {
        let c = M::CONTROL_MASK;
        assert_eq!(
            raccourci(gdk::Key::Page_Down, 117, c),
            Some(Raccourci::OngletSuivant)
        );
        assert_eq!(
            raccourci(gdk::Key::Page_Up, 112, c),
            Some(Raccourci::OngletPrecedent)
        );
        assert_eq!(raccourci(gdk::Key::Page_Down, 117, M::empty()), None);
        assert_eq!(
            raccourci(gdk::Key::F11, 95, M::empty()),
            Some(Raccourci::PleinEcran)
        );
        assert_eq!(raccourci(gdk::Key::F11, 95, c), None);
    }
}
