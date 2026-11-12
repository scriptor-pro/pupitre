// SPDX-License-Identifier: GPL-3.0-or-later
//! Notifications : indicateurs d'onglet (✓, ✗, ●) et bulles du bureau.
//!
//! Tout est silencieux. Une bulle est toujours doublée d'un indicateur d'onglet ou d'un
//! message dans la fenêtre ; le mode « Ne pas déranger » est celui du bureau (aucune urgence
//! « critique »). Les bulles passent par les notifications de GIO (locales, sans réseau).

use gtk::gio;
use gtk::prelude::*;
use pupitre_i18n::{ta, ArgI18n};
use pupitre_proto::SessionId;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Au plus une bulle par onglet pendant cet intervalle.
pub const INTERVALLE_BULLES: Duration = Duration::from_secs(10);

/// Une commande n'est signalée par une bulle qu'au-delà de cette durée (en millisecondes).
pub const SEUIL_BULLE_COMMANDE_MS: u64 = 10_000;

/// Icône de la bulle : celle de l'application (voir `scripts/installer-bureau.sh`).
const ICONE: &str = "fr.pupitre.Pupitre";

/// Ce qu'un onglet non affiché signale à l'utilisateur.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Indicateur {
    #[default]
    Aucun,
    /// Commande finie avec succès (✓).
    Reussi,
    /// Commande finie en échec (✗).
    Echec,
    /// Notification d'un programme ou cloche (●).
    Signale,
}

impl Indicateur {
    /// Marque placée devant le titre de l'onglet (vide pour `Aucun`).
    pub fn marque(self) -> &'static str {
        match self {
            Indicateur::Aucun => "",
            Indicateur::Reussi => "✓ ",
            Indicateur::Echec => "✗ ",
            Indicateur::Signale => "● ",
        }
    }

    /// Indicateur dont `texte` porte la marque, et le texte sans la marque.
    pub fn separer(texte: &str) -> (Indicateur, &str) {
        for i in [Indicateur::Reussi, Indicateur::Echec, Indicateur::Signale] {
            if let Some(reste) = texte.strip_prefix(i.marque()) {
                return (i, reste);
            }
        }
        (Indicateur::Aucun, texte)
    }

    /// Nom accessible de l'onglet : « titre, commande réussie », « titre, commande en
    /// échec », « titre, notification » (« titre, cloche » pour la cloche, voir `Onglet`).
    pub fn nom_accessible(self, titre: &str) -> String {
        let cle = match self {
            Indicateur::Aucun => return titre.to_string(),
            Indicateur::Reussi => "onglet-nom-reussi",
            Indicateur::Echec => "onglet-nom-echec",
            Indicateur::Signale => "onglet-nom-notification",
        };
        ta(cle, &[("titre", ArgI18n::from(titre))])
    }

    /// Infobulle de l'étiquette de l'onglet.
    pub fn infobulle(self, titre: &str) -> String {
        let cle = match self {
            Indicateur::Aucun => return titre.to_string(),
            Indicateur::Reussi => "onglet-infobulle-reussi",
            Indicateur::Echec => "onglet-infobulle-echec",
            Indicateur::Signale => "onglet-infobulle-notification",
        };
        ta(cle, &[("titre", ArgI18n::from(titre))])
    }
}

/// Une bulle de bureau à envoyer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bulle {
    /// Identifiant de la notification : `onglet-<session>` (une nouvelle bulle du même
    /// onglet remplace la précédente).
    pub id: String,
    pub session: SessionId,
    pub titre: String,
    pub corps: String,
}

/// Fonction d'envoi des bulles (remplaçable par les tests).
pub type EnvoiBulle = Rc<dyn Fn(&Bulle)>;

/// Une bulle est permise si aucune n'est partie de cet onglet depuis `INTERVALLE_BULLES`.
pub fn bulle_permise(dernier: Option<Instant>, maintenant: Instant) -> bool {
    dernier.is_none_or(|d| {
        maintenant
            .checked_duration_since(d)
            .is_none_or(|ecoule| ecoule >= INTERVALLE_BULLES)
    })
}

/// Identifiant de la bulle d'un onglet.
pub fn id_bulle(session: SessionId) -> String {
    format!("onglet-{session}")
}

/// Envoi réel : notification GIO, avec pour action par défaut `app.montrer-onglet` (le clic
/// sur la bulle montre la fenêtre et l'onglet).
pub fn envoi_reel(app: &gtk::Application) -> EnvoiBulle {
    let app = app.downgrade();
    Rc::new(move |b: &Bulle| {
        let Some(app) = app.upgrade() else { return };
        let n = gio::Notification::new(&b.titre);
        n.set_body(Some(&b.corps));
        n.set_icon(&gio::ThemedIcon::new(ICONE));
        n.set_priority(gio::NotificationPriority::Normal);
        n.set_default_action_and_target_value("app.montrer-onglet", Some(&b.session.to_variant()));
        app.send_notification(Some(&b.id), &n);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bulle_permise_intervalle() {
        let t0 = Instant::now();
        assert!(bulle_permise(None, t0), "première bulle");
        assert!(!bulle_permise(Some(t0), t0), "même instant");
        assert!(!bulle_permise(Some(t0), t0 + Duration::from_secs(9)));
        assert!(!bulle_permise(
            Some(t0),
            t0 + INTERVALLE_BULLES - Duration::from_millis(1)
        ));
        assert!(bulle_permise(Some(t0), t0 + INTERVALLE_BULLES));
        assert!(bulle_permise(Some(t0), t0 + Duration::from_secs(60)));
        // Horloge qui « recule » (instant antérieur) : pas de plantage, bulle permise.
        assert!(bulle_permise(Some(t0 + Duration::from_secs(5)), t0));
    }

    #[test]
    fn marques_separees() {
        assert_eq!(
            Indicateur::separer("✓ essai"),
            (Indicateur::Reussi, "essai")
        );
        assert_eq!(Indicateur::separer("✗ essai"), (Indicateur::Echec, "essai"));
        assert_eq!(
            Indicateur::separer("● essai"),
            (Indicateur::Signale, "essai")
        );
        assert_eq!(Indicateur::separer("essai"), (Indicateur::Aucun, "essai"));
    }

    #[test]
    fn identifiant_par_onglet() {
        assert_eq!(id_bulle(7), "onglet-7");
    }
}
