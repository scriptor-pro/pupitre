// SPDX-License-Identifier: GPL-3.0-or-later
//! Fenêtres et onglets. [`Pupitre`] tient la connexion au processus de fond et distribue ses
//! messages aux onglets ; chaque [`FenetrePupitre`] porte un carnet d'onglets.
//!
//! Reprise après plantage : au démarrage, les sessions déjà présentes dans le processus de
//! fond sont celles d'une interface disparue (l'application est unique par session de
//! bureau) ; elles sont rattachées et leur tampon de relecture est rejoué.

use crate::apparence::{suivre_preference_systeme, theme_systeme};
use crate::client::{connect_or_spawn_at, Client, CONNEXION_PERDUE};
use crate::taille::{largeur_ecran, taille_ouverture};
use crate::terminal::TerminalOnglet;
use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use pupitre_config::{theme_clair, theme_sombre, Config, Theme, ThemeChoix};
use pupitre_proto::{ClientMsg, ServerMsg, SessionId};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::Duration;
use vte4::prelude::*;

/// Grille annoncée pour une session tant que la vraie n'est pas connue.
const GRILLE_PAR_DEFAUT: (u16, u16) = (80, 24);
/// Délai avant de provoquer un redessin après la relecture (le temps d'une allocation).
const DELAI_REDESSIN: Duration = Duration::from_millis(150);
const TITRE_PAR_DEFAUT: &str = "Terminal";
const GROUPE_ONGLETS: &str = "pupitre-onglets";

const MSG_FOND_ARRETE: &str = "Le processus de fond s'est arrêté : les shells ouverts sont perdus.";
const BOUTON_NOUVEAU_TERMINAL: &str = "Ouvrir un nouveau terminal";

/// Action de fenêtre (préfixe `win.`) déclenchée par une combinaison de touches.
/// Ctrl+Maj+T, Ctrl+Maj+W, Ctrl+Maj+N (conception §12.1) ; les autres modificateurs
/// (Alt, Super) doivent être absents.
pub fn action_pour_touche(touche: gdk::Key, mods: gdk::ModifierType) -> Option<&'static str> {
    let utiles = gdk::ModifierType::CONTROL_MASK
        | gdk::ModifierType::SHIFT_MASK
        | gdk::ModifierType::ALT_MASK
        | gdk::ModifierType::SUPER_MASK
        | gdk::ModifierType::META_MASK;
    if mods & utiles != gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::SHIFT_MASK {
        return None;
    }
    match touche.to_lower() {
        gdk::Key::t => Some("nouvel-onglet"),
        gdk::Key::w => Some("fermer-onglet"),
        gdk::Key::n => Some("nouvelle-fenetre"),
        _ => None,
    }
}

/// Un onglet : un terminal et son étiquette (titre ou nom choisi, modifiable sur place).
pub struct Onglet {
    terminal: TerminalOnglet,
    pile: gtk::Stack,
    etiquette: gtk::Label,
    entree: gtk::Entry,
    nom: RefCell<Option<String>>,
    occupe: Cell<bool>,
    termine: Cell<bool>,
    /// Rattaché après une reprise : la sortie est ignorée jusqu'à la relecture.
    attend_relecture: Cell<bool>,
}

impl Onglet {
    pub fn terminal(&self) -> &TerminalOnglet {
        &self.terminal
    }

    pub fn widget(&self) -> gtk::Widget {
        self.terminal.widget().clone().upcast()
    }

    /// Une commande tourne au premier plan dans le shell.
    pub fn occupe(&self) -> bool {
        self.occupe.get()
    }

    /// Le shell s'est terminé ; Entrée en ouvre un nouveau.
    pub fn termine(&self) -> bool {
        self.termine.get()
    }

    /// Titre affiché : nom choisi, sinon titre du programme ou dossier courant.
    pub fn titre(&self) -> String {
        if let Some(n) = self.nom.borrow().as_ref() {
            return n.clone();
        }
        self.terminal
            .titre()
            .unwrap_or_else(|| TITRE_PAR_DEFAUT.to_string())
    }

    /// Donne un nom à l'onglet (`None` : revenir au titre automatique).
    pub fn renommer(&self, nom: Option<&str>) {
        let nom = nom
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(String::from);
        *self.nom.borrow_mut() = nom;
        self.rafraichir_titre();
    }

    fn rafraichir_titre(&self) {
        let titre = self.titre();
        self.etiquette.set_text(&titre);
        self.etiquette.set_tooltip_text(Some(&titre));
        // Le nom accessible de l'onglet suit son étiquette ; le terminal porte le même nom.
        self.terminal
            .terminal()
            .update_property(&[gtk::accessible::Property::Label(&titre)]);
        if let Some(c) = self.carnet() {
            c.page(&self.widget()).set_property("menu-label", &titre);
            if c.page_num(&self.widget()) == c.current_page() {
                if let Some(f) = c.root().and_downcast::<gtk::Window>() {
                    f.set_title(Some(&format!("{titre} — Pupitre")));
                }
            }
        }
    }

    /// Carnet qui contient l'onglet (une page n'en est pas l'enfant direct sous GTK 4).
    fn carnet(&self) -> Option<gtk::Notebook> {
        self.widget()
            .ancestor(gtk::Notebook::static_type())
            .and_downcast::<gtk::Notebook>()
    }

    fn commencer_renommage(&self) {
        self.entree.set_text(&self.titre());
        self.pile.set_visible_child(&self.entree);
        self.entree.grab_focus();
        self.entree.select_region(0, -1);
    }

    fn finir_renommage(&self, valider: bool) {
        if self.pile.visible_child().as_ref() != Some(self.entree.upcast_ref()) {
            return;
        }
        if valider {
            let texte = self.entree.text().to_string();
            // Valider le titre automatique tel quel ne fige pas de nom.
            if texte != self.titre() {
                self.renommer(Some(&texte));
            }
        }
        self.pile.set_visible_child(&self.etiquette);
        self.terminal.widget().grab_focus();
    }

    fn marquer_termine(&self, code: Option<i32>) {
        self.termine.set(true);
        self.occupe.set(false);
        self.terminal.rendre_muet(true);
        let code = code.map_or_else(|| "signal".to_string(), |c| c.to_string());
        let message = format!(
            "\r\n\x1b[0mLe shell s'est terminé (code {code}). \
             Appuyez sur Entrée pour en ouvrir un nouveau.\r\n"
        );
        self.terminal.on_output(message.as_bytes());
        self.terminal.terminal().announce(
            &message.replace(['\r', '\n'], " "),
            gtk::AccessibleAnnouncementPriority::Medium,
        );
    }
}

enum Attente {
    /// `Open` envoyé pour un nouvel onglet de cette fenêtre.
    NouvelOnglet(Weak<FenetreInterne>),
    /// `Open` envoyé pour remplacer le shell terminé de cet onglet.
    Remplacer(Weak<Onglet>),
}

/// L'application : connexion au processus de fond, fenêtres, onglets, thème.
pub struct Pupitre {
    app: gtk::Application,
    socket: PathBuf,
    config: Config,
    theme: RefCell<Theme>,
    client: RefCell<Option<Rc<Client>>>,
    /// Numéro de la connexion en cours : les messages d'une connexion remplacée sont ignorés.
    generation: Cell<u64>,
    attend_liste: Cell<bool>,
    attentes: RefCell<VecDeque<Attente>>,
    onglets: RefCell<Vec<Rc<Onglet>>>,
    fenetres: RefCell<Vec<FenetrePupitre>>,
    /// Garde l'application en vie tant que la première fenêtre n'existe pas (GTK quitte
    /// une application sans fenêtre).
    garde: RefCell<Option<gio::ApplicationHoldGuard>>,
}

impl Pupitre {
    /// Se connecte au processus de fond (en le lançant au besoin), puis ouvre la première
    /// fenêtre : avec les sessions orphelines s'il y en a (reprise), sinon avec un shell neuf.
    pub fn demarrer(app: &gtk::Application, socket: PathBuf, config: Config) -> Rc<Pupitre> {
        let theme = theme_initial(&config.theme);
        let p = Rc::new(Pupitre {
            app: app.clone(),
            socket,
            config,
            theme: RefCell::new(theme.clone()),
            client: RefCell::new(None),
            generation: Cell::new(0),
            attend_liste: Cell::new(false),
            attentes: RefCell::new(VecDeque::new()),
            onglets: RefCell::new(Vec::new()),
            fenetres: RefCell::new(Vec::new()),
            garde: RefCell::new(Some(app.hold())),
        });
        p.changer_theme(theme);
        // Fenêtre fermée (quelle qu'en soit la raison) : retirée de la liste. Le signal
        // `destroy` ne convient pas : il n'arrive qu'à la dernière référence relâchée.
        let faible = Rc::downgrade(&p);
        app.connect_window_removed(move |_, w| {
            if let Some(p) = faible.upgrade() {
                p.fenetres.borrow_mut().retain(|f| {
                    let garder = f.0.fenetre.upcast_ref::<gtk::Window>() != w;
                    if !garder {
                        f.0.fermee.set(true);
                    }
                    garder
                });
            }
        });
        if p.config.theme == ThemeChoix::Systeme {
            let faible = Rc::downgrade(&p);
            suivre_preference_systeme(move |pref| {
                if let Some(p) = faible.upgrade() {
                    p.changer_theme(theme_systeme(pref));
                }
            });
        }
        match p.connecter() {
            Ok(()) => {
                p.attend_liste.set(true);
                p.envoyer(ClientMsg::List);
            }
            Err(e) => {
                let f = FenetrePupitre::nouvelle(app, &p);
                f.signaler_panne(&format!(
                    "Pupitre ne peut pas joindre le processus de fond : {e}"
                ));
                f.fenetre().present();
            }
        }
        p
    }

    /// Fenêtres ouvertes.
    pub fn fenetres(&self) -> Vec<FenetrePupitre> {
        self.fenetres.borrow().clone()
    }

    /// Nouvelle fenêtre avec un nouveau shell (Ctrl+Maj+N, second lancement de Pupitre).
    pub fn nouvelle_fenetre(self: &Rc<Self>) -> FenetrePupitre {
        let f = FenetrePupitre::nouvelle(&self.app, self);
        if self.client.borrow().is_some() {
            f.ouvrir_session();
        } else {
            f.signaler_panne(MSG_FOND_ARRETE);
        }
        f.fenetre().present();
        f
    }

    /// Ouvre un nouveau shell dans l'onglet `onglet`, dont le shell s'est terminé.
    pub fn rouvrir(self: &Rc<Self>, onglet: &Rc<Onglet>) {
        if !onglet.termine() || self.client.borrow().is_none() {
            return;
        }
        let (cols, rows) = onglet.terminal.grille().unwrap_or(GRILLE_PAR_DEFAUT);
        self.attentes
            .borrow_mut()
            .push_back(Attente::Remplacer(Rc::downgrade(onglet)));
        self.envoyer(ClientMsg::Open {
            argv: None,
            cwd: None,
            cols,
            rows,
        });
    }

    fn connecter(self: &Rc<Self>) -> Result<(), String> {
        let (client, recus) = connect_or_spawn_at(&self.socket).map_err(|e| e.to_string())?;
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        *self.client.borrow_mut() = Some(Rc::new(client));
        let faible = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            while let Ok(m) = recus.recv().await {
                let Some(p) = faible.upgrade() else { break };
                if p.generation.get() != generation {
                    break;
                }
                p.traiter(m);
            }
        });
        Ok(())
    }

    fn envoyer(&self, m: ClientMsg) {
        if let Some(c) = self.client.borrow().as_ref() {
            // Échec = connexion perdue, signalée par ailleurs sur le canal.
            let _ = c.send(m);
        }
    }

    fn changer_theme(&self, theme: Theme) {
        // Les éléments GTK (onglets, menus) suivent le même clair/sombre que le terminal.
        if let Some(r) = gtk::Settings::default() {
            r.set_gtk_application_prefer_dark_theme(theme.nom != "clair");
        }
        for o in self.onglets.borrow().iter() {
            o.terminal.appliquer(&self.config, &theme);
        }
        *self.theme.borrow_mut() = theme;
    }

    fn onglet(&self, id: SessionId) -> Option<Rc<Onglet>> {
        self.onglets
            .borrow()
            .iter()
            .find(|o| !o.termine() && o.terminal.id() == id)
            .cloned()
    }

    fn onglet_de_widget(&self, w: &gtk::Widget) -> Option<Rc<Onglet>> {
        self.onglets
            .borrow()
            .iter()
            .find(|o| o.terminal.widget().upcast_ref::<gtk::Widget>() == w)
            .cloned()
    }

    fn traiter(self: &Rc<Self>, m: ServerMsg) {
        match m {
            ServerMsg::Sessions(sessions) if self.attend_liste.replace(false) => {
                let f = FenetrePupitre::nouvelle(&self.app, self);
                if sessions.is_empty() {
                    f.ouvrir_session();
                } else {
                    for s in sessions {
                        if let Some(o) = f.ajouter_onglet(s.id) {
                            o.occupe.set(s.busy);
                            o.attend_relecture.set(true);
                            self.envoyer(ClientMsg::Attach { id: s.id });
                        }
                    }
                }
                f.fenetre().present();
            }
            ServerMsg::Opened { id } => {
                let attente = self.attentes.borrow_mut().pop_front();
                match attente {
                    Some(Attente::NouvelOnglet(f)) => match f.upgrade() {
                        Some(f) if !f.fermee.get() => {
                            FenetrePupitre(f).ajouter_onglet(id);
                        }
                        _ => self.envoyer(ClientMsg::Close { id }),
                    },
                    Some(Attente::Remplacer(o)) => match o.upgrade() {
                        Some(o) if o.carnet().is_some() => {
                            o.terminal.relier(id);
                            o.termine.set(false);
                            o.occupe.set(false);
                            // La grille a pu changer depuis la demande.
                            o.terminal.forcer_redessin();
                        }
                        _ => self.envoyer(ClientMsg::Close { id }),
                    },
                    None => self.envoyer(ClientMsg::Close { id }),
                }
            }
            ServerMsg::OpenFailed { error } => {
                let message = format!("Le shell n'a pas pu être lancé : {error}");
                let attente = self.attentes.borrow_mut().pop_front();
                match attente {
                    Some(Attente::NouvelOnglet(f)) => {
                        if let Some(f) = f.upgrade() {
                            FenetrePupitre(f).afficher_message(&message, None);
                        }
                    }
                    Some(Attente::Remplacer(o)) => {
                        if let Some(o) = o.upgrade() {
                            o.terminal
                                .on_output(format!("\r\n{message}\r\n").as_bytes());
                        }
                    }
                    None => eprintln!("pupitre: {message}"),
                }
            }
            ServerMsg::Output { id, data } => {
                if let Some(o) = self.onglet(id).filter(|o| !o.attend_relecture.get()) {
                    o.terminal.on_output(&data);
                }
            }
            ServerMsg::Replay { id, data } => {
                if let Some(o) = self.onglet(id) {
                    o.attend_relecture.set(false);
                    o.terminal.on_output(&data);
                    let faible = Rc::downgrade(&o);
                    glib::timeout_add_local_once(DELAI_REDESSIN, move || {
                        if let Some(o) = faible.upgrade() {
                            o.terminal.forcer_redessin();
                        }
                    });
                }
            }
            ServerMsg::Busy { id, busy } => {
                if let Some(o) = self.onglet(id) {
                    o.occupe.set(busy);
                }
            }
            ServerMsg::Exited { id, code } => {
                if let Some(o) = self.onglet(id) {
                    o.marquer_termine(code);
                }
            }
            ServerMsg::Error(e) if e == CONNEXION_PERDUE => self.connexion_perdue(),
            ServerMsg::Error(e) => eprintln!("pupitre: processus de fond : {e}"),
            ServerMsg::Welcome { .. } | ServerMsg::Sessions(_) => {}
        }
    }

    fn connexion_perdue(self: &Rc<Self>) {
        *self.client.borrow_mut() = None;
        self.generation.set(self.generation.get() + 1);
        self.attend_liste.set(false);
        self.attentes.borrow_mut().clear();
        for o in self.onglets.borrow().iter() {
            o.termine.set(true);
            o.occupe.set(false);
            o.terminal.rendre_muet(true);
        }
        if self.fenetres.borrow().is_empty() {
            // Perte avant la première fenêtre : il en faut une pour le dire.
            let f = FenetrePupitre::nouvelle(&self.app, self);
            f.fenetre().present();
        }
        for f in self.fenetres() {
            f.signaler_panne(MSG_FOND_ARRETE);
        }
    }

    /// Relance le processus de fond et ouvre un terminal neuf dans `fenetre` ; les onglets
    /// des shells perdus sont fermés (les fenêtres vidées se ferment).
    fn reconnecter(self: &Rc<Self>, fenetre: &FenetrePupitre) {
        if let Err(e) = self.connecter() {
            fenetre.signaler_panne(&format!(
                "Impossible de relancer le processus de fond : {e}"
            ));
            return;
        }
        fenetre.0.garder_ouverte.set(true);
        let perdus: Vec<Rc<Onglet>> = self
            .onglets
            .borrow()
            .iter()
            .filter(|o| o.termine())
            .cloned()
            .collect();
        for o in perdus {
            self.retirer_onglet(&o);
        }
        for f in self.fenetres() {
            f.masquer_message();
        }
        fenetre.ouvrir_session();
    }

    /// Retire l'onglet de sa fenêtre et de la liste, sans toucher à sa session.
    fn retirer_onglet(&self, o: &Rc<Onglet>) {
        self.onglets.borrow_mut().retain(|x| !Rc::ptr_eq(x, o));
        if let Some(c) = o.carnet() {
            if let Some(n) = c.page_num(&o.widget()) {
                c.remove_page(Some(n));
            }
        }
    }

    /// Ferme l'onglet et termine sa session (si elle tourne encore).
    fn fermer_onglet(&self, o: &Rc<Onglet>) {
        if !o.termine() {
            self.envoyer(ClientMsg::Close {
                id: o.terminal.id(),
            });
        }
        self.retirer_onglet(o);
    }
}

fn theme_initial(choix: &ThemeChoix) -> Theme {
    match choix {
        ThemeChoix::Sombre | ThemeChoix::Systeme => theme_sombre(),
        ThemeChoix::Clair => theme_clair(),
        ThemeChoix::Fichier(chemin) => {
            // Les thèmes en fichier arrivent avec le rechargement de configuration (Task 10).
            eprintln!(
                "pupitre: thème {} pas encore pris en charge ; thème sombre utilisé",
                chemin.display()
            );
            theme_sombre()
        }
    }
}

/// Action du bouton du bandeau de messages.
pub type ActionMessage = Rc<dyn Fn()>;

pub struct FenetreInterne {
    fenetre: gtk::ApplicationWindow,
    carnet: gtk::Notebook,
    bandeau: gtk::Box,
    bandeau_texte: gtk::Label,
    bandeau_bouton: gtk::Button,
    bandeau_action: RefCell<Option<ActionMessage>>,
    /// Ne pas fermer la fenêtre quand son dernier onglet part (reconnexion en cours).
    garder_ouverte: Cell<bool>,
    fermee: Cell<bool>,
    pupitre: Weak<Pupitre>,
}

/// Une fenêtre de Pupitre : bandeau de messages et carnet d'onglets.
#[derive(Clone)]
pub struct FenetrePupitre(Rc<FenetreInterne>);

impl FenetrePupitre {
    pub fn nouvelle(app: &gtk::Application, pupitre: &Rc<Pupitre>) -> Self {
        let (largeur, hauteur) = taille_ouverture(largeur_ecran().unwrap_or(0));
        let fenetre = gtk::ApplicationWindow::builder()
            .application(app)
            .title("Pupitre")
            .default_width(largeur)
            .default_height(hauteur)
            .build();

        let carnet = gtk::Notebook::builder()
            .scrollable(true)
            .show_border(false)
            .group_name(GROUPE_ONGLETS)
            .vexpand(true)
            .hexpand(true)
            .build();
        carnet.popup_enable();
        carnet.set_action_widget(&bouton_menu(), gtk::PackType::End);

        let bandeau_texte = gtk::Label::builder()
            .wrap(true)
            .selectable(true)
            .xalign(0.0)
            .hexpand(true)
            .build();
        let bandeau_bouton = gtk::Button::new();
        let bandeau = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .margin_start(12)
            .margin_end(12)
            .margin_top(6)
            .margin_bottom(6)
            .visible(false)
            .accessible_role(gtk::AccessibleRole::Alert)
            .build();
        bandeau.append(&bandeau_texte);
        bandeau.append(&bandeau_bouton);

        let boite = gtk::Box::new(gtk::Orientation::Vertical, 0);
        boite.append(&bandeau);
        boite.append(&carnet);
        fenetre.set_child(Some(&boite));

        let f = FenetrePupitre(Rc::new(FenetreInterne {
            fenetre,
            carnet,
            bandeau,
            bandeau_texte,
            bandeau_bouton,
            bandeau_action: RefCell::new(None),
            garder_ouverte: Cell::new(false),
            fermee: Cell::new(false),
            pupitre: Rc::downgrade(pupitre),
        }));
        let faible = Rc::downgrade(&f.0);
        f.0.bandeau_bouton.connect_clicked(move |_| {
            // Action copiée avant l'appel : elle peut remplacer ou masquer le message.
            let action = faible
                .upgrade()
                .and_then(|f| f.bandeau_action.borrow().clone());
            if let Some(a) = action {
                a();
            }
        });
        f.installer_actions();
        f.installer_signaux();
        pupitre.fenetres.borrow_mut().push(f.clone());
        pupitre.garde.borrow_mut().take();
        f
    }

    pub fn fenetre(&self) -> &gtk::ApplicationWindow {
        &self.0.fenetre
    }

    fn pupitre(&self) -> Option<Rc<Pupitre>> {
        self.0.pupitre.upgrade()
    }

    /// Onglets de la fenêtre, dans l'ordre du carnet.
    pub fn onglets(&self) -> Vec<Rc<Onglet>> {
        let Some(p) = self.pupitre() else {
            return Vec::new();
        };
        (0..self.0.carnet.n_pages())
            .filter_map(|i| self.0.carnet.nth_page(Some(i)))
            .filter_map(|w| p.onglet_de_widget(&w))
            .collect()
    }

    /// Nombre de pages du carnet (égal au nombre d'onglets, sauf défaut).
    pub fn nombre_pages(&self) -> usize {
        self.0.carnet.n_pages() as usize
    }

    pub fn onglet_actif(&self) -> Option<Rc<Onglet>> {
        let w = self.0.carnet.nth_page(self.0.carnet.current_page())?;
        self.pupitre()?.onglet_de_widget(&w)
    }

    /// Ajoute un onglet relié à la session existante `id`, et le rend actif.
    /// `None` sans connexion au processus de fond.
    pub fn ajouter_onglet(&self, id: SessionId) -> Option<Rc<Onglet>> {
        let p = self.pupitre()?;
        let client = p.client.borrow().clone()?;
        let o = creer_onglet(&p, client, id);
        p.onglets.borrow_mut().push(o.clone());
        self.inserer(&o);
        Some(o)
    }

    fn inserer(&self, o: &Rc<Onglet>) {
        let c = &self.0.carnet;
        let n = c.append_page(&o.widget(), Some(&o.pile));
        c.set_tab_reorderable(&o.widget(), true);
        c.set_tab_detachable(&o.widget(), true);
        c.set_current_page(Some(n));
        self.0.garder_ouverte.set(false);
        o.rafraichir_titre();
        o.terminal.widget().grab_focus();
    }

    /// Ouvre un nouveau shell dans un nouvel onglet de cette fenêtre.
    pub fn ouvrir_session(&self) {
        let Some(p) = self.pupitre() else { return };
        let (cols, rows) = self
            .onglet_actif()
            .and_then(|o| o.terminal.grille())
            .unwrap_or(GRILLE_PAR_DEFAUT);
        p.attentes
            .borrow_mut()
            .push_back(Attente::NouvelOnglet(Rc::downgrade(&self.0)));
        p.envoyer(ClientMsg::Open {
            argv: None,
            cwd: None,
            cols,
            rows,
        });
    }

    /// Déplace l'onglet de la session `id` dans une nouvelle fenêtre (s'il n'est pas seul).
    pub fn detacher_onglet(&self, id: SessionId) {
        let Some(p) = self.pupitre() else { return };
        let Some(o) = self.onglets().into_iter().find(|o| o.terminal.id() == id) else {
            return;
        };
        if self.0.carnet.n_pages() < 2 {
            return;
        }
        let nouvelle = FenetrePupitre::nouvelle(&p.app, &p);
        self.0.carnet.detach_tab(&o.widget());
        nouvelle.inserer(&o);
        nouvelle.fenetre().present();
    }

    /// Texte du bandeau de messages, s'il est affiché.
    pub fn message(&self) -> Option<String> {
        self.0
            .bandeau
            .is_visible()
            .then(|| self.0.bandeau_texte.text().to_string())
    }

    /// Équivaut à un clic sur le bouton du bandeau.
    pub fn activer_message(&self) {
        if self.0.bandeau_bouton.is_visible() {
            self.0.bandeau_bouton.emit_clicked();
        }
    }

    /// Affiche un message persistant (lu par le lecteur d'écran), avec un bouton éventuel.
    pub fn afficher_message(&self, texte: &str, bouton: Option<(&str, ActionMessage)>) {
        self.0.bandeau_texte.set_text(texte);
        match bouton {
            Some((etiquette, action)) => {
                self.0.bandeau_bouton.set_label(etiquette);
                self.0.bandeau_bouton.set_visible(true);
                *self.0.bandeau_action.borrow_mut() = Some(action);
            }
            None => {
                self.0.bandeau_bouton.set_visible(false);
                *self.0.bandeau_action.borrow_mut() = None;
            }
        }
        self.0.bandeau.set_visible(true);
        self.0
            .bandeau
            .announce(texte, gtk::AccessibleAnnouncementPriority::High);
    }

    pub fn masquer_message(&self) {
        self.0.bandeau.set_visible(false);
        *self.0.bandeau_action.borrow_mut() = None;
    }

    fn signaler_panne(&self, texte: &str) {
        let faible = Rc::downgrade(&self.0);
        self.afficher_message(
            texte,
            Some((
                BOUTON_NOUVEAU_TERMINAL,
                Rc::new(move || {
                    if let Some(f) = faible.upgrade() {
                        let f = FenetrePupitre(f);
                        if let Some(p) = f.pupitre() {
                            p.reconnecter(&f);
                        }
                    }
                }),
            )),
        );
        self.0.bandeau_bouton.grab_focus();
    }

    fn installer_actions(&self) {
        let ajouter = |nom: &str, f: Box<dyn Fn(&FenetrePupitre)>| {
            let action = gio::SimpleAction::new(nom, None);
            let faible = Rc::downgrade(&self.0);
            action.connect_activate(move |_, _| {
                if let Some(w) = faible.upgrade() {
                    f(&FenetrePupitre(w));
                }
            });
            self.0.fenetre.add_action(&action);
        };
        ajouter("nouvel-onglet", Box::new(|f| f.ouvrir_session()));
        ajouter(
            "nouvelle-fenetre",
            Box::new(|f| {
                if let Some(p) = f.pupitre() {
                    p.nouvelle_fenetre();
                }
            }),
        );
        ajouter(
            "fermer-onglet",
            Box::new(|f| {
                if let Some(o) = f.onglet_actif() {
                    f.demander_fermeture_onglet(&o);
                }
            }),
        );
        ajouter(
            "renommer-onglet",
            Box::new(|f| {
                if let Some(o) = f.onglet_actif() {
                    o.commencer_renommage();
                }
            }),
        );
        ajouter(
            "detacher-onglet",
            Box::new(|f| {
                if let Some(o) = f.onglet_actif() {
                    f.detacher_onglet(o.terminal.id());
                }
            }),
        );

        // Raccourcis : interceptés avant le terminal (sinon VTE consomme les touches).
        let touches = gtk::EventControllerKey::new();
        touches.set_propagation_phase(gtk::PropagationPhase::Capture);
        let fenetre = self.0.fenetre.clone();
        touches.connect_key_pressed(move |_, touche, _, mods| {
            match action_pour_touche(touche, mods) {
                Some(a) => {
                    let _ = WidgetExt::activate_action(&fenetre, &format!("win.{a}"), None);
                    glib::Propagation::Stop
                }
                None => glib::Propagation::Proceed,
            }
        });
        self.0.fenetre.add_controller(touches);
    }

    fn installer_signaux(&self) {
        let faible = Rc::downgrade(&self.0);
        self.0.carnet.connect_page_removed(move |c, _, _| {
            if let Some(f) = faible.upgrade() {
                if c.n_pages() == 0 && !f.garder_ouverte.get() && !f.fermee.get() {
                    FenetrePupitre(f).detruire();
                }
            }
        });
        // Onglet glissé depuis une autre fenêtre : mêmes réglages que les autres.
        self.0.carnet.connect_page_added(|c, page, _| {
            c.set_tab_reorderable(page, true);
            c.set_tab_detachable(page, true);
        });
        let faible = Rc::downgrade(&self.0);
        self.0.carnet.connect_switch_page(move |_, page, _| {
            if let Some(f) = faible.upgrade() {
                if let Some(o) = f.pupitre.upgrade().and_then(|p| p.onglet_de_widget(page)) {
                    f.fenetre
                        .set_title(Some(&format!("{} — Pupitre", o.titre())));
                    o.terminal.widget().grab_focus();
                }
            }
        });
        // Onglet lâché hors de toute fenêtre : nouvelle fenêtre.
        let faible = Rc::downgrade(&self.0);
        self.0.carnet.connect_create_window(move |_, _| {
            let f = faible.upgrade()?;
            let p = f.pupitre.upgrade()?;
            let nouvelle = FenetrePupitre::nouvelle(&p.app, &p);
            nouvelle.fenetre().present();
            Some(nouvelle.0.carnet.clone())
        });
        let faible = Rc::downgrade(&self.0);
        self.0.fenetre.connect_close_request(move |_| {
            let Some(f) = faible.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let f = FenetrePupitre(f);
            let occupes = f.onglets().iter().filter(|o| o.occupe()).count();
            if occupes == 0 {
                f.fermer_tout();
                return glib::Propagation::Proceed;
            }
            let question = if occupes == 1 {
                "Une commande est en cours dans un onglet de cette fenêtre. Fermer quand même ?"
                    .to_string()
            } else {
                format!(
                    "Des commandes sont en cours dans {occupes} onglets de cette fenêtre. \
                     Fermer quand même ?"
                )
            };
            let g = f.clone();
            confirmer(f.fenetre(), &question, move || {
                g.fermer_tout();
                g.detruire();
            });
            glib::Propagation::Stop
        });
    }

    fn demander_fermeture_onglet(&self, o: &Rc<Onglet>) {
        let Some(p) = self.pupitre() else { return };
        if !o.occupe() {
            p.fermer_onglet(o);
            return;
        }
        let faible = Rc::downgrade(o);
        let pf = Rc::downgrade(&p);
        confirmer(
            self.fenetre(),
            "Une commande est en cours dans cet onglet. Fermer quand même ?",
            move || {
                if let (Some(o), Some(p)) = (faible.upgrade(), pf.upgrade()) {
                    p.fermer_onglet(&o);
                }
            },
        );
    }

    /// Termine les sessions de tous les onglets de la fenêtre.
    fn fermer_tout(&self) {
        self.0.fermee.set(true);
        if let Some(p) = self.pupitre() {
            for o in self.onglets() {
                p.fermer_onglet(&o);
            }
        }
    }

    fn detruire(&self) {
        self.0.fermee.set(true);
        self.0.fenetre.destroy();
    }
}

fn creer_onglet(p: &Rc<Pupitre>, client: Rc<Client>, id: SessionId) -> Rc<Onglet> {
    let terminal = TerminalOnglet::new(client, id, &p.config, &p.theme.borrow());
    let etiquette = gtk::Label::builder()
        .label(TITRE_PAR_DEFAUT)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .width_chars(8)
        .max_width_chars(24)
        .build();
    let entree = gtk::Entry::builder()
        .width_chars(16)
        .tooltip_text("Nom de l'onglet (vide : titre automatique)")
        .build();
    entree.update_property(&[gtk::accessible::Property::Label("Nom de l'onglet")]);
    let pile = gtk::Stack::new();
    pile.add_child(&etiquette);
    pile.add_child(&entree);
    pile.set_visible_child(&etiquette);

    let o = Rc::new(Onglet {
        terminal,
        pile,
        etiquette,
        entree,
        nom: RefCell::new(None),
        occupe: Cell::new(false),
        termine: Cell::new(false),
        attend_relecture: Cell::new(false),
    });

    // Titre : à chaque propriété de terminal changée (titre OSC 0/2, dossier OSC 7).
    let faible = Rc::downgrade(&o);
    o.terminal
        .terminal()
        .connect_termprop_changed(None, move |_, _| {
            if let Some(o) = faible.upgrade() {
                o.rafraichir_titre();
            }
        });

    // Renommage : Entrée valide, Échap annule.
    let faible = Rc::downgrade(&o);
    o.entree.connect_activate(move |_| {
        if let Some(o) = faible.upgrade() {
            o.finir_renommage(true);
        }
    });
    let echap = gtk::EventControllerKey::new();
    let faible = Rc::downgrade(&o);
    echap.connect_key_pressed(move |_, touche, _, _| {
        if touche == gdk::Key::Escape {
            if let Some(o) = faible.upgrade() {
                o.finir_renommage(false);
            }
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    o.entree.add_controller(echap);

    // Double-clic sur l'étiquette : renommer ; clic droit : menu de l'onglet.
    let clic = gtk::GestureClick::new();
    clic.set_button(0);
    let faible = Rc::downgrade(&o);
    clic.connect_pressed(move |geste, n, x, y| {
        let Some(o) = faible.upgrade() else { return };
        if let Some(c) = o.carnet() {
            c.set_current_page(c.page_num(&o.widget()));
        }
        match geste.current_button() {
            1 if n == 2 => o.commencer_renommage(),
            3 => {
                let menu = gtk::PopoverMenu::from_model(Some(&menu_onglet()));
                menu.set_parent(&o.pile);
                menu.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
                menu.connect_closed(|m| {
                    let m = m.clone();
                    glib::idle_add_local_once(move || m.unparent());
                });
                menu.popup();
            }
            _ => {}
        }
    });
    o.pile.add_controller(clic);

    // Shell terminé : Entrée en ouvre un nouveau dans le même onglet.
    let entree_terminal = gtk::EventControllerKey::new();
    entree_terminal.set_propagation_phase(gtk::PropagationPhase::Capture);
    let faible = Rc::downgrade(&o);
    let pf = Rc::downgrade(p);
    entree_terminal.connect_key_pressed(move |_, touche, _, _| {
        if let (Some(o), Some(p)) = (faible.upgrade(), pf.upgrade()) {
            if o.termine() && matches!(touche, gdk::Key::Return | gdk::Key::KP_Enter) {
                p.rouvrir(&o);
                return glib::Propagation::Stop;
            }
        }
        glib::Propagation::Proceed
    });
    o.terminal.terminal().add_controller(entree_terminal);
    o
}

fn menu_onglet() -> gio::Menu {
    let m = gio::Menu::new();
    m.append(Some("Renommer l'onglet"), Some("win.renommer-onglet"));
    m.append(
        Some("Détacher dans une nouvelle fenêtre"),
        Some("win.detacher-onglet"),
    );
    m.append(Some("Fermer l'onglet"), Some("win.fermer-onglet"));
    m
}

/// Menu principal (F10), à droite de la barre d'onglets.
fn bouton_menu() -> gtk::MenuButton {
    let m = gio::Menu::new();
    let item = |libelle: &str, action: &str, accel: Option<&str>| {
        let i = gio::MenuItem::new(Some(libelle), Some(action));
        if let Some(a) = accel {
            i.set_attribute_value("accel", Some(&a.to_variant()));
        }
        i
    };
    m.append_item(&item(
        "Nouvel onglet",
        "win.nouvel-onglet",
        Some("<Control><Shift>t"),
    ));
    m.append_item(&item(
        "Nouvelle fenêtre",
        "win.nouvelle-fenetre",
        Some("<Control><Shift>n"),
    ));
    let onglet = gio::Menu::new();
    onglet.append_item(&item("Renommer l'onglet", "win.renommer-onglet", None));
    onglet.append_item(&item(
        "Détacher dans une nouvelle fenêtre",
        "win.detacher-onglet",
        None,
    ));
    onglet.append_item(&item(
        "Fermer l'onglet",
        "win.fermer-onglet",
        Some("<Control><Shift>w"),
    ));
    m.append_section(None, &onglet);
    let b = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .tooltip_text("Menu (F10)")
        .menu_model(&m)
        .primary(true)
        .build();
    b.update_property(&[gtk::accessible::Property::Label("Menu")]);
    b
}

/// Dialogue modal « question ? » avec Annuler (par défaut, et Échap) et Fermer.
fn confirmer(parent: &impl IsA<gtk::Window>, question: &str, si_oui: impl Fn() + 'static) {
    let dialogue = gtk::Window::builder()
        .title("Fermer ?")
        .modal(true)
        .transient_for(parent)
        .destroy_with_parent(true)
        .resizable(false)
        .accessible_role(gtk::AccessibleRole::Dialog)
        .build();
    let texte = gtk::Label::builder()
        .label(question)
        .wrap(true)
        .max_width_chars(48)
        .build();
    let annuler = gtk::Button::with_label("Annuler");
    let fermer = gtk::Button::with_label("Fermer");
    fermer.add_css_class("destructive-action");
    let boutons = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .halign(gtk::Align::End)
        .build();
    boutons.append(&annuler);
    boutons.append(&fermer);
    let boite = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .margin_start(18)
        .margin_end(18)
        .margin_top(18)
        .margin_bottom(18)
        .build();
    boite.append(&texte);
    boite.append(&boutons);
    dialogue.set_child(Some(&boite));
    dialogue.update_relation(&[gtk::accessible::Relation::DescribedBy(
        &[texte.upcast_ref()],
    )]);
    dialogue.set_default_widget(Some(&annuler));

    let d = dialogue.clone();
    annuler.connect_clicked(move |_| d.destroy());
    let d = dialogue.clone();
    fermer.connect_clicked(move |_| {
        d.destroy();
        si_oui();
    });
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
    dialogue.present();
    annuler.grab_focus();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raccourcis_de_la_conception() {
        let cs = gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::SHIFT_MASK;
        assert_eq!(action_pour_touche(gdk::Key::T, cs), Some("nouvel-onglet"));
        assert_eq!(action_pour_touche(gdk::Key::w, cs), Some("fermer-onglet"));
        assert_eq!(
            action_pour_touche(gdk::Key::N, cs),
            Some("nouvelle-fenetre")
        );
        assert_eq!(
            action_pour_touche(gdk::Key::t, gdk::ModifierType::CONTROL_MASK),
            None
        );
        let avec_verr = cs | gdk::ModifierType::LOCK_MASK;
        assert_eq!(
            action_pour_touche(gdk::Key::T, avec_verr),
            Some("nouvel-onglet")
        );
        assert_eq!(
            action_pour_touche(gdk::Key::T, cs | gdk::ModifierType::ALT_MASK),
            None
        );
    }
}
