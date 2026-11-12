// SPDX-License-Identifier: GPL-3.0-or-later
//! Disposition du clavier : bandeau d'une ligne dans chaque fenêtre, et annonce au lecteur
//! d'écran de chaque changement (même quand le bandeau est masqué). Un seul
//! [`SuiviDisposition`] par application : il écoute le clavier du siège par défaut et met à
//! jour le bandeau de toutes les fenêtres.
//!
//! GDK donne les noms lisibles des dispositions (« French (Dvorak) ») et l'indice de la
//! disposition active. `notify::layout-names` arrive en rafale sans changement réel : seul
//! un changement du nom de la disposition active est annoncé. Sans clavier (Weston sans
//! écran) ou avec une disposition illisible, le bandeau reste caché, sans erreur.

use crate::messages::annoncer;
use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
pub use pupitre_config::BandeauClavier;
use pupitre_i18n::{t, ta, ArgI18n};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

/// Abréviations des langues (premier mot du nom de disposition), pour le mode « code ».
const CODES: &[(&str, &str)] = &[
    ("French", "fr"),
    ("English", "en"),
    ("German", "de"),
    ("Spanish", "es"),
    ("Italian", "it"),
    ("Dutch", "nl"),
    ("Portuguese", "pt"),
    ("Catalan", "ca"),
    ("Swedish", "sv"),
    ("Danish", "da"),
    ("Norwegian", "no"),
    ("Finnish", "fi"),
    ("Polish", "pl"),
    ("Czech", "cs"),
    ("Russian", "ru"),
    ("Greek", "el"),
    ("Turkish", "tr"),
    ("Japanese", "ja"),
    ("Korean", "ko"),
    ("Arabic", "ar"),
    ("Hebrew", "he"),
];

/// Nombre maximal d'annonces gardées pour les tests.
const ANNONCES_GARDEES: usize = 32;

/// Texte du bandeau pour la disposition `actif` de `noms`. `None` : rien à montrer (mode
/// masqué, indice hors de la liste, nom vide).
///
/// Mode `Nom` : le nom tel que donné par GTK. Mode `Code` : le mot avant la parenthèse
/// abrégé par `CODES`, suivi de la variante entre parenthèses en minuscules (« fr dvorak ») ;
/// une langue absente de la table garde son nom complet.
pub fn texte_disposition(noms: &[String], actif: i32, mode: BandeauClavier) -> Option<String> {
    if mode == BandeauClavier::Masque {
        return None;
    }
    let nom = noms.get(usize::try_from(actif).ok()?)?.trim();
    if nom.is_empty() {
        return None;
    }
    if mode == BandeauClavier::Nom {
        return Some(nom.to_owned());
    }
    let (mot, variante) = match nom.split_once('(') {
        Some((mot, reste)) => (mot.trim(), Some(reste.trim_end_matches(')').trim())),
        None => (nom, None),
    };
    let Some((_, code)) = CODES.iter().find(|(m, _)| *m == mot) else {
        return Some(nom.to_owned());
    };
    match variante.filter(|v| !v.is_empty()) {
        Some(v) => Some(format!("{code} {}", v.to_lowercase())),
        None => Some((*code).to_owned()),
    }
}

/// Suivi unique de la disposition du clavier pour toute l'application.
pub struct SuiviDisposition {
    app: gtk::Application,
    mode: Cell<BandeauClavier>,
    noms: RefCell<Vec<String>>,
    actif: Cell<i32>,
    etiquettes: RefCell<Vec<glib::WeakRef<gtk::Label>>>,
    /// Une écoute est posée sur un clavier (évite d'en poser deux).
    branche: Cell<bool>,
    /// Attente de l'arrivée d'un clavier : siège et écoute de `device-added` (une seule).
    attente: RefCell<Option<(gdk::Seat, glib::SignalHandlerId)>>,
    /// Dernières annonces faites, pour les tests.
    annonces: RefCell<Vec<String>>,
}

impl SuiviDisposition {
    /// Lit la disposition actuelle (sans l'annoncer) et se met à l'écoute des changements.
    pub fn new(app: &gtk::Application, mode: BandeauClavier) -> Rc<Self> {
        let s = Rc::new(SuiviDisposition {
            app: app.clone(),
            mode: Cell::new(mode),
            noms: RefCell::new(Vec::new()),
            actif: Cell::new(-1),
            etiquettes: RefCell::new(Vec::new()),
            branche: Cell::new(false),
            attente: RefCell::new(None),
            annonces: RefCell::new(Vec::new()),
        });
        s.brancher();
        s
    }

    /// Clavier du siège par défaut ; `None` sans clavier (ou sans écran).
    fn clavier() -> Option<gdk::Device> {
        gdk::Display::default()?.default_seat()?.keyboard()
    }

    /// Se met à l'écoute du clavier s'il y en a un ; sinon attend l'arrivée d'un clavier.
    fn brancher(self: &Rc<Self>) {
        if self.branche.get() {
            return;
        }
        let Some(clavier) = Self::clavier() else {
            if self.attente.borrow().is_none() {
                if let Some(siege) = gdk::Display::default().and_then(|d| d.default_seat()) {
                    let faible = Rc::downgrade(self);
                    let id = siege.connect_device_added(move |_, _| {
                        if let Some(s) = faible.upgrade() {
                            s.brancher();
                        }
                    });
                    *self.attente.borrow_mut() = Some((siege, id));
                }
            }
            return;
        };
        self.branche.set(true);
        // Clavier trouvé : plus besoin d'attendre (l'emprunt est relâché avant la déconnexion).
        let attente = self.attente.borrow_mut().take();
        if let Some((siege, id)) = attente {
            siege.disconnect(id);
        }
        self.lire(&clavier, false);
        let faible: Weak<Self> = Rc::downgrade(self);
        clavier.connect_layout_names_notify({
            let faible = faible.clone();
            move |c| {
                if let Some(s) = faible.upgrade() {
                    s.lire(c, true);
                }
            }
        });
        clavier.connect_active_layout_index_notify(move |c| {
            if let Some(s) = faible.upgrade() {
                s.lire(c, true);
            }
        });
    }

    fn lire(&self, clavier: &gdk::Device, annoncer: bool) {
        let noms = clavier
            .layout_names()
            .iter()
            .map(|n| n.to_string())
            .collect();
        self.mettre_a_jour(noms, clavier.active_layout_index(), annoncer);
    }

    /// Nouvelle disposition : met à jour les bandeaux et annonce le changement réel du nom
    /// (jamais d'annonce si le nom ne change pas, si la disposition est illisible, ni si
    /// aucune disposition n'était encore connue : GDK donne parfois une liste vide au
    /// démarrage, puis les noms).
    fn mettre_a_jour(&self, noms: Vec<String>, actif: i32, annoncer: bool) {
        let avant = texte_disposition(&self.noms.borrow(), self.actif.get(), BandeauClavier::Nom);
        let apres = texte_disposition(&noms, actif, BandeauClavier::Nom);
        *self.noms.borrow_mut() = noms;
        self.actif.set(actif);
        self.rafraichir();
        if let (true, true, Some(nom), true) = (annoncer, avant.is_some(), &apres, apres != avant) {
            self.annoncer(nom);
        }
    }

    /// Annonce dans la fenêtre active (sinon la première fenêtre de l'application).
    fn annoncer(&self, nom: &str) {
        let texte = ta("disposition-annonce", &[("nom", ArgI18n::from(nom))]);
        let cible = self
            .app
            .active_window()
            .filter(|w| w.is::<gtk::ApplicationWindow>())
            .or_else(|| {
                self.app
                    .windows()
                    .into_iter()
                    .find(|w| w.is::<gtk::ApplicationWindow>())
            });
        if let Some(w) = cible {
            annoncer(&w, &texte);
        }
        let mut a = self.annonces.borrow_mut();
        if a.len() >= ANNONCES_GARDEES {
            a.remove(0);
        }
        a.push(texte);
    }

    /// Texte montré dans les bandeaux, selon le mode.
    pub fn texte(&self) -> Option<String> {
        texte_disposition(&self.noms.borrow(), self.actif.get(), self.mode.get())
    }

    /// Change le mode d'affichage (rechargement de la configuration).
    pub fn definir_mode(&self, mode: BandeauClavier) {
        if self.mode.replace(mode) != mode {
            self.rafraichir();
        }
    }

    /// Crée le bandeau d'une fenêtre : une ligne de texte, indicateur seul (ni focus ni
    /// action), rôle « status ». Suivi tant que la fenêtre vit.
    pub fn creer_bandeau(&self) -> gtk::Label {
        let etiquette = gtk::Label::builder()
            .accessible_role(gtk::AccessibleRole::Status)
            .xalign(0.0)
            .margin_start(12)
            .margin_end(12)
            .margin_top(2)
            .margin_bottom(2)
            .focusable(false)
            .can_focus(false)
            .selectable(false)
            .visible(false)
            .build();
        etiquette.set_tooltip_text(Some(&t("disposition-aide")));
        self.appliquer(&etiquette);
        self.etiquettes.borrow_mut().push(etiquette.downgrade());
        etiquette
    }

    fn appliquer(&self, etiquette: &gtk::Label) {
        match self.texte() {
            Some(texte) => {
                etiquette.set_label(&texte);
                etiquette.set_visible(true);
            }
            None => etiquette.set_visible(false),
        }
    }

    fn rafraichir(&self) {
        // Aucun emprunt pendant les appels GTK : ils peuvent ré-entrer.
        let vivantes: Vec<gtk::Label> = {
            let mut v = self.etiquettes.borrow_mut();
            v.retain(|w| w.upgrade().is_some());
            v.iter().filter_map(|w| w.upgrade()).collect()
        };
        for e in vivantes {
            self.appliquer(&e);
        }
    }

    /// Dernières annonces faites, pour les tests.
    pub fn annonces(&self) -> Vec<String> {
        self.annonces.borrow().clone()
    }

    /// Simule un changement de disposition (Xvfb ignore les vrais changements).
    pub fn changer_pour_tests(&self, noms: &[&str], actif: i32) {
        let noms = noms.iter().map(|n| (*n).to_owned()).collect();
        self.mettre_a_jour(noms, actif, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noms(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn texte_disposition_cas() {
        let n = noms(&["French (Dvorak)", "English (US)", "German", "Esperanto"]);
        // Nom : tel que donné par GTK.
        assert_eq!(
            texte_disposition(&n, 0, BandeauClavier::Nom).as_deref(),
            Some("French (Dvorak)")
        );
        assert_eq!(
            texte_disposition(&n, 2, BandeauClavier::Nom).as_deref(),
            Some("German")
        );
        // Code : mot abrégé, variante en minuscules.
        assert_eq!(
            texte_disposition(&n, 0, BandeauClavier::Code).as_deref(),
            Some("fr dvorak")
        );
        assert_eq!(
            texte_disposition(&n, 1, BandeauClavier::Code).as_deref(),
            Some("en us")
        );
        assert_eq!(
            texte_disposition(&n, 2, BandeauClavier::Code).as_deref(),
            Some("de")
        );
        // Langue absente de la table : le nom.
        assert_eq!(
            texte_disposition(&n, 3, BandeauClavier::Code).as_deref(),
            Some("Esperanto")
        );
        // Masqué : rien à afficher.
        assert_eq!(texte_disposition(&n, 0, BandeauClavier::Masque), None);
        // Index hors bornes, liste vide, nom vide : rien, sans erreur.
        assert_eq!(texte_disposition(&n, -1, BandeauClavier::Nom), None);
        assert_eq!(texte_disposition(&n, 4, BandeauClavier::Nom), None);
        assert_eq!(texte_disposition(&[], 0, BandeauClavier::Nom), None);
        assert_eq!(
            texte_disposition(&noms(&["  "]), 0, BandeauClavier::Nom),
            None
        );
    }

    #[test]
    fn codes_des_langues_courantes() {
        for (nom, code) in [
            ("French", "fr"),
            ("English (UK)", "en uk"),
            ("German", "de"),
            ("Spanish", "es"),
            ("Italian", "it"),
            ("Dutch", "nl"),
            ("Portuguese (Brazil)", "pt brazil"),
        ] {
            assert_eq!(
                texte_disposition(&noms(&[nom]), 0, BandeauClavier::Code).as_deref(),
                Some(code),
                "{nom}"
            );
        }
    }
}
