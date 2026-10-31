// SPDX-License-Identifier: GPL-3.0-or-later
//! Préférence clair/sombre du bureau, lue par le portail freedesktop
//! (`org.freedesktop.portal.Settings`, D-Bus local) et suivie en direct.

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use pupitre_config::{theme_clair, theme_sombre, Theme};
use std::rc::Rc;

const PORTAIL_NOM: &str = "org.freedesktop.portal.Desktop";
const PORTAIL_CHEMIN: &str = "/org/freedesktop/portal/desktop";
const PORTAIL_INTERFACE: &str = "org.freedesktop.portal.Settings";
const ESPACE: &str = "org.freedesktop.appearance";
const CLE: &str = "color-scheme";
/// Délai de réponse du portail ; au-delà, on garde le thème sombre.
const DELAI_MS: i32 = 3000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preference {
    Sombre,
    Clair,
    /// Portail absent, muet ou « sans préférence ».
    SansPreference,
}

impl Preference {
    /// Valeur du portail : 1 = sombre, 2 = clair, autre = sans préférence.
    pub fn depuis_portail(v: u32) -> Self {
        match v {
            1 => Preference::Sombre,
            2 => Preference::Clair,
            _ => Preference::SansPreference,
        }
    }
}

/// Thème intégré pour une préférence du bureau (sans préférence → sombre).
pub fn theme_systeme(p: Preference) -> Theme {
    match p {
        Preference::Clair => theme_clair(),
        Preference::Sombre | Preference::SansPreference => theme_sombre(),
    }
}

/// Extrait un `u32` d'une valeur du portail, éventuellement emballée dans des variants.
fn valeur_u32(v: &glib::Variant) -> Option<u32> {
    let mut v = v.clone();
    for _ in 0..4 {
        if let Some(n) = v.get::<u32>() {
            return Some(n);
        }
        v = v.as_variant()?;
    }
    None
}

/// Appelle `sur_changement` avec la préférence actuelle, puis à chaque changement.
/// Ne bloque jamais : tout est asynchrone sur la boucle GTK. Sans bus ou sans portail,
/// `sur_changement(SansPreference)` est appelé une fois.
pub fn suivre_preference_systeme(sur_changement: impl Fn(Preference) + 'static) {
    let rappel: Rc<dyn Fn(Preference)> = Rc::new(sur_changement);
    gio::bus_get(gio::BusType::Session, gio::Cancellable::NONE, move |bus| {
        let connexion = match bus {
            Ok(c) => c,
            Err(_) => return rappel(Preference::SansPreference),
        };
        // Abonnement d'abord : aucun changement ne peut être manqué entre lecture et écoute.
        let r = rappel.clone();
        let abonnement = connexion.subscribe_to_signal(
            Some(PORTAIL_NOM),
            Some(PORTAIL_INTERFACE),
            Some("SettingChanged"),
            Some(PORTAIL_CHEMIN),
            None,
            gio::DBusSignalFlags::NONE,
            move |signal| {
                let params = signal.parameters;
                if let (Some(ns), Some(cle), Some(v)) = (
                    params.try_child_value(0).and_then(|v| v.get::<String>()),
                    params.try_child_value(1).and_then(|v| v.get::<String>()),
                    params.try_child_value(2),
                ) {
                    if ns == ESPACE && cle == CLE {
                        r(valeur_u32(&v)
                            .map_or(Preference::SansPreference, Preference::depuis_portail));
                    }
                }
            },
        );
        // L'abonnement doit durer autant que le processus : le libérer le résilierait.
        std::mem::forget(abonnement);
        let r = rappel.clone();
        connexion.call(
            Some(PORTAIL_NOM),
            PORTAIL_CHEMIN,
            PORTAIL_INTERFACE,
            "ReadOne",
            Some(&(ESPACE, CLE).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            DELAI_MS,
            gio::Cancellable::NONE,
            move |rep| {
                let p = rep
                    .ok()
                    .and_then(|v| v.try_child_value(0))
                    .and_then(|v| valeur_u32(&v))
                    .map_or(Preference::SansPreference, Preference::depuis_portail);
                r(p);
            },
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valeurs_du_portail() {
        assert_eq!(Preference::depuis_portail(1), Preference::Sombre);
        assert_eq!(Preference::depuis_portail(2), Preference::Clair);
        assert_eq!(Preference::depuis_portail(0), Preference::SansPreference);
        assert_eq!(Preference::depuis_portail(77), Preference::SansPreference);
    }

    #[test]
    fn sans_preference_donne_le_sombre() {
        assert_eq!(theme_systeme(Preference::SansPreference).nom, "sombre");
        assert_eq!(theme_systeme(Preference::Clair).nom, "clair");
    }

    #[test]
    fn valeur_emballee() {
        let v = 2u32.to_variant().to_variant().to_variant();
        assert_eq!(valeur_u32(&v), Some(2));
        assert_eq!(valeur_u32(&"x".to_variant()), None);
    }
}
