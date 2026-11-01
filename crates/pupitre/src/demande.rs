// SPDX-License-Identifier: GPL-3.0-or-later
//! Fenêtre modale d'une demande SSH : phrase de passe d'une clé, question sur un hôte
//! inconnu, confirmation d'usage d'une clé ou saisie libre. Elle ne fait que montrer la
//! question et rappeler `repondre` une seule fois ; le routage (quelle fenêtre parente,
//! quelle demande) est dans `fenetre.rs`.

use crate::messages::annoncer_focus_une_fois_affiche;
use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use pupitre_i18n::{t, ta, ArgI18n};
use pupitre_proto::{EtatTrousseau, Genre};
use pupitre_ssh::trousseau::chemin_lisible;
use std::cell::Cell;
use std::rc::Rc;
use zeroize::Zeroizing;

/// Ce qu'il faut savoir d'une demande pour l'afficher.
#[derive(Clone, Debug)]
pub struct DemandeAffichee {
    /// Texte de l'invite, tel que donné par OpenSSH.
    pub invite: String,
    pub genre: Genre,
    pub trousseau: EtatTrousseau,
}

/// Réponse rendue à l'appelant : `None` = refus ou annulation.
type Reponse = Option<Zeroizing<String>>;

/// Ouvre la fenêtre de la demande `d`, modale sur `parent`. `repondre(reponse, memoriser)`
/// est appelé exactement une fois, quelle que soit la façon de fermer la fenêtre (sauf si
/// l'appelant la détruit lui-même : demande réglée ailleurs). Le texte saisi est effacé de
/// la saisie dès l'envoi. Le genre `Information` n'a pas de fenêtre : l'appelant l'affiche
/// dans le bandeau ; ici il ne donnerait qu'un bouton « Fermer ».
pub fn ouvrir_demande(
    parent: &gtk::Window,
    d: &DemandeAffichee,
    repondre: impl Fn(Option<Zeroizing<String>>, bool) + 'static,
) -> gtk::Window {
    let titre = match &d.genre {
        Genre::PhraseCle { .. } => t("demande-titre-phrase"),
        Genre::HoteInconnu => t("demande-titre-hote"),
        Genre::Confirmation => t("demande-titre-confirmation"),
        Genre::Texte { .. } => t("demande-titre-question"),
        Genre::Information => t("demande-titre-information"),
    };
    let dialogue = gtk::Window::builder()
        .title(&titre)
        .modal(true)
        .transient_for(parent)
        .destroy_with_parent(true)
        .resizable(false)
        .accessible_role(gtk::AccessibleRole::Dialog)
        .build();
    // Rattachée à l'application : même identifiant (et donc même icône) que les fenêtres
    // principales pour le bureau.
    dialogue.set_application(parent.application().as_ref());

    // Une seule réponse, puis la fenêtre disparaît. Détruite sans réponse (fenêtre
    // parente fermée, par exemple), elle répond « annulé ».
    let faite = Rc::new(Cell::new(false));
    let repondre_une_fois: Rc<dyn Fn(Reponse, bool)> = Rc::new(move |reponse, memoriser| {
        if !faite.replace(true) {
            repondre(reponse, memoriser);
        }
    });
    let finir: Rc<dyn Fn(Reponse, bool)> = {
        let une_fois = repondre_une_fois.clone();
        let dialogue = dialogue.clone();
        Rc::new(move |reponse, memoriser| {
            une_fois(reponse, memoriser);
            dialogue.destroy();
        })
    };
    dialogue.connect_destroy(move |_| repondre_une_fois(None, false));

    let boite = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_start(18)
        .margin_end(18)
        .margin_top(18)
        .margin_bottom(18)
        .build();
    let boutons = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .halign(gtk::Align::End)
        .margin_top(6)
        .build();

    let focus: gtk::Widget = match &d.genre {
        Genre::PhraseCle { cle, nouvel_essai } => {
            let id = if *nouvel_essai {
                "demande-phrase-question-incorrecte"
            } else {
                "demande-phrase-question"
            };
            let question = ta(id, &[("cle", ArgI18n::from(chemin_lisible(cle)))]);
            let (etiquette, saisie) = question_et_saisie(&question, true);
            boite.append(&etiquette);
            boite.append(&saisie);
            let case = gtk::CheckButton::with_label(&t("demande-memoriser"));
            case.set_active(d.trousseau == EtatTrousseau::Disponible);
            case.set_sensitive(d.trousseau == EtatTrousseau::Disponible);
            boite.append(&case);
            let explication = match d.trousseau {
                EtatTrousseau::Disponible => None,
                EtatTrousseau::Verrouille => Some(t("demande-trousseau-verrouille")),
                EtatTrousseau::Absent => Some(t("demande-trousseau-absent")),
            };
            if let Some(texte) = explication {
                let e = gtk::Label::builder()
                    .label(&texte)
                    .wrap(true)
                    .max_width_chars(56)
                    .xalign(0.0)
                    .build();
                // Décrit la case (inactive, jamais atteinte au clavier) et la saisie, où
                // se trouve le focus : c'est là qu'Orca la lit.
                case.update_relation(&[gtk::accessible::Relation::DescribedBy(&[e.upcast_ref()])]);
                saisie
                    .update_relation(&[gtk::accessible::Relation::DescribedBy(&[e.upcast_ref()])]);
                boite.append(&e);
            }
            let valider = ajouter_annuler_valider(&boutons, &dialogue, &finir);
            let (saisie_v, finir_v) = (saisie.clone(), finir.clone());
            let envoyer = move || {
                let phrase = Zeroizing::new(editable(&saisie_v).text().to_string());
                editable(&saisie_v).set_text("");
                finir_v(Some(phrase), case.is_active() && case.is_sensitive());
            };
            lier_validation(&valider, &saisie, envoyer);
            saisie.upcast()
        }
        Genre::Texte { masque } => {
            let (etiquette, saisie) = question_et_saisie(&d.invite, *masque);
            boite.append(&etiquette);
            boite.append(&saisie);
            let valider = ajouter_annuler_valider(&boutons, &dialogue, &finir);
            let (saisie_v, finir_v) = (saisie.clone(), finir.clone());
            let envoyer = move || {
                let texte = Zeroizing::new(editable(&saisie_v).text().to_string());
                editable(&saisie_v).set_text("");
                finir_v(Some(texte), false);
            };
            lier_validation(&valider, &saisie, envoyer);
            saisie
        }
        Genre::HoteInconnu | Genre::Confirmation => {
            let texte = texte_openssh(&d.invite);
            boite.append(&texte);
            let (refus, accord) = if d.genre == Genre::HoteInconnu {
                (t("demande-refuser"), t("demande-faire-confiance"))
            } else {
                (t("demande-refuser"), t("demande-autoriser"))
            };
            let refuser = gtk::Button::with_label(&refus);
            let autoriser = gtk::Button::with_label(&accord);
            boutons.append(&refuser);
            boutons.append(&autoriser);
            // Le texte d'OpenSSH (empreinte, usage de la clé) est ce qu'on décide : il est
            // lu à l'ouverture (description de la fenêtre) et à chaque bouton.
            let decrit = gtk::accessible::Relation::DescribedBy(&[texte.upcast_ref()]);
            dialogue.update_relation(&[decrit]);
            for b in [&refuser, &autoriser] {
                b.update_relation(&[gtk::accessible::Relation::DescribedBy(
                    &[texte.upcast_ref()],
                )]);
            }
            let f = finir.clone();
            refuser.connect_clicked(move |_| f(Some(Zeroizing::new("no".into())), false));
            let f = finir.clone();
            autoriser.connect_clicked(move |_| f(Some(Zeroizing::new("yes".into())), false));
            // Le refus est le choix sûr : il a le focus et reste le défaut.
            dialogue.set_default_widget(Some(&refuser));
            refuser.upcast()
        }
        Genre::Information => {
            boite.append(&texte_openssh(&d.invite));
            let fermer = gtk::Button::with_label(&t("bouton-fermer"));
            boutons.append(&fermer);
            let f = finir.clone();
            fermer.connect_clicked(move |_| f(None, false));
            dialogue.set_default_widget(Some(&fermer));
            fermer.upcast()
        }
    };
    boite.append(&boutons);
    dialogue.set_child(Some(&boite));

    // Échap et fermeture par le gestionnaire de fenêtres : refus, sans mémorisation.
    let echap = gtk::EventControllerKey::new();
    let f = finir.clone();
    echap.connect_key_pressed(move |_, touche, _, _| {
        if touche == gdk::Key::Escape {
            f(None, false);
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    dialogue.add_controller(echap);
    let f = finir.clone();
    dialogue.connect_close_request(move |_| {
        f(None, false);
        glib::Propagation::Stop
    });

    dialogue.present();
    focus.grab_focus();
    annoncer_focus_une_fois_affiche(&focus);
    dialogue
}

/// Texte d'OpenSSH, sélectionnable à la souris mais sans focus clavier (il volerait le
/// focus initial de la fenêtre).
fn texte_openssh(texte: &str) -> gtk::Label {
    let e = gtk::Label::builder()
        .label(texte)
        .wrap(true)
        .max_width_chars(56)
        .selectable(true)
        .xalign(0.0)
        .build();
    e.set_focusable(false);
    e
}

/// Question et saisie qu'elle nomme pour le lecteur d'écran (la saisie est « étiquetée
/// par » la question, qui n'est pas aussi sa description : Orca la dirait deux fois).
fn question_et_saisie(question: &str, masque: bool) -> (gtk::Label, gtk::Widget) {
    let etiquette = texte_openssh(question);
    let saisie: gtk::Widget = if masque {
        gtk::PasswordEntry::builder()
            .show_peek_icon(true)
            .hexpand(true)
            .build()
            .upcast()
    } else {
        gtk::Entry::builder().hexpand(true).build().upcast()
    };
    saisie.update_relation(&[gtk::accessible::Relation::LabelledBy(&[
        etiquette.upcast_ref()
    ])]);
    (etiquette, saisie)
}

/// Vue « texte modifiable » d'une saisie créée par [`question_et_saisie`].
fn editable(w: &gtk::Widget) -> gtk::Editable {
    // Les deux types de saisie implémentent `Editable` ; à défaut, une saisie vide.
    w.clone()
        .dynamic_cast::<gtk::Editable>()
        .unwrap_or_else(|_| gtk::Text::new().upcast())
}

/// Boutons « Annuler » et « Valider » (par défaut) ; renvoie « Valider ».
fn ajouter_annuler_valider(
    boutons: &gtk::Box,
    dialogue: &gtk::Window,
    finir: &Rc<dyn Fn(Reponse, bool)>,
) -> gtk::Button {
    let annuler = gtk::Button::with_label(&t("bouton-annuler"));
    let valider = gtk::Button::with_label(&t("bouton-valider"));
    valider.add_css_class("suggested-action");
    boutons.append(&annuler);
    boutons.append(&valider);
    dialogue.set_default_widget(Some(&valider));
    let f = finir.clone();
    annuler.connect_clicked(move |_| f(None, false));
    valider
}

/// « Valider » et Entrée dans la saisie font la même chose.
fn lier_validation(valider: &gtk::Button, saisie: &gtk::Widget, envoyer: impl Fn() + 'static) {
    let envoyer = Rc::new(envoyer);
    let e = envoyer.clone();
    valider.connect_clicked(move |_| e());
    saisie.connect_local("activate", false, move |_| {
        envoyer();
        None
    });
}
