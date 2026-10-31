// SPDX-License-Identifier: GPL-3.0-or-later
//! Bandeau de messages d'une fenêtre, entre la barre d'onglets et le terminal.
//! Chaque message est annoncé au lecteur d'écran. Un message persistant a un bouton
//! « Fermer » ; un message temporaire disparaît seul. Un message « à clé » (ex. l'erreur de
//! configuration du moment) est remplacé ou retiré par la même clé.

use gtk::glib;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;

/// Action d'un bouton de message.
pub type ActionMessage = Rc<dyn Fn()>;

struct Message {
    id: u64,
    cle: Option<String>,
    texte: String,
    ligne: gtk::Box,
    action: Option<gtk::Button>,
}

pub struct BandeauMessages {
    boite: gtk::Box,
    messages: RefCell<Vec<Message>>,
    prochain: Cell<u64>,
    soi: RefCell<Weak<BandeauMessages>>,
}

impl BandeauMessages {
    pub fn new() -> Rc<Self> {
        let boite = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .visible(false)
            .build();
        let b = Rc::new(BandeauMessages {
            boite,
            messages: RefCell::new(Vec::new()),
            prochain: Cell::new(1),
            soi: RefCell::new(Weak::new()),
        });
        *b.soi.borrow_mut() = Rc::downgrade(&b);
        b
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.boite
    }

    /// Textes affichés, du plus ancien au plus récent.
    pub fn textes(&self) -> Vec<String> {
        self.messages
            .borrow()
            .iter()
            .map(|m| m.texte.clone())
            .collect()
    }

    /// Affiche un message ; `duree` = `None` : persistant avec bouton « Fermer ».
    /// Un message identique déjà affiché n'est pas répété. Renvoie son numéro.
    pub fn afficher(&self, texte: &str, duree: Option<Duration>) -> u64 {
        if let Some(m) = self
            .messages
            .borrow()
            .iter()
            .find(|m| m.cle.is_none() && m.texte == texte)
        {
            return m.id;
        }
        let id = self.ajouter(None, texte, duree.is_none(), None);
        if let Some(d) = duree {
            let soi = self.soi.borrow().clone();
            glib::timeout_add_local_once(d, move || {
                if let Some(b) = soi.upgrade() {
                    b.retirer(id);
                }
            });
        }
        id
    }

    /// Pose (ou remplace) le message persistant de clé `cle` ; `None` le retire.
    pub fn poser(&self, cle: &str, texte: Option<&str>) {
        self.poser_avec_action(cle, texte, None);
    }

    /// Comme [`poser`](Self::poser), avec un bouton d'action `(libellé, action)`.
    pub fn poser_avec_action(
        &self,
        cle: &str,
        texte: Option<&str>,
        action: Option<(&str, ActionMessage)>,
    ) {
        let existant = self
            .messages
            .borrow()
            .iter()
            .find(|m| m.cle.as_deref() == Some(cle))
            .map(|m| (m.id, m.texte.clone()));
        match (existant, texte) {
            (Some((_, ancien)), Some(t)) if ancien == t && action.is_none() => {}
            (Some((id, _)), t) => {
                self.retirer(id);
                if let Some(t) = t {
                    self.ajouter(Some(cle), t, true, action);
                }
            }
            (None, Some(t)) => {
                self.ajouter(Some(cle), t, true, action);
            }
            (None, None) => {}
        }
    }

    /// Déclenche le bouton d'action du message de clé `cle` (comme un clic).
    pub fn activer(&self, cle: &str) {
        let bouton = self
            .messages
            .borrow()
            .iter()
            .find(|m| m.cle.as_deref() == Some(cle))
            .and_then(|m| m.action.clone());
        if let Some(b) = bouton {
            b.emit_clicked();
        }
    }

    /// Boutons des messages affichés (action puis « Fermer »), dans l'ordre.
    pub fn boutons(&self) -> Vec<gtk::Widget> {
        let mut v = Vec::new();
        for m in self.messages.borrow().iter() {
            let mut enfant = m.ligne.first_child();
            while let Some(e) = enfant {
                enfant = e.next_sibling();
                if e.is::<gtk::Button>() {
                    v.push(e);
                }
            }
        }
        v
    }

    /// Met le focus clavier sur le bouton d'action du message `cle`, s'il existe.
    pub fn donner_focus(&self, cle: &str) {
        if let Some(b) = self
            .messages
            .borrow()
            .iter()
            .find(|m| m.cle.as_deref() == Some(cle))
            .and_then(|m| m.action.clone())
        {
            b.grab_focus();
        }
    }

    pub fn retirer(&self, id: u64) {
        let retire = {
            let mut v = self.messages.borrow_mut();
            v.iter().position(|m| m.id == id).map(|i| v.remove(i))
        };
        if let Some(m) = retire {
            self.boite.remove(&m.ligne);
        }
        self.boite.set_visible(!self.messages.borrow().is_empty());
    }

    fn ajouter(
        &self,
        cle: Option<&str>,
        texte: &str,
        persistant: bool,
        action: Option<(&str, ActionMessage)>,
    ) -> u64 {
        let id = self.prochain.get();
        self.prochain.set(id + 1);
        let ligne = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .margin_start(12)
            .margin_end(12)
            .margin_top(4)
            .margin_bottom(4)
            .build();
        let etiquette = gtk::Label::builder()
            .label(texte)
            .wrap(true)
            .selectable(true)
            .xalign(0.0)
            .hexpand(true)
            .build();
        // Pas de focus clavier sur le texte : il volerait le focus initial de la fenêtre (et
        // tout sélectionner). Les boutons du message sont décrits par ce texte : Orca le lit
        // en y arrivant. La sélection à la souris reste possible.
        etiquette.set_focusable(false);
        ligne.append(&etiquette);
        let decrire = |b: &gtk::Button| {
            b.update_relation(&[gtk::accessible::Relation::DescribedBy(&[
                etiquette.upcast_ref()
            ])]);
        };
        let bouton_action = action.map(|(libelle, a)| {
            let b = gtk::Button::with_label(libelle);
            b.connect_clicked(move |_| a());
            decrire(&b);
            ligne.append(&b);
            b
        });
        if persistant {
            let fermer = gtk::Button::with_label("Fermer");
            fermer.set_tooltip_text(Some("Fermer ce message"));
            decrire(&fermer);
            let soi = self.soi.borrow().clone();
            fermer.connect_clicked(move |_| {
                if let Some(b) = soi.upgrade() {
                    b.retirer(id);
                }
            });
            ligne.append(&fermer);
        }
        self.boite.append(&ligne);
        self.boite.set_visible(true);
        self.messages.borrow_mut().push(Message {
            id,
            cle: cle.map(String::from),
            texte: texte.to_string(),
            ligne,
            action: bouton_action,
        });
        annoncer(&self.boite, texte);
        id
    }
}

/// Annonce `texte` au lecteur d'écran, poliment (sans couper ce qu'il dit déjà).
///
/// L'annonce part de la fenêtre qui contient `w` : un widget qui vient d'apparaître
/// (bandeau, état de la recherche) n'est pas encore publié pour le lecteur d'écran et
/// son annonce serait perdue.
pub fn annoncer(w: &impl IsA<gtk::Widget>, texte: &str) {
    let prio = gtk::AccessibleAnnouncementPriority::Medium;
    match w.as_ref().root() {
        Some(r) => r.announce(texte, prio),
        None => w.as_ref().announce(texte, prio),
    }
}

/// Délai avant d'annoncer un élément qui a reçu le focus juste après sa création.
const DELAI_ANNONCE_NOUVEAU: Duration = Duration::from_millis(100);

/// Retire puis redonne le focus à l'élément qui l'a : le lecteur d'écran reçoit un
/// nouvel événement « focus » et annonce l'élément (rien ne change pour le clavier).
pub fn refaire_annonce_focus(fenetre: &impl IsA<gtk::Window>) {
    if let Some(w) = GtkWindowExt::focus(fenetre) {
        GtkWindowExt::set_focus(fenetre, None::<&gtk::Widget>);
        w.grab_focus();
    }
}

/// Un widget qui reçoit le focus juste après sa création n'existe pas encore pour le
/// lecteur d'écran : GTK n'envoie pas l'événement « focus » et rien n'est annoncé
/// (nouvel onglet, barre de recherche). Peu après, s'il a toujours le focus, l'annonce
/// est refaite.
pub fn annoncer_focus_une_fois_affiche(w: &gtk::Widget) {
    let faible = w.downgrade();
    glib::timeout_add_local_once(DELAI_ANNONCE_NOUVEAU, move || {
        let Some(w) = faible.upgrade() else { return };
        let Some(f) = w.root().and_then(|r| r.downcast::<gtk::Window>().ok()) else {
            return;
        };
        // Le focus peut être sur un enfant (la saisie de texte d'un GtkSearchEntry).
        if GtkWindowExt::focus(&f).is_some_and(|x| x == w || x.is_ancestor(&w)) {
            refaire_annonce_focus(&f);
        }
    });
}
