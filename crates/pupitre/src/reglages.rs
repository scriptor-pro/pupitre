// SPDX-License-Identifier: GPL-3.0-or-later
//! Fenêtre « Réglages de Pupitre » : les réglages courants de `pupitre.toml` dans une colonne de
//! champs, chacun avec son étiquette (relation `labelled-by`). Chaque changement est écrit aussitôt
//! dans le fichier (commentaires, ordre et clés inconnues gardés, sauvegarde `.bak`) ; c'est la
//! surveillance du fichier qui l'applique à toutes les fenêtres : celle-ci n'applique rien.
//! Un fichier invalide n'est jamais écrasé : la fenêtre est alors en lecture seule.
//! Une seule fenêtre ; Échap la ferme.

use crate::polices::polices_chasse_fixe;
use crate::shells::{path_courant, shells_proposes};
use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use pupitre_config::{
    charger, chemin_config, enregistrer_reglage, AnnonceFin, BandeauClavier, Config, ErreurEdition,
    Invite, ThemeChoix,
};
use pupitre_i18n::t;
use std::cell::RefCell;
use std::rc::Rc;
use toml_edit::Value;

/// Titre de la fenêtre (aussi celui que cherchent les tests).
pub const TITRE_FR: &str = "Réglages de Pupitre";

/// Thèmes proposés : valeur écrite dans `pupitre.toml` et clé du libellé. Un seul endroit : la
/// liste des thèmes à contraste élevé (Task 4) s'y ajoute. Un thème fichier est affiché comme
/// « personnalisé » (entrée ajoutée à la volée) et jamais écrasé par la fenêtre.
pub const THEMES: &[(&str, &str)] = &[
    ("systeme", "reglages-theme-systeme"),
    ("sombre", "reglages-theme-sombre"),
    ("clair", "reglages-theme-clair"),
];

const INVITES: &[(&str, &str)] = &[
    ("starship", "reglages-invite-starship"),
    ("shell", "reglages-invite-shell"),
];

const ANNONCES: &[(&str, &str)] = &[
    ("longues", "reglages-annonce-longues"),
    ("toutes", "reglages-annonce-toutes"),
    ("aucune", "reglages-annonce-aucune"),
];

const BANDEAUX: &[(&str, &str)] = &[
    ("nom", "reglages-bandeau-nom"),
    ("code", "reglages-bandeau-code"),
    ("masque", "reglages-bandeau-masque"),
];

/// Les six cases de `[notifications]` : clé du fichier, clé du libellé.
const NOTIFICATIONS: &[(&[&str], &str)] = &[
    (
        &["notifications", "fin_commande_onglet"],
        "reglages-notif-fin-onglet",
    ),
    (
        &["notifications", "fin_commande_bulle"],
        "reglages-notif-fin-bulle",
    ),
    (
        &["notifications", "programme_onglet"],
        "reglages-notif-prog-onglet",
    ),
    (
        &["notifications", "programme_bulle"],
        "reglages-notif-prog-bulle",
    ),
    (
        &["notifications", "presse_papiers"],
        "reglages-notif-presse-papiers",
    ),
    (&["notifications", "cloche_onglet"], "reglages-notif-cloche"),
];

const TAILLE_MIN: f64 = 6.0;
const TAILLE_MAX: f64 = 72.0;
const TAILLE_DEFAUT: f64 = 11.0;

thread_local! {
    static OUVERTE: RefCell<glib::WeakRef<gtk::Window>> = RefCell::new(glib::WeakRef::new());
}

/// Ouvre la fenêtre de réglages (une seule : la rouvrir la ramène au premier plan).
pub fn ouvrir_reglages(app: &gtk::Application, parent: &gtk::Window) {
    // Sortie du `RefCell` avant `present` : aucun emprunt pendant un rappel ré-entrant.
    let existante = OUVERTE.with(|o| o.borrow().upgrade());
    if let Some(w) = existante.filter(|w| w.is_visible()) {
        w.present();
        return;
    }
    let fenetre = gtk::Window::builder()
        .title(t("reglages-titre"))
        .application(app)
        .transient_for(parent)
        .destroy_with_parent(true)
        .default_width(560)
        .default_height(640)
        .build();
    let colonne = gtk::Box::new(gtk::Orientation::Vertical, 12);
    colonne.set_margin_top(12);
    colonne.set_margin_bottom(12);
    colonne.set_margin_start(12);
    colonne.set_margin_end(12);
    let etat = gtk::Label::new(None);
    etat.set_xalign(0.0);
    etat.set_wrap(true);
    let racine = gtk::Box::new(gtk::Orientation::Vertical, 0);
    match charger(&chemin_config()) {
        Ok(config) => {
            let champs = gtk::Box::new(gtk::Orientation::Vertical, 12);
            remplir(&champs, &config, &etat);
            colonne.append(&champs);
            colonne.append(&etat);
        }
        Err(e) => {
            // Lecture seule : le message, aucun champ.
            let message = ErreurEdition::Invalide(e.ligne).to_string();
            etat.set_text(&message);
            etat.set_focusable(true);
            colonne.append(&etat);
            GtkWindowExt::set_focus(&fenetre, Some(&etat));
            etat.announce(&message, gtk::AccessibleAnnouncementPriority::High);
        }
    }
    let defilement = gtk::ScrolledWindow::new();
    defilement.set_child(Some(&colonne));
    defilement.set_vexpand(true);
    racine.append(&defilement);
    fenetre.set_child(Some(&racine));
    // Échap ferme (explicite, comme pour l'aide et « À propos »).
    let clavier = gtk::EventControllerKey::new();
    let faible = fenetre.downgrade();
    clavier.connect_key_pressed(move |_, touche, _, mods| {
        if touche == gdk::Key::Escape && mods.is_empty() {
            if let Some(f) = faible.upgrade() {
                f.close();
            }
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    fenetre.add_controller(clavier);
    OUVERTE.with(|o| o.borrow().set(Some(&fenetre)));
    fenetre.present();
}

/// Écrit un réglage ; sur un fichier devenu invalide (modifié à la main), plus aucune écriture :
/// les champs sont désactivés et le message est affiché.
fn enregistrer(champs: &gtk::Box, etat: &gtk::Label, chemin: &[&str], valeur: Value) {
    let (message, priorite) = match enregistrer_reglage(chemin, valeur) {
        Ok(()) => (
            t("reglages-enregistre"),
            gtk::AccessibleAnnouncementPriority::Medium,
        ),
        Err(e) => {
            if matches!(e, ErreurEdition::Invalide(_)) {
                champs.set_sensitive(false);
            }
            (e.to_string(), gtk::AccessibleAnnouncementPriority::High)
        }
    };
    etat.set_text(&message);
    etat.announce(&message, priorite);
}

/// Une rangée : étiquette puis champ, reliés par `labelled-by` (et par l'accès mnémonique).
fn rangee(champs: &gtk::Box, libelle: &str, controle: &impl IsA<gtk::Widget>) {
    let ligne = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let etiquette = gtk::Label::new(Some(libelle));
    etiquette.set_xalign(0.0);
    etiquette.set_hexpand(true);
    etiquette.set_wrap(true);
    etiquette.set_mnemonic_widget(Some(controle));
    let controle: &gtk::Widget = controle.upcast_ref();
    controle.update_relation(&[gtk::accessible::Relation::LabelledBy(&[
        etiquette.upcast_ref()
    ])]);
    controle.set_valign(gtk::Align::Center);
    ligne.append(&etiquette);
    ligne.append(controle);
    champs.append(&ligne);
}

/// Liste déroulante sur `choix` (valeur, libellé) ; renvoie aussi la position de `courant`, qui
/// est ajouté en fin de liste (sous `libelle_extra`) s'il n'y figure pas.
fn liste(
    choix: &[(String, String)],
    courant: &str,
    libelle_extra: Option<&str>,
) -> (gtk::DropDown, Vec<String>) {
    let mut valeurs: Vec<String> = choix.iter().map(|(v, _)| v.clone()).collect();
    let mut libelles: Vec<String> = choix.iter().map(|(_, l)| l.clone()).collect();
    let position = match valeurs.iter().position(|v| v == courant) {
        Some(i) => i,
        None => {
            valeurs.push(courant.to_string());
            libelles.push(libelle_extra.unwrap_or(courant).to_string());
            valeurs.len() - 1
        }
    };
    let refs: Vec<&str> = libelles.iter().map(String::as_str).collect();
    let d = gtk::DropDown::from_strings(&refs);
    d.set_selected(u32::try_from(position).unwrap_or(0));
    (d, valeurs)
}

fn choix_i18n(table: &[(&str, &str)]) -> Vec<(String, String)> {
    table
        .iter()
        .map(|(v, c)| ((*v).to_string(), t(c)))
        .collect()
}

/// Liste déroulante dont chaque changement s'écrit sous `cle` (texte). `ignorer` : valeur qui
/// ne s'écrit jamais (thème fichier affiché comme « personnalisé »).
fn liste_ecrite(
    champs: &gtk::Box,
    etat: &gtk::Label,
    cle: &'static str,
    libelle: &str,
    choix: &[(String, String)],
    courant: &str,
    ignorer: Option<(String, String)>,
) {
    let (d, valeurs) = liste(choix, courant, ignorer.as_ref().map(|(_, l)| l.as_str()));
    rangee(champs, libelle, &d);
    let (champs, etat) = (champs.clone(), etat.clone());
    let valeurs = Rc::new(valeurs);
    d.connect_selected_notify(move |d| {
        let Some(v) = valeurs.get(d.selected() as usize) else {
            return;
        };
        if ignorer.as_ref().is_some_and(|(i, _)| i == v) {
            return;
        }
        enregistrer(&champs, &etat, &[cle], Value::from(v.as_str()));
    });
}

fn case_ecrite(
    champs: &gtk::Box,
    etat: &gtk::Label,
    chemin: &'static [&'static str],
    libelle: &str,
    actif: bool,
) {
    let c = gtk::CheckButton::with_label(libelle);
    c.set_active(actif);
    let (champs_c, etat_c) = (champs.clone(), etat.clone());
    c.connect_toggled(move |c| {
        enregistrer(&champs_c, &etat_c, chemin, Value::from(c.is_active()));
    });
    champs.append(&c);
}

/// Sépare « B612 Mono 11 » en famille et taille.
fn police_et_taille(police: &str) -> (String, f64) {
    if let Some((famille, taille)) = police.trim().rsplit_once(' ') {
        if let Ok(t) = taille.parse::<f64>() {
            return (famille.trim().to_string(), t.clamp(TAILLE_MIN, TAILLE_MAX));
        }
    }
    (police.trim().to_string(), TAILLE_DEFAUT)
}

fn remplir(champs: &gtk::Box, config: &Config, etat: &gtk::Label) {
    // Police et taille : la police actuelle est ajoutée à la liste si elle n'y est pas.
    let (famille, taille) = police_et_taille(&config.police);
    let familles: Vec<(String, String)> = polices_chasse_fixe()
        .into_iter()
        .map(|f| (f.clone(), f))
        .collect();
    let (dpolice, valeurs) = liste(&familles, &famille, None);
    let spin = gtk::SpinButton::with_range(TAILLE_MIN, TAILLE_MAX, 1.0);
    spin.set_value(taille);
    rangee(champs, &t("reglages-police"), &dpolice);
    rangee(champs, &t("reglages-taille"), &spin);
    // Les deux champs s'écrivent ensemble : « famille taille ».
    let valeurs = Rc::new(valeurs);
    let ecrire = {
        let (champs, etat) = (champs.clone(), etat.clone());
        let (dpolice, spin, valeurs) = (dpolice.clone(), spin.clone(), valeurs.clone());
        Rc::new(move || {
            let Some(f) = valeurs.get(dpolice.selected() as usize) else {
                return;
            };
            let texte = format!("{f} {}", spin.value_as_int());
            enregistrer(&champs, &etat, &["police"], Value::from(texte));
        })
    };
    let e = ecrire.clone();
    dpolice.connect_selected_notify(move |_| e());
    let e = ecrire;
    spin.connect_value_changed(move |_| e());

    // Thème : un thème fichier est montré comme « personnalisé », sans jamais être écrasé.
    let (courant, perso) = match &config.theme {
        ThemeChoix::Systeme => ("systeme".to_string(), None),
        ThemeChoix::Sombre => ("sombre".to_string(), None),
        ThemeChoix::Clair => ("clair".to_string(), None),
        ThemeChoix::Fichier(_) => ("\u{0}fichier".to_string(), Some("\u{0}fichier".to_string())),
    };
    let perso_libelle = t("reglages-theme-personnalise");
    liste_ecrite(
        champs,
        etat,
        "theme",
        &t("reglages-theme"),
        &choix_i18n(THEMES),
        &courant,
        perso.map(|v| (v, perso_libelle)),
    );

    // Shell : shell de connexion (valeur vide) ou un shell installé.
    let mut shells = vec![(String::new(), t("reglages-shell-connexion"))];
    shells.extend(
        shells_proposes(&path_courant())
            .into_iter()
            .map(|s| (s.nom.clone(), s.nom)),
    );
    liste_ecrite(
        champs,
        etat,
        "shell",
        &t("reglages-shell"),
        &shells,
        &config.shell,
        None,
    );

    let invite = match config.invite {
        Invite::Starship => "starship",
        Invite::Shell => "shell",
    };
    liste_ecrite(
        champs,
        etat,
        "invite",
        &t("reglages-invite"),
        &choix_i18n(INVITES),
        invite,
        None,
    );
    let annonce = match config.annonce_fin_commande {
        AnnonceFin::Longues => "longues",
        AnnonceFin::Toutes => "toutes",
        AnnonceFin::Aucune => "aucune",
    };
    liste_ecrite(
        champs,
        etat,
        "annonce_fin_commande",
        &t("reglages-annonce"),
        &choix_i18n(ANNONCES),
        annonce,
        None,
    );
    case_ecrite(
        champs,
        etat,
        &["copie_selection"],
        &t("reglages-copie-selection"),
        config.copie_selection,
    );
    let bandeau = match config.bandeau_clavier {
        BandeauClavier::Nom => "nom",
        BandeauClavier::Code => "code",
        BandeauClavier::Masque => "masque",
    };
    liste_ecrite(
        champs,
        etat,
        "bandeau_clavier",
        &t("reglages-bandeau"),
        &choix_i18n(BANDEAUX),
        bandeau,
        None,
    );
    let n = &config.notifications;
    let valeurs = [
        n.fin_commande_onglet,
        n.fin_commande_bulle,
        n.programme_onglet,
        n.programme_bulle,
        n.presse_papiers,
        n.cloche_onglet,
    ];
    for ((chemin, libelle), actif) in NOTIFICATIONS.iter().zip(valeurs) {
        case_ecrite(champs, etat, chemin, &t(libelle), actif);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn police_et_taille_separees() {
        assert_eq!(police_et_taille("B612 Mono 11"), ("B612 Mono".into(), 11.0));
        assert_eq!(police_et_taille("Iosevka"), ("Iosevka".into(), 11.0));
        assert_eq!(police_et_taille("X 200"), ("X".into(), 72.0));
        assert_eq!(police_et_taille("X 2"), ("X".into(), 6.0));
    }

    #[test]
    fn themes_ont_des_valeurs_connues() {
        for (v, _) in THEMES {
            let d = tempfile::tempdir().unwrap();
            let f = d.path().join("pupitre.toml");
            std::fs::write(&f, format!("theme = \"{v}\"\n")).unwrap();
            assert!(charger(&f).is_ok(), "{v}");
        }
    }
}
