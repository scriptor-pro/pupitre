// SPDX-License-Identifier: GPL-3.0-or-later
//! Fenêtre « Connexions SSH » : les connexions mémorisées par Pupitre (`connexions.toml`),
//! puis les hôtes de `~/.ssh/config`. On s'y connecte, on y ajoute, modifie et supprime
//! ses propres connexions. Tout se fait au clavier ; les textes sont écrits pour être lus
//! par Orca (étiquettes, descriptions, annonces).
//!
//! Règle de propriété : les rappels branchés sur les éléments de la fenêtre ne gardent
//! qu'une référence faible vers l'état ; seule la fenêtre elle-même (rappel « destroy »)
//! le garde, pour qu'il disparaisse avec elle.

use crate::fenetre::{confirmer_action, FenetreInterne, FenetrePupitre};
use crate::messages::{annoncer, annoncer_focus_une_fois_affiche};
use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use pupitre_config::{
    analyser_port, argv_ssh, charger_connexions, chemin_connexions, enregistrer_connexions,
    valider_connexions, ChampConnexion, Connexion,
};
use pupitre_ssh::hotes::hotes_ssh_config;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::{Rc, Weak};

const TITRE: &str = "Connexions SSH";
const TEXTE_VIDE: &str =
    "Aucune connexion. Ajoutez-en une, ou décrivez vos serveurs dans ~/.ssh/config.";
const TEXTE_VIDE_INVALIDE: &str = "Aucune connexion utilisable : corrigez connexions.toml, ou décrivez vos serveurs dans ~/.ssh/config.";
const PAR_DEFAUT: &str = "Par défaut";

/// État de la fenêtre de la liste.
struct Etat {
    fenetre: Weak<FenetreInterne>,
    dialogue: gtk::Window,
    liste: gtk::ListBox,
    vide: gtk::Label,
    /// Pourquoi connexions.toml est inutilisable (vide si tout va bien).
    explication: gtk::Label,
    se_connecter: gtk::Button,
    ajouter: gtk::Button,
    modifier: gtk::Button,
    supprimer: gtk::Button,
    fermer: gtk::Button,
    connexions: RefCell<Vec<Connexion>>,
    hotes: RefCell<Vec<String>>,
    /// `connexions.toml` est illisible : il n'est ni modifié ni réécrit.
    invalide: Cell<bool>,
}

/// Ouvre la fenêtre des connexions, modale sur `parent`.
pub fn ouvrir_connexions(parent: &FenetrePupitre) -> gtk::Window {
    let dialogue = gtk::Window::builder()
        .title(TITRE)
        .modal(true)
        .transient_for(parent.fenetre())
        .destroy_with_parent(true)
        .default_width(480)
        .default_height(360)
        .accessible_role(gtk::AccessibleRole::Dialog)
        .build();
    // Rattachée à l'application : même identifiant (et donc même icône) que les fenêtres
    // principales pour le bureau.
    dialogue.set_application(parent.fenetre().application().as_ref());
    let liste = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::Browse)
        .activate_on_single_click(false)
        .build();
    liste.set_widget_name("liste");
    liste.update_property(&[gtk::accessible::Property::Label("Connexions")]);
    let defile = gtk::ScrolledWindow::builder()
        .child(&liste)
        .vexpand(true)
        .hexpand(true)
        .build();
    let vide = gtk::Label::builder()
        .label(TEXTE_VIDE)
        .wrap(true)
        .max_width_chars(56)
        .xalign(0.0)
        .visible(false)
        .build();
    let explication = gtk::Label::builder()
        .wrap(true)
        .max_width_chars(56)
        .xalign(0.0)
        .visible(false)
        .build();
    explication.set_widget_name("explication");
    explication.add_css_class("error");
    let bouton = |libelle: &str| gtk::Button::with_label(libelle);
    let etat = Rc::new(Etat {
        fenetre: parent.faible(),
        dialogue: dialogue.clone(),
        liste,
        vide,
        explication,
        se_connecter: bouton("Se connecter"),
        ajouter: bouton("Ajouter…"),
        modifier: bouton("Modifier…"),
        supprimer: bouton("Supprimer"),
        fermer: bouton("Fermer"),
        connexions: RefCell::new(Vec::new()),
        hotes: RefCell::new(Vec::new()),
        invalide: Cell::new(false),
    });
    etat.se_connecter.add_css_class("suggested-action");

    let boutons = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .halign(gtk::Align::End)
        .build();
    for b in [
        &etat.se_connecter,
        &etat.ajouter,
        &etat.modifier,
        &etat.supprimer,
        &etat.fermer,
    ] {
        boutons.append(b);
    }
    let boite = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_start(18)
        .margin_end(18)
        .margin_top(18)
        .margin_bottom(18)
        .build();
    boite.append(&etat.explication);
    boite.append(&etat.vide);
    boite.append(&defile);
    boite.append(&boutons);
    dialogue.set_child(Some(&boite));

    // Rappels des éléments : référence faible seulement.
    let faible = Rc::downgrade(&etat);
    let avec = move |f: fn(&Rc<Etat>)| {
        let faible = faible.clone();
        move |_: &gtk::Button| {
            if let Some(e) = faible.upgrade() {
                f(&e);
            }
        }
    };
    etat.se_connecter.connect_clicked(avec(connecter_selection));
    etat.ajouter
        .connect_clicked(avec(|e| ouvrir_formulaire(e, None)));
    etat.modifier.connect_clicked(avec(|e| {
        if let Some(i) = indice_connexion_selectionnee(e) {
            ouvrir_formulaire(e, Some(i));
        }
    }));
    etat.supprimer.connect_clicked(avec(demander_suppression));
    let d = dialogue.clone();
    etat.fermer.connect_clicked(move |_| d.destroy());
    let faible = Rc::downgrade(&etat);
    etat.liste.connect_row_activated(move |_, ligne| {
        if let Some(e) = faible.upgrade() {
            connecter(&e, usize::try_from(ligne.index()).unwrap_or(usize::MAX));
        }
    });
    let faible = Rc::downgrade(&etat);
    etat.liste.connect_row_selected(move |_, _| {
        if let Some(e) = faible.upgrade() {
            maj_boutons(&e);
        }
    });

    // Échap ferme. Seule la fenêtre garde l'état (voir l'en-tête du module).
    let echap = gtk::EventControllerKey::new();
    let d = dialogue.clone();
    echap.connect_key_pressed(move |_, touche, _, _| {
        if touche == gdk::Key::Escape {
            d.destroy();
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    dialogue.add_controller(echap);
    let garde = RefCell::new(Some(etat.clone()));
    dialogue.connect_destroy(move |_| drop(garde.borrow_mut().take()));

    recharger(&etat, None);
    dialogue.present();
    donner_focus(&etat);
    dialogue
}

fn fenetre_pupitre(etat: &Etat) -> Option<FenetrePupitre> {
    FenetrePupitre::depuis_faible(&etat.fenetre)
}

fn chemin_ssh_config() -> Option<PathBuf> {
    Some(std::env::home_dir()?.join(".ssh").join("config"))
}

fn libelle_connexion(c: &Connexion) -> String {
    let mut s = format!("{} — ", c.nom);
    if let Some(u) = &c.utilisateur {
        s.push_str(u);
        s.push('@');
    }
    s.push_str(&c.hote);
    if let Some(p) = c.port {
        s.push_str(&format!(":{p}"));
    }
    s
}

fn libelle_hote(alias: &str) -> String {
    format!("{alias} (~/.ssh/config)")
}

fn nouvelle_ligne(texte: &str) -> gtk::ListBoxRow {
    let etiquette = gtk::Label::builder()
        .label(texte)
        .xalign(0.0)
        .margin_start(6)
        .margin_end(6)
        .margin_top(6)
        .margin_bottom(6)
        .build();
    let ligne = gtk::ListBoxRow::new();
    ligne.set_child(Some(&etiquette));
    // Le nom lu est le texte visible, ni plus ni moins.
    ligne.update_property(&[gtk::accessible::Property::Label(texte)]);
    ligne
}

/// Relit `connexions.toml` et `~/.ssh/config`, reconstruit la liste et sélectionne la
/// ligne `selection` (sinon la première).
fn recharger(etat: &Rc<Etat>, selection: Option<usize>) {
    let (connexions, message) = match charger_connexions(&chemin_connexions()) {
        Ok(v) => (v, None),
        Err(e) => (
            Vec::new(),
            Some(format!(
                "Le fichier connexions.toml est invalide ({e}). Il n'est pas modifié : \
                 corrigez-le pour retrouver vos connexions."
            )),
        ),
    };
    etat.invalide.set(message.is_some());
    match &message {
        Some(m) => {
            etat.explication.set_label(m);
            etat.explication.set_visible(true);
        }
        None => etat.explication.set_visible(false),
    }
    if let Some(f) = fenetre_pupitre(etat) {
        f.poser_message_connexions(message);
    }
    let hotes = chemin_ssh_config()
        .map(|c| hotes_ssh_config(&c))
        .unwrap_or_default();

    etat.liste.remove_all();
    for c in &connexions {
        etat.liste.append(&nouvelle_ligne(&libelle_connexion(c)));
    }
    for h in &hotes {
        etat.liste.append(&nouvelle_ligne(&libelle_hote(h)));
    }
    let total = connexions.len() + hotes.len();
    *etat.connexions.borrow_mut() = connexions;
    *etat.hotes.borrow_mut() = hotes;

    let vide = total == 0;
    etat.vide.set_label(if etat.invalide.get() {
        TEXTE_VIDE_INVALIDE
    } else {
        TEXTE_VIDE
    });
    etat.vide.set_visible(vide);
    let mut decrit: Vec<gtk::Accessible> = Vec::new();
    if etat.invalide.get() {
        decrit.push(etat.explication.clone().upcast());
    }
    if vide {
        decrit.push(etat.vide.clone().upcast());
    }
    if decrit.is_empty() {
        etat.dialogue
            .reset_relation(gtk::AccessibleRelation::DescribedBy);
    } else {
        let refs: Vec<&gtk::Accessible> = decrit.iter().collect();
        etat.dialogue
            .update_relation(&[gtk::accessible::Relation::DescribedBy(&refs)]);
    }
    let choix = selection.unwrap_or(0).min(total.saturating_sub(1));
    if let Some(l) = i32::try_from(choix)
        .ok()
        .and_then(|i| etat.liste.row_at_index(i))
    {
        etat.liste.select_row(Some(&l));
    }
    maj_boutons(etat);
}

/// Focus sur la ligne choisie ; liste vide : sur « Ajouter… ».
fn donner_focus(etat: &Etat) {
    let cible: gtk::Widget = match etat.liste.selected_row() {
        Some(l) => l.upcast(),
        None if etat.ajouter.is_sensitive() => etat.ajouter.clone().upcast(),
        None => etat.fermer.clone().upcast(),
    };
    cible.grab_focus();
    annoncer_focus_une_fois_affiche(&cible);
}

fn indice_selectionne(etat: &Etat) -> Option<usize> {
    etat.liste
        .selected_row()
        .and_then(|l| usize::try_from(l.index()).ok())
}

/// Indice dans `connexions.toml` de la ligne choisie (pas un hôte de `~/.ssh/config`).
fn indice_connexion_selectionnee(etat: &Etat) -> Option<usize> {
    indice_selectionne(etat).filter(|&i| i < etat.connexions.borrow().len())
}

fn maj_boutons(etat: &Etat) {
    let choisie = indice_selectionne(etat).is_some();
    let propre = indice_connexion_selectionnee(etat).is_some() && !etat.invalide.get();
    etat.se_connecter.set_sensitive(choisie);
    etat.ajouter.set_sensitive(!etat.invalide.get());
    etat.modifier.set_sensitive(propre);
    etat.supprimer.set_sensitive(propre);
}

fn connecter_selection(etat: &Rc<Etat>) {
    if let Some(i) = indice_selectionne(etat) {
        connecter(etat, i);
    }
}

/// Ouvre la ligne `i` dans un nouvel onglet de la fenêtre d'origine, puis se ferme.
fn connecter(etat: &Rc<Etat>, i: usize) {
    let n = etat.connexions.borrow().len();
    let (argv, titre) = if let Some(c) = etat.connexions.borrow().get(i) {
        (argv_ssh(c), c.nom.clone())
    } else if let Some(h) = etat.hotes.borrow().get(i.saturating_sub(n)) {
        (
            vec!["ssh".to_owned(), "--".to_owned(), h.clone()],
            h.clone(),
        )
    } else {
        return;
    };
    if let Some(f) = fenetre_pupitre(etat) {
        f.ouvrir_connexion(argv, titre);
    }
    etat.dialogue.destroy();
}

fn demander_suppression(etat: &Rc<Etat>) {
    let Some(i) = indice_connexion_selectionnee(etat) else {
        return;
    };
    let Some(nom) = etat.connexions.borrow().get(i).map(|c| c.nom.clone()) else {
        return;
    };
    let faible = Rc::downgrade(etat);
    let question = format!("Supprimer la connexion « {nom} » ?");
    confirmer_action(
        &etat.dialogue,
        "Supprimer ?",
        &question,
        "Supprimer",
        move || {
            if let Some(e) = faible.upgrade() {
                supprimer(&e, &nom);
            }
        },
    );
}

/// Retire la connexion `nom` du fichier (relu juste avant : rien d'autre n'est perdu).
fn supprimer(etat: &Rc<Etat>, nom: &str) {
    let chemin = chemin_connexions();
    let resultat = charger_connexions(&chemin)
        .map_err(|e| e.to_string())
        .and_then(|mut v| {
            v.retain(|c| c.nom != nom);
            enregistrer_connexions(&chemin, &v).map_err(|e| e.to_string())
        });
    let position = indice_selectionne(etat);
    if let Err(e) = resultat {
        let m = format!("Suppression impossible : {e}");
        if let Some(f) = fenetre_pupitre(etat) {
            f.afficher_message(&m);
        }
        annoncer(&etat.dialogue, &m);
    }
    recharger(etat, position);
    donner_focus(etat);
}

/// Clés privées de `~/.ssh` ayant une clé publique voisine, par nom.
fn cles_privees() -> Vec<(String, PathBuf)> {
    let Some(dossier) = std::env::home_dir().map(|h| h.join(".ssh")) else {
        return Vec::new();
    };
    let Ok(entrees) = std::fs::read_dir(&dossier) else {
        return Vec::new();
    };
    let mut v: Vec<(String, PathBuf)> = entrees
        .flatten()
        .filter_map(|e| {
            let nom = e.file_name().to_string_lossy().into_owned();
            if nom.ends_with(".pub") || !e.path().is_file() {
                return None;
            }
            dossier
                .join(format!("{nom}.pub"))
                .is_file()
                .then(|| (nom, e.path()))
        })
        .collect();
    v.sort();
    v
}

const CHAMPS: [(ChampConnexion, &str, &str); 5] = [
    (ChampConnexion::Nom, "nom", "Nom"),
    (ChampConnexion::Hote, "hote", "Hôte"),
    (
        ChampConnexion::Utilisateur,
        "utilisateur",
        "Utilisateur (facultatif)",
    ),
    (ChampConnexion::Port, "port", "Port (facultatif)"),
    (ChampConnexion::Cle, "cle", "Clé"),
];

/// État du formulaire d'ajout ou de modification.
struct Formulaire {
    etat: Weak<Etat>,
    fenetre: gtk::Window,
    /// Nom de la connexion modifiée (`None` : ajout).
    original: Option<String>,
    saisies: Vec<(ChampConnexion, gtk::Widget, gtk::Label)>,
    cle: gtk::DropDown,
    cles: Vec<PathBuf>,
    general: gtk::Label,
}

fn ouvrir_formulaire(etat: &Rc<Etat>, indice: Option<usize>) {
    let existante = indice.and_then(|i| etat.connexions.borrow().get(i).cloned());
    if indice.is_some() && existante.is_none() {
        return;
    }
    let titre = if existante.is_some() {
        "Modifier la connexion"
    } else {
        "Ajouter une connexion"
    };
    let fenetre = gtk::Window::builder()
        .title(titre)
        .modal(true)
        .transient_for(&etat.dialogue)
        .destroy_with_parent(true)
        .resizable(false)
        .accessible_role(gtk::AccessibleRole::Dialog)
        .build();
    fenetre.set_application(etat.dialogue.application().as_ref());
    let boite = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .margin_start(18)
        .margin_end(18)
        .margin_top(18)
        .margin_bottom(18)
        .build();

    // Clés : « Par défaut », les clés de ~/.ssh, et la clé déjà choisie si elle est ailleurs.
    let mut cles = cles_privees();
    let actuelle = existante.as_ref().and_then(|c| c.cle.clone());
    if let Some(a) = &actuelle {
        if !cles.iter().any(|(_, p)| p == a) {
            cles.push((a.to_string_lossy().into_owned(), a.clone()));
        }
    }
    let noms: Vec<&str> = std::iter::once(PAR_DEFAUT)
        .chain(cles.iter().map(|(n, _)| n.as_str()))
        .collect();
    let cle = gtk::DropDown::from_strings(&noms);
    cle.set_widget_name("cle");
    if let Some(a) = &actuelle {
        if let Some(p) = cles.iter().position(|(_, p)| p == a) {
            cle.set_selected(u32::try_from(p + 1).unwrap_or(0));
        }
    }

    let mut saisies = Vec::new();
    for (champ, nom, libelle) in CHAMPS {
        let etiquette = gtk::Label::builder()
            .label(libelle)
            .xalign(0.0)
            .margin_top(6)
            .build();
        let widget: gtk::Widget = if champ == ChampConnexion::Cle {
            cle.clone().upcast()
        } else {
            let texte = existante.as_ref().map(|c| match champ {
                ChampConnexion::Nom => c.nom.clone(),
                ChampConnexion::Hote => c.hote.clone(),
                ChampConnexion::Utilisateur => c.utilisateur.clone().unwrap_or_default(),
                _ => c.port.map(|p| p.to_string()).unwrap_or_default(),
            });
            let e = gtk::Entry::builder()
                .hexpand(true)
                .width_chars(32)
                .text(texte.unwrap_or_default())
                .activates_default(true)
                .build();
            e.set_widget_name(nom);
            e.upcast()
        };
        let erreur = gtk::Label::builder()
            .wrap(true)
            .max_width_chars(48)
            .xalign(0.0)
            .visible(false)
            .build();
        erreur.set_widget_name(&format!("erreur-{nom}"));
        erreur.add_css_class("error");
        widget.update_relation(&[
            gtk::accessible::Relation::LabelledBy(&[etiquette.upcast_ref()]),
            gtk::accessible::Relation::DescribedBy(&[erreur.upcast_ref()]),
        ]);
        boite.append(&etiquette);
        boite.append(&widget);
        boite.append(&erreur);
        saisies.push((champ, widget, erreur));
    }
    let general = gtk::Label::builder()
        .wrap(true)
        .max_width_chars(48)
        .xalign(0.0)
        .visible(false)
        .margin_top(6)
        .build();
    general.set_widget_name("erreur-general");
    general.add_css_class("error");
    boite.append(&general);

    let annuler = gtk::Button::with_label("Annuler");
    let enregistrer = gtk::Button::with_label("Enregistrer");
    enregistrer.add_css_class("suggested-action");
    let boutons = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .halign(gtk::Align::End)
        .margin_top(12)
        .build();
    boutons.append(&annuler);
    boutons.append(&enregistrer);
    boite.append(&boutons);
    fenetre.set_child(Some(&boite));
    fenetre.set_default_widget(Some(&enregistrer));

    let form = Rc::new(Formulaire {
        etat: Rc::downgrade(etat),
        fenetre: fenetre.clone(),
        original: existante.map(|c| c.nom),
        saisies,
        cle,
        cles: cles.into_iter().map(|(_, p)| p).collect(),
        general,
    });

    let faible = Rc::downgrade(&form);
    enregistrer.connect_clicked(move |_| {
        if let Some(f) = faible.upgrade() {
            f.valider();
        }
    });
    let f = fenetre.clone();
    annuler.connect_clicked(move |_| f.destroy());
    // Corriger un champ efface son message.
    for (_, widget, erreur) in &form.saisies {
        if let Ok(e) = widget.clone().dynamic_cast::<gtk::Editable>() {
            let (erreur, general) = (erreur.clone(), form.general.clone());
            e.connect_changed(move |_| {
                erreur.set_visible(false);
                general.set_visible(false);
            });
        }
    }
    let echap = gtk::EventControllerKey::new();
    let f = fenetre.clone();
    echap.connect_key_pressed(move |_, touche, _, _| {
        if touche == gdk::Key::Escape {
            f.destroy();
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    fenetre.add_controller(echap);
    let garde = RefCell::new(Some(form.clone()));
    fenetre.connect_destroy(move |_| drop(garde.borrow_mut().take()));

    fenetre.present();
    // Orca ignore l'activation de ce formulaire (celle de la liste vient juste de la
    // précéder) : le titre est annoncé ici, avant le premier champ.
    let faible_fenetre = fenetre.downgrade();
    glib::timeout_add_local_once(std::time::Duration::from_millis(40), move || {
        if let Some(f) = faible_fenetre.upgrade() {
            annoncer(&f, titre);
        }
    });
    if let Some((_, premier, _)) = form.saisies.first() {
        premier.grab_focus();
        annoncer_focus_une_fois_affiche(premier);
    }
}

impl Formulaire {
    fn texte(&self, champ: ChampConnexion) -> String {
        self.saisies
            .iter()
            .find(|(c, _, _)| *c == champ)
            .and_then(|(_, w, _)| w.clone().dynamic_cast::<gtk::Editable>().ok())
            .map(|e| e.text().trim().to_string())
            .unwrap_or_default()
    }

    /// Montre `message` sous le champ fautif, l'annonce et y met le focus.
    fn signaler(&self, champ: ChampConnexion, message: &str) {
        for (_, _, erreur) in &self.saisies {
            erreur.set_visible(false);
        }
        self.general.set_visible(false);
        let Some((_, widget, erreur)) = self.saisies.iter().find(|(c, _, _)| *c == champ) else {
            return;
        };
        let texte = majuscule(message);
        erreur.set_label(&texte);
        erreur.set_visible(true);
        annoncer(&self.fenetre, &texte);
        widget.grab_focus();
    }

    fn signaler_general(&self, message: &str) {
        self.general.set_label(message);
        self.general.set_visible(true);
        annoncer(&self.fenetre, message);
    }

    fn valider(&self) {
        let Some(etat) = self.etat.upgrade() else {
            return;
        };
        let port = match analyser_port(&self.texte(ChampConnexion::Port)) {
            Ok(p) => p,
            Err(m) => return self.signaler(ChampConnexion::Port, &m),
        };
        let facultatif = |c| Some(self.texte(c)).filter(|t| !t.is_empty());
        let cle = usize::try_from(self.cle.selected())
            .ok()
            .and_then(|i| i.checked_sub(1))
            .and_then(|i| self.cles.get(i).cloned());
        let nouvelle = Connexion {
            nom: self.texte(ChampConnexion::Nom),
            hote: self.texte(ChampConnexion::Hote),
            utilisateur: facultatif(ChampConnexion::Utilisateur),
            port,
            cle,
        };

        // Le fichier est relu : une modification faite ailleurs n'est pas écrasée.
        let chemin = chemin_connexions();
        let mut liste = match charger_connexions(&chemin) {
            Ok(v) => v,
            Err(e) => {
                return self.signaler_general(&format!(
                    "Enregistrement impossible : connexions.toml est invalide ({e})."
                ))
            }
        };
        match &self.original {
            Some(nom) => match liste.iter_mut().find(|c| &c.nom == nom) {
                Some(c) => *c = nouvelle.clone(),
                None => {
                    return self.signaler_general(
                        "Enregistrement impossible : cette connexion n'existe plus.",
                    )
                }
            },
            None => liste.push(nouvelle.clone()),
        }
        if let Err(e) = valider_connexions(&liste) {
            return self.signaler(e.champ, &e.message);
        }
        if let Err(e) = enregistrer_connexions(&chemin, &liste) {
            return self.signaler_general(&format!("Enregistrement impossible : {e}"));
        }
        let position = liste.iter().position(|c| c.nom == nouvelle.nom);
        self.fenetre.destroy();
        recharger(&etat, position);
        donner_focus(&etat);
    }
}

fn majuscule(m: &str) -> String {
    let mut c = m.chars();
    match c.next() {
        Some(p) => p.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}
