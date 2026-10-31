// SPDX-License-Identifier: GPL-3.0-or-later
//! Barre de recherche dans l'historique d'un terminal (Ctrl+Maj+F) : texte simple ou
//! expression régulière, Entrée = précédent (vers le haut), Maj+Entrée = suivant,
//! Échap ferme et rend le focus au terminal.

use crate::terminal::{PCRE2_CASELESS, PCRE2_MULTILINE, PCRE2_UTF};
use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::{Rc, Weak};
use vte4::prelude::*;

pub struct BarreRecherche {
    barre: gtk::SearchBar,
    entree: gtk::SearchEntry,
    expression: gtk::CheckButton,
    /// « Aucun résultat » quand la dernière recherche n'a rien trouvé.
    etat: gtk::Label,
    cible: RefCell<Option<glib::WeakRef<vte4::Terminal>>>,
    /// Recherche posée sur le terminal (motif, options) : la reposer à l'identique
    /// remettrait la position de recherche au départ.
    posee: RefCell<Option<(String, u32)>>,
}

impl BarreRecherche {
    pub fn new() -> Rc<Self> {
        let entree = gtk::SearchEntry::builder()
            .placeholder_text("Rechercher dans l'historique")
            .hexpand(true)
            .build();
        entree.update_property(&[gtk::accessible::Property::Label(
            "Rechercher dans l'historique",
        )]);
        let expression = gtk::CheckButton::with_label("Expression régulière");
        let precedent = gtk::Button::with_label("Précédent");
        precedent.set_tooltip_text(Some("Occurrence précédente (Entrée)"));
        let suivant = gtk::Button::with_label("Suivant");
        suivant.set_tooltip_text(Some("Occurrence suivante (Maj+Entrée)"));
        let etat = gtk::Label::new(None);
        let boite = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        boite.append(&entree);
        boite.append(&etat);
        boite.append(&expression);
        boite.append(&precedent);
        boite.append(&suivant);
        // Bouton de fermeture nommé (celui de GtkSearchBar n'a pas de nom accessible).
        let fermer = gtk::Button::from_icon_name("window-close-symbolic");
        fermer.set_tooltip_text(Some("Fermer la recherche (Échap)"));
        fermer.update_property(&[gtk::accessible::Property::Label("Fermer la recherche")]);
        boite.append(&fermer);
        let barre = gtk::SearchBar::builder()
            .child(&boite)
            .show_close_button(false)
            .build();
        barre.connect_entry(&entree);

        let b = Rc::new(BarreRecherche {
            barre,
            entree,
            expression,
            etat,
            cible: RefCell::new(None),
            posee: RefCell::new(None),
        });
        let faible = Rc::downgrade(&b);
        let avec = move |f: fn(&BarreRecherche)| {
            let faible: Weak<BarreRecherche> = faible.clone();
            move || {
                if let Some(b) = faible.upgrade() {
                    f(&b);
                }
            }
        };
        let a = avec(BarreRecherche::appliquer);
        b.entree.connect_changed(move |_| a());
        let a = avec(BarreRecherche::appliquer);
        b.expression.connect_toggled(move |_| a());
        let a = avec(BarreRecherche::precedent);
        b.entree.connect_activate(move |_| a());
        let a = avec(BarreRecherche::precedent);
        b.entree.connect_previous_match(move |_| a());
        let a = avec(BarreRecherche::suivant);
        b.entree.connect_next_match(move |_| a());
        let a = avec(BarreRecherche::precedent);
        precedent.connect_clicked(move |_| a());
        let a = avec(BarreRecherche::suivant);
        suivant.connect_clicked(move |_| a());
        let a = avec(BarreRecherche::fermer);
        b.entree.connect_stop_search(move |_| a());
        let a = avec(BarreRecherche::fermer);
        fermer.connect_clicked(move |_| a());
        // Maj+Entrée : suivant (Entrée seule passe par `activate`).
        let touches = gtk::EventControllerKey::new();
        touches.set_propagation_phase(gtk::PropagationPhase::Capture);
        let a = avec(BarreRecherche::suivant);
        touches.connect_key_pressed(move |_, t, _, m| {
            if matches!(t, gdk::Key::Return | gdk::Key::KP_Enter)
                && m.contains(gdk::ModifierType::SHIFT_MASK)
            {
                a();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        b.entree.add_controller(touches);
        b
    }

    pub fn widget(&self) -> &gtk::SearchBar {
        &self.barre
    }

    pub fn entree(&self) -> &gtk::SearchEntry {
        &self.entree
    }

    pub fn ouverte(&self) -> bool {
        self.barre.is_search_mode()
    }

    /// Ouvre la barre pour chercher dans `terminal`.
    pub fn ouvrir(&self, terminal: &vte4::Terminal) {
        *self.cible.borrow_mut() = Some(terminal.downgrade());
        *self.posee.borrow_mut() = None;
        self.barre.set_search_mode(true);
        self.entree.grab_focus();
        self.appliquer();
    }

    pub fn fermer(&self) {
        self.barre.set_search_mode(false);
        *self.posee.borrow_mut() = None;
        if let Some(t) = self.terminal() {
            t.search_set_regex(None, 0);
            t.grab_focus();
        }
    }

    pub fn expression_reguliere(&self, oui: bool) {
        self.expression.set_active(oui);
    }

    fn terminal(&self) -> Option<vte4::Terminal> {
        self.cible.borrow().as_ref().and_then(|w| w.upgrade())
    }

    /// Recompile la recherche ; une expression invalide est signalée sur la saisie.
    fn appliquer(&self) {
        let Some(t) = self.terminal() else { return };
        self.etat.set_text("");
        let texte = self.entree.text();
        if texte.is_empty() {
            self.signaler_erreur(None);
            *self.posee.borrow_mut() = None;
            t.search_set_regex(None, 0);
            return;
        }
        let motif = if self.expression.is_active() {
            texte.to_string()
        } else {
            echapper(&texte)
        };
        // Casse ignorée sauf si le texte cherché contient une majuscule.
        let mut options = PCRE2_UTF | PCRE2_MULTILINE;
        if !texte.chars().any(char::is_uppercase) {
            options |= PCRE2_CASELESS;
        }
        let cle = Some((motif.clone(), options));
        if *self.posee.borrow() == cle {
            return;
        }
        match vte4::Regex::for_search(&motif, options) {
            Ok(re) => {
                self.signaler_erreur(None);
                t.search_set_regex(Some(&re), 0);
                t.search_set_wrap_around(true);
                *self.posee.borrow_mut() = cle;
            }
            Err(e) => {
                self.signaler_erreur(Some(&format!("Expression invalide : {}", e.message())));
                *self.posee.borrow_mut() = None;
                t.search_set_regex(None, 0);
            }
        }
    }

    fn signaler_erreur(&self, message: Option<&str>) {
        match message {
            Some(m) => {
                self.entree.add_css_class("error");
                self.entree.set_tooltip_text(Some(m));
                self.entree
                    .announce(m, gtk::AccessibleAnnouncementPriority::Medium);
            }
            None => {
                self.entree.remove_css_class("error");
                self.entree.set_tooltip_text(None);
            }
        }
    }

    /// Texte d'état (« Aucun résultat » ou vide).
    pub fn etat(&self) -> String {
        self.etat.text().to_string()
    }

    fn precedent(&self) {
        self.chercher(|t| t.search_find_previous());
    }

    fn suivant(&self) {
        self.chercher(|t| t.search_find_next());
    }

    fn chercher(&self, f: impl Fn(&vte4::Terminal) -> bool) {
        self.appliquer();
        let Some(t) = self.terminal() else { return };
        if self.posee.borrow().is_none() {
            return; // texte vide ou expression invalide : déjà signalé
        }
        if f(&t) {
            self.etat.set_text("");
        } else {
            self.etat.set_text("Aucun résultat");
            self.etat.announce(
                "Aucun résultat",
                gtk::AccessibleAnnouncementPriority::Medium,
            );
        }
    }
}

/// Échappe les caractères spéciaux PCRE2 d'un texte simple.
pub fn echapper(texte: &str) -> String {
    let mut s = String::with_capacity(texte.len() * 2);
    for c in texte.chars() {
        if "\\^$.|?*+()[]{}/-#".contains(c) {
            s.push('\\');
        }
        s.push(c);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echappement() {
        assert_eq!(echapper("a.b*(c)"), r"a\.b\*\(c\)");
        assert_eq!(echapper("été"), "été");
        assert_eq!(echapper(r"\d"), r"\\d");
    }
}
