// SPDX-License-Identifier: GPL-3.0-or-later
//! Fenêtres et onglets. [`Pupitre`] tient la connexion au processus de fond et distribue ses
//! messages aux onglets ; chaque [`FenetrePupitre`] porte un carnet d'onglets.
//!
//! Reprise après plantage : au démarrage, les sessions déjà présentes dans le processus de
//! fond sont celles d'une interface disparue (l'application est unique par session de
//! bureau) ; elles sont rattachées et leur tampon de relecture est rejoué.

use crate::apparence::{suivre_preference_systeme, theme_systeme, Preference};
use crate::client::{connect_or_spawn_at, Client, CONNEXION_PERDUE};
use crate::commandes::{doit_annoncer, texte_annonce, Sens};
use crate::demande::{ouvrir_demande, DemandeAffichee};
use crate::messages::{
    annoncer_focus_une_fois_affiche, refaire_annonce_focus, ActionMessage, BandeauMessages,
};
use crate::raccourcis::{raccourci, Raccourci};
use crate::rechargement::{surveiller, surveiller_fichiers, Surveillance};
use crate::recherche::BarreRecherche;
use crate::shells::{self, Choix};
use crate::taille::{largeur_ecran, taille_ouverture};
use crate::terminal::{animations_actives, TerminalOnglet};
use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use pupitre_config::{
    charger_theme, check_theme, theme_clair, theme_sombre, Config, ErreurConfig, Theme, ThemeChoix,
};
use pupitre_i18n::{t as tr, ta, ArgI18n};
use pupitre_proto::{ClientMsg, DemandeId, Genre, MarqueCommande, ServerMsg, SessionId};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::Duration;
use vte4::prelude::*;

/// Grille annoncée pour une session tant que la vraie n'est pas connue.
const GRILLE_PAR_DEFAUT: (u16, u16) = (80, 24);
/// Délai avant de provoquer un redessin après la relecture (le temps d'une allocation).
const DELAI_REDESSIN: Duration = Duration::from_millis(150);
const GROUPE_ONGLETS: &str = "pupitre-onglets";
/// Marque d'un onglet où la cloche a sonné (doublée de « , cloche » pour le lecteur d'écran).
const MARQUE_CLOCHE: &str = "● ";
/// Délai avant de refaire l'annonce du lancement (voir `reannoncer_au_lancement`).
const DELAI_REANNONCE: Duration = Duration::from_millis(500);

/// Clés des messages persistants (un seul message par clé).
const CLE_PANNE: &str = "panne";
const CLE_CONFIG: &str = "config";
const CLE_THEME: &str = "theme";
const CLE_CONTRASTE: &str = "contraste";
const CLE_POLICE: &str = "police";
const CLE_SURVEILLANCE: &str = "surveillance";
const CLE_FOND_ANCIEN: &str = "fond-ancien";
const CLE_CONNEXIONS: &str = "connexions-invalides";
/// Police embarquée, utilisée si celle demandée est introuvable.
const POLICE_DE_SECOURS: &str = "B612 Mono";
const TAILLE_DE_SECOURS: i32 = 11;

/// Un onglet : un terminal et son étiquette (titre ou nom choisi, modifiable sur place).
pub struct Onglet {
    terminal: TerminalOnglet,
    /// Tête d'onglet : l'étiquette, ou la saisie du nom pendant un renommage. Une boîte
    /// et non une GtkStack : GTK 4.18 publie l'enfant caché d'une GtkStack avec une
    /// taille aberrante, et Orca boucle sans fin dessus (revue à plat).
    tete: gtk::Box,
    etiquette: gtk::Label,
    entree: gtk::Entry,
    nom: RefCell<Option<String>>,
    /// Titre en attendant que le programme en donne un (nom d'une connexion SSH) ; ce
    /// n'est pas un nom choisi : il cède la place au titre du programme.
    provisoire: RefCell<Option<String>>,
    occupe: Cell<bool>,
    termine: Cell<bool>,
    /// Rattaché après une reprise : la sortie est ignorée jusqu'à la relecture.
    attend_relecture: Cell<bool>,
    /// Indicateur ● (cloche dans un onglet non visible).
    signale: Cell<bool>,
    /// Dernières annonces de commandes faites au lecteur d'écran (les plus anciennes sont
    /// oubliées).
    annonces: RefCell<VecDeque<String>>,
}

/// Annonces de commandes gardées par onglet (observées par les tests).
const ANNONCES_GARDEES: usize = 32;

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
        if self.terminal.titre_programme().is_none() {
            if let Some(p) = self.provisoire.borrow().as_ref() {
                return p.clone();
            }
        }
        self.terminal
            .titre()
            .unwrap_or_else(|| tr("onglet-titre-defaut"))
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

    /// Un événement attend l'attention de l'utilisateur dans cet onglet (●).
    pub fn signale(&self) -> bool {
        self.signale.get()
    }

    /// Texte de l'étiquette de l'onglet, indicateur compris.
    pub fn titre_affiche(&self) -> String {
        self.etiquette.text().to_string()
    }

    /// Dernières annonces de commandes (fin d'une commande, navigation), de la plus
    /// ancienne à la plus récente.
    pub fn annonces(&self) -> Vec<String> {
        self.annonces.borrow().iter().cloned().collect()
    }

    /// Nombre de commandes repérées dans la sortie du shell de cet onglet.
    pub fn nombre_commandes(&self) -> usize {
        self.terminal.nombre_commandes()
    }

    /// Annonce `texte` au lecteur d'écran, poliment, et le garde dans le journal.
    fn annoncer_commande(&self, texte: &str) {
        {
            let mut a = self.annonces.borrow_mut();
            if a.len() >= ANNONCES_GARDEES {
                a.pop_front();
            }
            a.push_back(texte.to_string());
        }
        crate::messages::annoncer(self.terminal.terminal(), texte);
    }

    /// L'onglet est affiché dans une fenêtre active (les autres : à l'étape 4).
    fn visible_et_actif(&self) -> bool {
        let Some(c) = self.carnet() else { return false };
        c.page_num(&self.widget()) == c.current_page()
            && c.root()
                .and_downcast::<gtk::Window>()
                .is_some_and(|w| crate::commandes::fenetre_active(w.is_active()))
    }

    /// Défile jusqu'à la commande précédente ou suivante et l'annonce.
    pub fn naviguer_commande(self: &Rc<Self>, sens: Sens) {
        let faible = Rc::downgrade(self);
        self.terminal.naviguer(sens, move |n| {
            if let (Some(o), Some(n)) = (faible.upgrade(), n) {
                o.annoncer_commande(&n.texte());
            }
        });
    }

    fn signaler(&self, oui: bool) {
        if self.signale.replace(oui) != oui {
            self.rafraichir_titre();
        }
    }

    fn rafraichir_titre(&self) {
        let titre = self.titre();
        if self.signale.get() {
            // Le ● est doublé d'un texte pour le lecteur d'écran.
            self.etiquette.set_text(&format!("{MARQUE_CLOCHE}{titre}"));
            self.etiquette.set_tooltip_text(Some(&ta(
                "onglet-infobulle-cloche",
                &[("titre", ArgI18n::from(titre.as_str()))],
            )));
            self.etiquette
                .update_property(&[gtk::accessible::Property::Label(&nom_avec_cloche(&titre))]);
        } else {
            self.etiquette.set_text(&titre);
            self.etiquette.set_tooltip_text(Some(&titre));
            self.etiquette
                .reset_property(gtk::AccessibleProperty::Label);
        }
        // Le nom accessible de l'onglet suit son étiquette ; le terminal porte le même nom.
        self.terminal
            .terminal()
            .update_property(&[gtk::accessible::Property::Label(&titre)]);
        if let Some(c) = self.carnet() {
            c.page(&self.widget()).set_property("menu-label", &titre);
            decrire_onglets(&c);
            if c.page_num(&self.widget()) == c.current_page() {
                if let Some(f) = c.root().and_downcast::<gtk::Window>() {
                    f.set_title(Some(&titre_fenetre(&titre)));
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
        self.etiquette.set_visible(false);
        self.entree.set_visible(true);
        self.entree.grab_focus();
        self.entree.select_region(0, -1);
    }

    fn finir_renommage(&self, valider: bool) {
        if !WidgetExt::is_visible(&self.entree) {
            return;
        }
        if valider {
            let texte = self.entree.text().to_string();
            // Valider le titre automatique tel quel ne fige pas de nom.
            if texte != self.titre() {
                self.renommer(Some(&texte));
            }
        }
        self.entree.set_visible(false);
        self.etiquette.set_visible(true);
        self.terminal.widget().grab_focus();
    }

    fn marquer_termine(&self, code: Option<i32>) {
        self.termine.set(true);
        self.occupe.set(false);
        self.terminal.rendre_muet(true);
        let message = match code {
            Some(c) => ta("shell-termine", &[("code", ArgI18n::from(c.to_string()))]),
            None => tr("shell-termine-signal"),
        };
        self.terminal
            .on_output(format!("\r\n\x1b[0m{message}\r\n").as_bytes());
        crate::messages::annoncer(self.terminal.terminal(), &message);
    }
}

enum Attente {
    /// `Open` envoyé pour un nouvel onglet de cette fenêtre.
    NouvelOnglet(Weak<FenetreInterne>, Option<String>),
    /// `Open` envoyé pour remplacer le shell terminé de cet onglet.
    Remplacer(Weak<Onglet>),
}

/// L'application : connexion au processus de fond, fenêtres, onglets, thème.
pub struct Pupitre {
    app: gtk::Application,
    socket: PathBuf,
    /// Configuration lue (dernière valide).
    config: RefCell<Config>,
    /// Configuration appliquée (police de secours éventuelle).
    config_effective: RefCell<Config>,
    theme: RefCell<Theme>,
    preference: Cell<Preference>,
    /// Messages persistants (clé, texte) montrés dans toutes les fenêtres.
    messages: RefCell<Vec<(String, String)>>,
    surveillance_config: RefCell<Option<Surveillance>>,
    surveillance_theme: RefCell<Option<(PathBuf, Surveillance)>>,
    client: RefCell<Option<Rc<Client>>>,
    /// Numéro de la connexion en cours : les messages d'une connexion remplacée sont ignorés.
    generation: Cell<u64>,
    attend_liste: Cell<bool>,
    attentes: RefCell<VecDeque<Attente>>,
    onglets: RefCell<Vec<Rc<Onglet>>>,
    fenetres: RefCell<Vec<FenetrePupitre>>,
    /// Fenêtres des demandes SSH en attente de réponse.
    demandes: RefCell<HashMap<DemandeId, gtk::Window>>,
    /// Demandes « Information » affichées dans un bandeau (retirées à la perte du fond).
    informations: RefCell<HashSet<DemandeId>>,
    /// Demandes arrivées avant la première fenêtre (celle de `Sessions`) : ouvertes dès
    /// qu'elle existe, au lieu de créer une fenêtre vide.
    demandes_sans_fenetre: RefCell<Vec<(DemandeId, Option<SessionId>, DemandeAffichee)>>,
    /// Messages du fond arrivés sans aucune fenêtre : montrés par la première créée.
    messages_sans_fenetre: RefCell<Vec<String>>,
    /// Garde l'application en vie tant que la première fenêtre n'existe pas (GTK quitte
    /// une application sans fenêtre).
    garde: RefCell<Option<gio::ApplicationHoldGuard>>,
    /// Avertissements de lancement déjà montrés : un seul par lancement de Pupitre.
    avertis: RefCell<HashSet<String>>,
}

impl Pupitre {
    /// Se connecte au processus de fond (en le lançant au besoin), puis ouvre la première
    /// fenêtre : avec les sessions orphelines s'il y en a (reprise), sinon avec un shell neuf.
    pub fn demarrer(app: &gtk::Application, socket: PathBuf, config: Config) -> Rc<Pupitre> {
        let p = Rc::new(Pupitre {
            app: app.clone(),
            socket,
            config: RefCell::new(config.clone()),
            config_effective: RefCell::new(config.clone()),
            theme: RefCell::new(theme_sombre()),
            preference: Cell::new(Preference::SansPreference),
            messages: RefCell::new(Vec::new()),
            surveillance_config: RefCell::new(None),
            surveillance_theme: RefCell::new(None),
            client: RefCell::new(None),
            generation: Cell::new(0),
            attend_liste: Cell::new(false),
            attentes: RefCell::new(VecDeque::new()),
            onglets: RefCell::new(Vec::new()),
            fenetres: RefCell::new(Vec::new()),
            demandes: RefCell::new(HashMap::new()),
            informations: RefCell::new(HashSet::new()),
            demandes_sans_fenetre: RefCell::new(Vec::new()),
            messages_sans_fenetre: RefCell::new(Vec::new()),
            garde: RefCell::new(Some(app.hold())),
            avertis: RefCell::new(HashSet::new()),
        });
        p.appliquer_config(Ok(config));
        // Fenêtre fermée (quelle qu'en soit la raison) : retirée de la liste. Le signal
        // `destroy` ne convient pas : il n'arrive qu'à la dernière référence relâchée.
        let faible = Rc::downgrade(&p);
        app.connect_window_removed(move |_, w| {
            // Les dialogues sont aussi rattachés à l'application : seules les fenêtres
            // principales sont suivies (aucun emprunt pendant la fermeture d'un dialogue).
            if !w.is::<gtk::ApplicationWindow>() {
                return;
            }
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
        let faible = Rc::downgrade(&p);
        suivre_preference_systeme(move |pref| {
            if let Some(p) = faible.upgrade() {
                p.preference.set(pref);
                if p.config.borrow().theme == ThemeChoix::Systeme {
                    p.changer_theme(theme_systeme(pref));
                }
            }
        });
        match p.connecter() {
            Ok(()) => {
                p.attend_liste.set(true);
                p.envoyer(ClientMsg::List);
            }
            Err(e) => {
                let f = FenetrePupitre::nouvelle(app, &p);
                f.signaler_panne(&ta(
                    "fond-injoignable",
                    &[("erreur", ArgI18n::from(e.to_string()))],
                ));
                f.fenetre().present();
            }
        }
        p
    }

    /// Applique une configuration relue : valide → appliquée à tous les onglets ;
    /// invalide → la précédente est gardée et un message persistant donne la ligne.
    pub fn appliquer_config(self: &Rc<Self>, r: Result<Config, ErreurConfig>) {
        match r {
            Ok(c) => {
                self.poser_message(CLE_CONFIG, None);
                *self.config.borrow_mut() = c;
            }
            Err(e) => {
                self.poser_message(
                    CLE_CONFIG,
                    Some(ta(
                        "config-invalide",
                        &[("erreur", ArgI18n::from(e.to_string()))],
                    )),
                );
                return;
            }
        }
        self.reappliquer();
    }

    /// Relit la configuration `chemin` à chaque enregistrement (et son thème s'il est
    /// dans un fichier).
    pub fn surveiller_config(self: &Rc<Self>, chemin: PathBuf) {
        let faible = Rc::downgrade(self);
        match surveiller(&chemin, move |r| {
            if let Some(p) = faible.upgrade() {
                p.appliquer_config(r);
            }
        }) {
            Ok(s) => {
                *self.surveillance_config.borrow_mut() = Some(s);
                self.poser_message(CLE_SURVEILLANCE, None);
            }
            Err(e) => self.poser_message(
                CLE_SURVEILLANCE,
                Some(ta(
                    "config-rechargement-indisponible",
                    &[("erreur", ArgI18n::from(e))],
                )),
            ),
        }
    }

    /// Résout thème et police de la configuration courante, puis applique.
    fn reappliquer(self: &Rc<Self>) {
        let config = self.config.borrow().clone();
        let theme = match &config.theme {
            ThemeChoix::Sombre => Some(theme_sombre()),
            ThemeChoix::Clair => Some(theme_clair()),
            ThemeChoix::Systeme => Some(theme_systeme(self.preference.get())),
            ThemeChoix::Fichier(chemin) => match charger_theme(chemin) {
                Ok(t) => {
                    self.poser_message(CLE_THEME, None);
                    Some(t)
                }
                Err(e) => {
                    self.poser_message(
                        CLE_THEME,
                        Some(ta(
                            "theme-illisible-conserve",
                            &[
                                ("chemin", ArgI18n::from(chemin.display().to_string())),
                                ("erreur", ArgI18n::from(e.to_string())),
                            ],
                        )),
                    );
                    None
                }
            },
        };
        if !matches!(config.theme, ThemeChoix::Fichier(_)) {
            self.poser_message(CLE_THEME, None);
        }
        self.suivre_fichier_de_theme(&config.theme);

        let mut effective = config.clone();
        match police_de_secours(&config.police) {
            Some((famille, remplacement)) => {
                self.poser_message(
                    CLE_POLICE,
                    Some(ta(
                        "police-introuvable",
                        &[
                            ("famille", ArgI18n::from(famille.as_str())),
                            ("secours", ArgI18n::from(POLICE_DE_SECOURS)),
                        ],
                    )),
                );
                effective.police = remplacement;
            }
            None => self.poser_message(CLE_POLICE, None),
        }
        *self.config_effective.borrow_mut() = effective;

        let theme = theme.unwrap_or_else(|| self.theme.borrow().clone());
        let problemes = check_theme(&theme);
        self.poser_message(
            CLE_CONTRASTE,
            (!problemes.is_empty()).then(|| {
                let liste: Vec<String> = problemes.iter().map(|p| p.to_string()).collect();
                ta(
                    "theme-contraste-insuffisant",
                    &[
                        ("nom", ArgI18n::from(theme.nom.as_str())),
                        ("liste", ArgI18n::from(liste.join(" ; "))),
                    ],
                )
            }),
        );
        self.changer_theme(theme);
        for f in self.fenetres() {
            f.rafraichir_shells();
        }
    }

    /// Surveille le fichier de thème choisi (et lui seul).
    fn suivre_fichier_de_theme(self: &Rc<Self>, choix: &ThemeChoix) {
        let ThemeChoix::Fichier(chemin) = choix else {
            *self.surveillance_theme.borrow_mut() = None;
            return;
        };
        if self
            .surveillance_theme
            .borrow()
            .as_ref()
            .is_some_and(|(c, _)| c == chemin)
        {
            return;
        }
        let faible = Rc::downgrade(self);
        match surveiller_fichiers(std::slice::from_ref(chemin), move || {
            if let Some(p) = faible.upgrade() {
                p.reappliquer();
            }
        }) {
            Ok(s) => *self.surveillance_theme.borrow_mut() = Some((chemin.clone(), s)),
            Err(e) => {
                *self.surveillance_theme.borrow_mut() = None;
                eprintln!("pupitre: {e}");
            }
        }
    }

    /// Pose (ou retire) un message persistant dans toutes les fenêtres, présentes et à venir.
    fn poser_message(&self, cle: &str, texte: Option<String>) {
        {
            let mut m = self.messages.borrow_mut();
            m.retain(|(c, _)| c != cle);
            if let Some(t) = &texte {
                m.push((cle.to_string(), t.clone()));
            }
        }
        for f in self.fenetres() {
            f.0.bandeau.poser(cle, texte.as_deref());
        }
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
            f.signaler_panne(&tr("fond-arrete"));
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
        self.ouvrir_session(&Choix::Defaut, cols, rows);
    }

    fn connecter(self: &Rc<Self>) -> Result<(), String> {
        let (client, recus) = connect_or_spawn_at(&self.socket).map_err(|e| e.to_string())?;
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        // Même clé à chaque connexion : pas de doublon à la reconnexion.
        self.poser_message(
            CLE_FOND_ANCIEN,
            (client.version() < 2).then(|| tr("fond-ancien")),
        );
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

    /// Demande l'ouverture d'une session pour `choix` : shell, intégration et variables
    /// viennent de [`shells::preparer`]. Un avertissement (shell ou Starship introuvable…) est
    /// montré une seule fois par lancement de Pupitre, dans le bandeau de messages.
    fn ouvrir_session(&self, choix: &Choix, cols: u16, rows: u16) {
        if self.client.borrow().is_none() {
            return;
        }
        let lancement = shells::preparer(choix, &self.config.borrow(), &shells::path_courant());
        if let Some(a) = lancement.avertissement {
            if self.avertis.borrow_mut().insert(a.clone()) {
                self.poser_message(&format!("lancement:{a}"), Some(a));
            }
        }
        if let Some(c) = self.client.borrow().as_ref() {
            // Échec = connexion perdue, signalée par ailleurs sur le canal.
            let _ = c.ouvrir(Some(lancement.argv), None, cols, rows, lancement.env);
        }
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
        let config = self.config_effective.borrow();
        for o in self.onglets.borrow().iter() {
            o.terminal.appliquer(&config, &theme);
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
                // Demandes reçues avant cette première fenêtre : elles y sont ouvertes.
                let en_attente = std::mem::take(&mut *self.demandes_sans_fenetre.borrow_mut());
                for (demande, session, d) in en_attente {
                    self.ouvrir_demande(demande, session, d);
                }
            }
            ServerMsg::Opened { id } => {
                let attente = self.attentes.borrow_mut().pop_front();
                match attente {
                    Some(Attente::NouvelOnglet(f, titre)) => match f.upgrade() {
                        Some(f) if !f.fermee.get() => {
                            if let Some(o) = FenetrePupitre(f).ajouter_onglet(id) {
                                *o.provisoire.borrow_mut() = titre;
                                o.rafraichir_titre();
                            }
                        }
                        _ => self.envoyer(ClientMsg::Close { id }),
                    },
                    Some(Attente::Remplacer(o)) => match o.upgrade() {
                        Some(o) if o.carnet().is_some() => {
                            o.terminal.relier(id);
                            o.termine.set(false);
                            o.occupe.set(false);
                            // Le shell local n'est plus le serveur dont l'onglet portait le nom.
                            *o.provisoire.borrow_mut() = None;
                            o.rafraichir_titre();
                            // La grille a pu changer depuis la demande.
                            o.terminal.forcer_redessin();
                        }
                        _ => self.envoyer(ClientMsg::Close { id }),
                    },
                    None => self.envoyer(ClientMsg::Close { id }),
                }
            }
            ServerMsg::OpenFailed { error } => {
                let message = ta(
                    "shell-lancement-impossible",
                    &[("erreur", ArgI18n::from(error))],
                );
                let attente = self.attentes.borrow_mut().pop_front();
                match attente {
                    Some(Attente::NouvelOnglet(f, _)) => {
                        if let Some(f) = f.upgrade() {
                            FenetrePupitre(f).afficher_message(&message);
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
            ServerMsg::Replay { id, data } => self.rejouer(id, &data, &[]),
            ServerMsg::ReplayMarque { id, data, marques } => self.rejouer(id, &data, &marques),
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
            ServerMsg::Demande {
                demande,
                session,
                invite,
                genre,
                trousseau,
            } => self.ouvrir_demande(
                demande,
                session,
                DemandeAffichee {
                    invite,
                    genre,
                    trousseau,
                },
            ),
            ServerMsg::DemandeClose { demande } => self.fermer_demande(demande),
            // Marque de commande en direct (OSC 133), à sa place dans le flux des octets ;
            // pendant la relecture d'une reprise, la sortie est ignorée et les marques aussi.
            ServerMsg::Marque {
                id,
                marque,
                rejouee,
            } => {
                if let Some(o) = self.onglet(id).filter(|o| !o.attend_relecture.get()) {
                    o.terminal.marque(marque, rejouee);
                }
            }
            ServerMsg::Message(t) => {
                let fenetres = self.fenetres();
                if fenetres.is_empty() {
                    // Pas encore de fenêtre (le message précède `Sessions`) : gardé pour la
                    // première, qui le montrera.
                    self.messages_sans_fenetre.borrow_mut().push(t);
                    return;
                }
                for f in fenetres {
                    f.afficher_message(&t);
                }
            }
            ServerMsg::Welcome { .. } | ServerMsg::Sessions(_) | ServerMsg::Reponse { .. } => {}
        }
    }

    /// Reprise d'une session : octets (avec leurs marques de commandes) à afficher d'un coup.
    fn rejouer(&self, id: SessionId, data: &[u8], marques: &[(u32, MarqueCommande)]) {
        if let Some(o) = self.onglet(id) {
            o.attend_relecture.set(false);
            o.terminal.rejouer(data, marques);
            let faible = Rc::downgrade(&o);
            glib::timeout_add_local_once(DELAI_REDESSIN, move || {
                if let Some(o) = faible.upgrade() {
                    o.terminal.forcer_redessin();
                }
            });
        }
    }

    fn cle_information(demande: DemandeId) -> String {
        format!("demande-{demande}")
    }

    /// Fenêtre qui doit porter la demande de `session` : celle de son onglet, sinon la
    /// fenêtre active, sinon la première ; aucune fenêtre : une nouvelle.
    fn fenetre_de_demande(self: &Rc<Self>, session: Option<SessionId>) -> FenetrePupitre {
        let de_session = session.and_then(|s| self.onglet(s)).and_then(|o| {
            let carnet = o.carnet()?;
            self.fenetres().into_iter().find(|f| f.0.carnet == carnet)
        });
        de_session
            .or_else(|| {
                self.fenetres()
                    .into_iter()
                    .find(|f| f.0.fenetre.is_active())
            })
            .or_else(|| self.fenetres().into_iter().next())
            .unwrap_or_else(|| {
                let f = FenetrePupitre::nouvelle(&self.app, self);
                f.fenetre().present();
                f
            })
    }

    /// Montre la demande `demande`. Le fond renvoie les demandes ouvertes à chaque
    /// interface qui arrive, au `Hello` puis au `List` : une demande déjà affichée (ou déjà
    /// en attente de fenêtre) est ignorée, ce qui garde la fenêtre et le texte saisi.
    fn ouvrir_demande(
        self: &Rc<Self>,
        demande: DemandeId,
        session: Option<SessionId>,
        d: DemandeAffichee,
    ) {
        if self.demandes.borrow().contains_key(&demande)
            || self.informations.borrow().contains(&demande)
        {
            return;
        }
        if self.fenetres.borrow().is_empty() && self.attend_liste.get() {
            // La première fenêtre naîtra de `Sessions` : on l'attend.
            let mut file = self.demandes_sans_fenetre.borrow_mut();
            if !file.iter().any(|(id, _, _)| *id == demande) {
                file.push((demande, session, d));
            }
            return;
        }
        let f = self.fenetre_de_demande(session);
        if d.genre == Genre::Information {
            self.informations.borrow_mut().insert(demande);
            f.0.bandeau
                .poser(&Self::cle_information(demande), Some(&d.invite));
            return;
        }
        let faible = Rc::downgrade(self);
        let fenetre = ouvrir_demande(f.fenetre().upcast_ref(), &d, move |reponse, memoriser| {
            let Some(p) = faible.upgrade() else { return };
            // Demande déjà réglée côté interface (`DemandeClose`) : pas de réponse.
            if p.demandes.borrow_mut().remove(&demande).is_none() {
                return;
            }
            let v2 = p.client.borrow().as_ref().is_some_and(|c| c.version() >= 2);
            if v2 {
                p.envoyer(ClientMsg::Repondre {
                    demande,
                    reponse,
                    memoriser,
                });
            }
        });
        self.demandes.borrow_mut().insert(demande, fenetre);
    }

    /// La demande est réglée ailleurs : fenêtre fermée (ou message retiré), sans réponse.
    fn fermer_demande(&self, demande: DemandeId) {
        self.demandes_sans_fenetre
            .borrow_mut()
            .retain(|(id, _, _)| *id != demande);
        self.informations.borrow_mut().remove(&demande);
        // Sortie du `RefCell` avant `destroy()` : sa destruction rappelle `ouvrir_demande`
        // (fermeture sans réponse), qui emprunte à nouveau `demandes`.
        let fenetre = self.demandes.borrow_mut().remove(&demande);
        if let Some(w) = fenetre {
            w.destroy();
        }
        let cle = Self::cle_information(demande);
        for f in self.fenetres() {
            f.0.bandeau.poser(&cle, None);
        }
    }

    fn connexion_perdue(self: &Rc<Self>) {
        *self.client.borrow_mut() = None;
        self.generation.set(self.generation.get() + 1);
        self.attend_liste.set(false);
        self.attentes.borrow_mut().clear();
        // Plus personne à qui répondre.
        let demandes: Vec<gtk::Window> =
            self.demandes.borrow_mut().drain().map(|(_, w)| w).collect();
        for w in demandes {
            w.destroy();
        }
        self.demandes_sans_fenetre.borrow_mut().clear();
        let informations: Vec<DemandeId> = self.informations.borrow_mut().drain().collect();
        for demande in informations {
            let cle = Self::cle_information(demande);
            for f in self.fenetres() {
                f.0.bandeau.poser(&cle, None);
            }
        }
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
            f.signaler_panne(&tr("fond-arrete"));
        }
    }

    /// Relance le processus de fond et ouvre un terminal neuf dans `fenetre` ; les onglets
    /// des shells perdus sont fermés (les fenêtres vidées se ferment).
    fn reconnecter(self: &Rc<Self>, fenetre: &FenetrePupitre) {
        if let Err(e) = self.connecter() {
            fenetre.signaler_panne(&ta(
                "fond-relance-impossible",
                &[("erreur", ArgI18n::from(e.to_string()))],
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
            f.masquer_panne();
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

pub struct FenetreInterne {
    fenetre: gtk::ApplicationWindow,
    carnet: gtk::Notebook,
    bandeau: Rc<BandeauMessages>,
    recherche: Rc<BarreRecherche>,
    menu: gtk::MenuButton,
    /// Sous-menu « Nouvel onglet avec » (refait quand les réglages changent).
    sous_menu_shells: gio::Menu,
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
            .title(tr("fenetre-nom"))
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
        // Ctrl+Tab met le focus sur le carnet lui-même : sans nom, Orca dirait seulement
        // « groupement ». Sa description donne l'onglet courant (voir `decrire_onglets`).
        carnet.update_property(&[gtk::accessible::Property::Label(&tr("onglets-nom"))]);
        let (menu, sous_menu_shells) = bouton_menu();
        carnet.set_action_widget(&menu, gtk::PackType::End);

        let bandeau = BandeauMessages::new();

        let boite = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let recherche = BarreRecherche::new();
        boite.append(bandeau.widget());
        boite.append(recherche.widget());
        boite.append(&carnet);
        fenetre.set_child(Some(&boite));

        let f = FenetrePupitre(Rc::new(FenetreInterne {
            fenetre,
            carnet,
            bandeau,
            recherche,
            menu,
            sous_menu_shells,
            garder_ouverte: Cell::new(false),
            fermee: Cell::new(false),
            pupitre: Rc::downgrade(pupitre),
        }));
        for (cle, texte) in pupitre.messages.borrow().iter() {
            f.0.bandeau.poser(cle, Some(texte));
        }
        let gardes = std::mem::take(&mut *pupitre.messages_sans_fenetre.borrow_mut());
        for texte in gardes {
            f.afficher_message(&texte);
        }
        f.installer_actions();
        f.rafraichir_shells();
        crate::raccourcis::installer_raccourcis(&f);
        f.installer_signaux();
        f.reannoncer_au_lancement();
        pupitre.fenetres.borrow_mut().push(f.clone());
        pupitre.garde.borrow_mut().take();
        f
    }

    pub fn fenetre(&self) -> &gtk::ApplicationWindow {
        &self.0.fenetre
    }

    /// GTK 4.18 signale l'activation de la première fenêtre avant d'avoir fini de
    /// s'inscrire sur le bus d'accessibilité : le lecteur d'écran ne trouve pas la fenêtre
    /// et n'annonce rien au lancement. Peu après la première activation, le focus est
    /// redonné à l'élément qui l'a, ce qui refait l'annonce (fenêtre, terminal, invite).
    fn reannoncer_au_lancement(&self) {
        thread_local! {
            static FAIT: Cell<bool> = const { Cell::new(false) };
        }
        let faible = Rc::downgrade(&self.0);
        self.0.fenetre.connect_is_active_notify(move |fenetre| {
            if !fenetre.is_active() || FAIT.with(|f| f.replace(true)) {
                return;
            }
            let faible = faible.clone();
            glib::timeout_add_local_once(DELAI_REANNONCE, move || {
                let Some(f) = faible.upgrade() else { return };
                if !f.fenetre.is_active() {
                    return;
                }
                refaire_annonce_focus(&f.fenetre);
            });
        });
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
        let n = c.append_page(&o.widget(), Some(&o.tete));
        c.set_tab_reorderable(&o.widget(), true);
        c.set_tab_detachable(&o.widget(), true);
        c.set_current_page(Some(n));
        self.0.garder_ouverte.set(false);
        o.rafraichir_titre();
        o.terminal.widget().grab_focus();
        annoncer_focus_une_fois_affiche(o.terminal.terminal().upcast_ref());
    }

    /// Ouvre un nouveau shell dans un nouvel onglet de cette fenêtre.
    pub fn ouvrir_session(&self) {
        self.ouvrir(&Choix::Defaut, None);
    }

    /// Ouvre un nouvel onglet pour une entrée du sous-menu « Nouvel onglet avec » (valeur
    /// d'action rendue par [`entrees_nouvel_onglet_avec`](Self::entrees_nouvel_onglet_avec)).
    fn ouvrir_avec(&self, cible: &str) {
        let Some(p) = self.pupitre() else { return };
        let choix = shells::choix_de_cible(cible, &p.config.borrow());
        if let Some(c) = choix {
            self.ouvrir(&c, None);
        }
    }

    /// Entrées du sous-menu « Nouvel onglet avec » : `(libellé, valeur de l'action)`.
    pub fn entrees_nouvel_onglet_avec(&self) -> Vec<(String, String)> {
        let m = &self.0.sous_menu_shells;
        (0..m.n_items())
            .filter_map(|i| {
                let texte = |attr: &str| {
                    m.item_attribute_value(i, attr, Some(glib::VariantTy::STRING))
                        .and_then(|v| v.get::<String>())
                };
                Some((texte("label")?, texte("target")?))
            })
            .collect()
    }

    /// Refait le sous-menu « Nouvel onglet avec » d'après les shells installés et les réglages.
    fn rafraichir_shells(&self) {
        let Some(p) = self.pupitre() else { return };
        let entrees = shells::entrees_menu(&p.config.borrow(), &shells::path_courant());
        let m = &self.0.sous_menu_shells;
        m.remove_all();
        for (libelle, cible) in entrees {
            let i = gio::MenuItem::new(Some(&libelle), None);
            i.set_action_and_target_value(
                Some("win.nouvel-onglet-avec"),
                Some(&cible.to_variant()),
            );
            m.append_item(&i);
        }
    }

    /// Ouvre `argv` (une commande `ssh …`) dans un nouvel onglet de cette fenêtre. `titre`
    /// reste affiché tant que le programme n'en donne pas.
    pub fn ouvrir_connexion(&self, argv: Vec<String>, titre: String) {
        self.ouvrir(&Choix::Commande(argv), Some(titre));
    }

    fn ouvrir(&self, choix: &Choix, titre: Option<String>) {
        let Some(p) = self.pupitre() else { return };
        let (cols, rows) = self
            .onglet_actif()
            .and_then(|o| o.terminal.grille())
            .unwrap_or(GRILLE_PAR_DEFAUT);
        p.attentes
            .borrow_mut()
            .push_back(Attente::NouvelOnglet(Rc::downgrade(&self.0), titre));
        p.ouvrir_session(choix, cols, rows);
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

    /// Premier message affiché dans la fenêtre, s'il y en a un.
    pub fn message(&self) -> Option<String> {
        self.0.bandeau.textes().into_iter().next()
    }

    /// Messages affichés dans la fenêtre.
    pub fn messages(&self) -> Vec<String> {
        self.0.bandeau.textes()
    }

    /// Équivaut à un clic sur « Ouvrir un nouveau terminal » du message de panne.
    pub fn activer_message(&self) {
        self.0.bandeau.activer(CLE_PANNE);
    }

    /// Pose (ou retire) le message « connexions.toml invalide », dans toutes les fenêtres.
    pub fn poser_message_connexions(&self, texte: Option<String>) {
        if let Some(p) = self.pupitre() {
            p.poser_message(CLE_CONNEXIONS, texte);
        }
    }

    /// Affiche un message persistant (lu par le lecteur d'écran).
    pub fn afficher_message(&self, texte: &str) {
        self.0.bandeau.afficher(texte, None);
    }

    fn masquer_panne(&self) {
        self.0.bandeau.poser(CLE_PANNE, None);
    }

    fn signaler_panne(&self, texte: &str) {
        let faible = Rc::downgrade(&self.0);
        let action: ActionMessage = Rc::new(move || {
            if let Some(f) = faible.upgrade() {
                let f = FenetrePupitre(f);
                if let Some(p) = f.pupitre() {
                    p.reconnecter(&f);
                }
            }
        });
        self.0.bandeau.poser_avec_action(
            CLE_PANNE,
            Some(texte),
            Some((&tr("fond-nouveau-terminal"), action)),
        );
        self.0.bandeau.donner_focus(CLE_PANNE);
    }

    /// Référence faible (pour les rappels attachés aux widgets de la fenêtre elle-même).
    pub fn faible(&self) -> Weak<FenetreInterne> {
        Rc::downgrade(&self.0)
    }

    pub fn depuis_faible(f: &Weak<FenetreInterne>) -> Option<FenetrePupitre> {
        f.upgrade().map(FenetrePupitre)
    }

    /// Exécute le raccourci de la touche, s'il y en a un ; vrai si la touche est consommée.
    pub fn traiter_touche(&self, touche: gdk::Key, code: u32, mods: gdk::ModifierType) -> bool {
        match raccourci(touche, code, mods) {
            Some(r) => {
                self.executer(r);
                true
            }
            None => false,
        }
    }

    /// Exécute un raccourci dans cette fenêtre (onglet actif pour zoom, copie, recherche).
    pub fn executer(&self, r: Raccourci) {
        let c = &self.0.carnet;
        let n = c.n_pages();
        let actif = self.onglet_actif();
        let zoom_max = self
            .pupitre()
            .map_or(200, |p| p.config_effective.borrow().zoom_max_pourcent);
        match r {
            Raccourci::NouvelOnglet => self.ouvrir_session(),
            Raccourci::NouvelleFenetre => {
                if let Some(p) = self.pupitre() {
                    p.nouvelle_fenetre();
                }
            }
            Raccourci::Connexions => {
                crate::connexions::ouvrir_connexions(self);
            }
            Raccourci::FermerOnglet => {
                if let Some(o) = actif {
                    self.demander_fermeture_onglet(&o);
                }
            }
            Raccourci::Copier => actif.iter().for_each(|o| o.terminal.copier()),
            Raccourci::Coller => actif.iter().for_each(|o| o.terminal.coller()),
            Raccourci::Rechercher => {
                if let Some(o) = actif {
                    self.0.recherche.ouvrir(o.terminal.terminal());
                }
            }
            Raccourci::OngletSuivant | Raccourci::OngletPrecedent if n > 1 => {
                let i = c.current_page().unwrap_or(0);
                let j = if r == Raccourci::OngletSuivant {
                    (i + 1) % n
                } else {
                    (i + n - 1) % n
                };
                c.set_current_page(Some(j));
            }
            Raccourci::OngletSuivant | Raccourci::OngletPrecedent => {}
            Raccourci::Onglet(k) => {
                if (1..=n).contains(&k) {
                    c.set_current_page(Some(k - 1));
                }
            }
            Raccourci::PleinEcran => {
                let f = &self.0.fenetre;
                if f.is_fullscreen() {
                    f.unfullscreen();
                } else {
                    f.fullscreen();
                }
            }
            Raccourci::ZoomPlus => actif
                .iter()
                .for_each(|o| o.terminal.zoomer(Some(1), zoom_max)),
            Raccourci::ZoomMoins => actif
                .iter()
                .for_each(|o| o.terminal.zoomer(Some(-1), zoom_max)),
            Raccourci::FocusSuivant => {
                self.deplacer_focus(gtk::DirectionType::TabForward);
            }
            Raccourci::FocusPrecedent => {
                self.deplacer_focus(gtk::DirectionType::TabBackward);
            }
            Raccourci::ZoomNormal => actif.iter().for_each(|o| o.terminal.zoomer(None, zoom_max)),
            Raccourci::CommandePrecedente => actif
                .iter()
                .for_each(|o| o.naviguer_commande(Sens::Precedente)),
            Raccourci::CommandeSuivante => actif
                .iter()
                .for_each(|o| o.naviguer_commande(Sens::Suivante)),
        }
    }

    pub fn recherche_ouverte(&self) -> bool {
        self.0.recherche.ouverte()
    }

    pub fn entree_recherche(&self) -> gtk::SearchEntry {
        self.0.recherche.entree().clone()
    }

    pub fn recherche_expression_reguliere(&self, oui: bool) {
        self.0.recherche.expression_reguliere(oui);
    }

    /// Éléments que Ctrl+Tab / Ctrl+Maj+Tab parcourent, dans l'ordre visuel : boutons des
    /// messages, saisie de recherche (si ouverte), onglets, menu, terminal actif.
    fn cycle_de_focus(&self) -> Vec<gtk::Widget> {
        let mut v: Vec<gtk::Widget> = self.0.bandeau.boutons();
        if self.0.recherche.ouverte() {
            v.push(self.0.recherche.entree().clone().upcast());
        }
        if self.0.carnet.n_pages() > 0 {
            v.push(self.0.carnet.clone().upcast());
        }
        v.push(self.0.menu.clone().upcast());
        if let Some(o) = self.onglet_actif() {
            v.push(o.terminal.terminal().clone().upcast());
        }
        v.retain(|w| w.is_visible() && w.is_sensitive());
        v
    }

    /// Ctrl+Tab : élément suivant (ou précédent) du cycle ; boucle, jamais de piège.
    fn deplacer_focus(&self, sens: gtk::DirectionType) {
        let cycle = self.cycle_de_focus();
        if cycle.is_empty() {
            return;
        }
        let actuel = GtkWindowExt::focus(&self.0.fenetre);
        // Élément du cycle qui contient le focus ; le plus profond, car le menu et le
        // terminal sont eux-mêmes dans le carnet d'onglets.
        let profondeur =
            |w: &gtk::Widget| std::iter::successors(w.parent(), |p| p.parent()).count();
        let position = actuel.as_ref().and_then(|w| {
            cycle
                .iter()
                .enumerate()
                .filter(|(_, c)| *c == w || w.is_ancestor(*c))
                .max_by_key(|(_, c)| profondeur(c))
                .map(|(i, _)| i)
        });
        let n = cycle.len();
        let suivant = match (position, sens) {
            (Some(i), gtk::DirectionType::TabBackward) => (i + n - 1) % n,
            (Some(i), _) => (i + 1) % n,
            (None, _) => n - 1, // focus perdu : retour au terminal
        };
        cycle[suivant].grab_focus();
    }

    /// Bouton du menu principal.
    pub fn bouton_menu(&self) -> gtk::MenuButton {
        self.0.menu.clone()
    }

    /// « Aucun résultat » si la dernière recherche n'a rien trouvé, sinon vide.
    pub fn etat_recherche(&self) -> String {
        self.0.recherche.etat()
    }

    pub fn fermer_recherche(&self) {
        self.0.recherche.fermer();
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
        let avec = gio::SimpleAction::new("nouvel-onglet-avec", Some(glib::VariantTy::STRING));
        let faible = Rc::downgrade(&self.0);
        avec.connect_activate(move |_, cible| {
            let (Some(w), Some(cible)) = (faible.upgrade(), cible.and_then(|c| c.get::<String>()))
            else {
                return;
            };
            FenetrePupitre(w).ouvrir_avec(&cible);
        });
        self.0.fenetre.add_action(&avec);
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
        for (nom, r) in [
            ("connexions", Raccourci::Connexions),
            ("copier", Raccourci::Copier),
            ("coller", Raccourci::Coller),
            ("rechercher", Raccourci::Rechercher),
            ("plein-ecran", Raccourci::PleinEcran),
            ("zoom-plus", Raccourci::ZoomPlus),
            ("zoom-moins", Raccourci::ZoomMoins),
            ("zoom-normal", Raccourci::ZoomNormal),
        ] {
            ajouter(nom, Box::new(move |f| f.executer(r)));
        }
        ajouter(
            "detacher-onglet",
            Box::new(|f| {
                if let Some(o) = f.onglet_actif() {
                    f.detacher_onglet(o.terminal.id());
                }
            }),
        );
    }

    fn installer_signaux(&self) {
        let faible = Rc::downgrade(&self.0);
        self.0.carnet.connect_page_removed(move |c, _, _| {
            decrire_onglets(c);
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
            decrire_onglets(c);
        });
        let faible = Rc::downgrade(&self.0);
        self.0.carnet.connect_switch_page(move |c, page, _| {
            let Some(f) = faible.upgrade() else { return };
            // Flèches sur la barre d'onglets : le clavier y reste et l'onglet choisi est
            // annoncé. Sinon (Alt+chiffre, clic, nouvel onglet) : retour au terminal.
            let sur_les_onglets = c.has_focus();
            if let Some(o) = f.pupitre.upgrade().and_then(|p| p.onglet_de_widget(page)) {
                o.signaler(false);
                f.fenetre.set_title(Some(&titre_fenetre(&o.titre())));
                if !sur_les_onglets {
                    o.terminal.widget().grab_focus();
                }
            }
            // « switch-page » précède le changement de page : décrire une fois fait. Orca
            // lit la nouvelle description si le focus est sur le carnet ou dedans.
            let c = c.downgrade();
            glib::idle_add_local_once(move || {
                if let Some(c) = c.upgrade() {
                    decrire_onglets(&c);
                }
            });
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
                tr("fermeture-fenetre-une-commande")
            } else {
                ta(
                    "fermeture-fenetre-commandes",
                    &[("n", ArgI18n::from(occupes))],
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
            &tr("fermeture-onglet-commande"),
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
    let terminal = TerminalOnglet::new(client, id, &p.config_effective.borrow(), &p.theme.borrow());
    let etiquette = gtk::Label::builder()
        .label(tr("onglet-titre-defaut"))
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .width_chars(8)
        .max_width_chars(24)
        .build();
    let entree = gtk::Entry::builder()
        .width_chars(16)
        .tooltip_text(tr("onglet-nom-aide"))
        .build();
    entree.update_property(&[gtk::accessible::Property::Label(&tr("onglet-nom"))]);
    entree.set_visible(false);
    let tete = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    tete.append(&etiquette);
    tete.append(&entree);

    let o = Rc::new(Onglet {
        terminal,
        tete,
        etiquette,
        entree,
        nom: RefCell::new(None),
        provisoire: RefCell::new(None),
        occupe: Cell::new(false),
        termine: Cell::new(false),
        attend_relecture: Cell::new(false),
        signale: Cell::new(false),
        annonces: RefCell::new(VecDeque::new()),
    });

    // Fin d'une commande vue en direct : annoncée selon le réglage, seulement si l'onglet
    // est affiché dans la fenêtre active.
    let (faible, pupitre) = (Rc::downgrade(&o), Rc::downgrade(p));
    o.terminal.connecter_fin_commande(move |code, duree| {
        let (Some(o), Some(p)) = (faible.upgrade(), pupitre.upgrade()) else {
            return;
        };
        let mode = p.config.borrow().annonce_fin_commande;
        if doit_annoncer(mode, duree) && o.visible_et_actif() {
            o.annoncer_commande(&texte_annonce(code, duree));
        }
    });

    // Cloche : ● si l'onglet n'est pas visible, ou si le flash est remplacé (animations
    // réduites). Effacé quand l'onglet devient actif ou à la prochaine frappe.
    let faible = Rc::downgrade(&o);
    o.terminal.connecter_cloche(move || {
        if let Some(o) = faible.upgrade() {
            let visible = o
                .carnet()
                .is_some_and(|c| c.page_num(&o.widget()) == c.current_page());
            if !visible || !animations_actives() {
                o.signaler(true);
            }
        }
    });
    let faible = Rc::downgrade(&o);
    o.terminal.terminal().connect_commit(move |_, _, _| {
        if let Some(o) = faible.upgrade() {
            o.signaler(false);
        }
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
                menu.set_parent(&o.tete);
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
    o.tete.add_controller(clic);

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

/// Si la famille de `police` est introuvable : `(famille demandée, police de secours à la
/// même taille)`. `None` si la police est disponible.
fn police_de_secours(police: &str) -> Option<(String, String)> {
    let demandee = gtk::pango::FontDescription::from_string(police);
    let famille = demandee.family().map(|f| f.to_string()).unwrap_or_default();
    let contexte = gtk::Label::new(None).pango_context();
    let connue = !famille.is_empty()
        && famille.split(',').any(|f| {
            let f = f.trim();
            contexte
                .list_families()
                .iter()
                .any(|x| x.name().eq_ignore_ascii_case(f))
        });
    if connue {
        return None;
    }
    let mut secours = gtk::pango::FontDescription::new();
    secours.set_family(POLICE_DE_SECOURS);
    let taille = if demandee.size() > 0 {
        demandee.size()
    } else {
        TAILLE_DE_SECOURS * gtk::pango::SCALE
    };
    secours.set_size(taille);
    Some((famille, secours.to_string()))
}

fn menu_onglet() -> gio::Menu {
    let m = gio::Menu::new();
    m.append(
        Some(&tr("menu-renommer-onglet")),
        Some("win.renommer-onglet"),
    );
    m.append(
        Some(&tr("menu-detacher-onglet")),
        Some("win.detacher-onglet"),
    );
    m.append(Some(&tr("menu-fermer-onglet")), Some("win.fermer-onglet"));
    m
}

/// Menu principal, à droite de la barre d'onglets.
fn bouton_menu() -> (gtk::MenuButton, gio::Menu) {
    let m = gio::Menu::new();
    let item = |libelle: &str, action: &str, accel: Option<&str>| {
        let i = gio::MenuItem::new(Some(libelle), Some(action));
        if let Some(a) = accel {
            i.set_attribute_value("accel", Some(&a.to_variant()));
        }
        i
    };
    m.append_item(&item(
        &tr("menu-nouvel-onglet"),
        "win.nouvel-onglet",
        Some("<Control><Shift>t"),
    ));
    let sous_menu_shells = gio::Menu::new();
    m.append_submenu(Some(&tr("menu-nouvel-onglet-avec")), &sous_menu_shells);
    m.append_item(&item(
        &tr("menu-nouvelle-fenetre"),
        "win.nouvelle-fenetre",
        Some("<Control><Shift>n"),
    ));
    m.append_item(&item(
        &tr("menu-connexions"),
        "win.connexions",
        Some("<Control><Shift>o"),
    ));
    let edition = gio::Menu::new();
    edition.append_item(&item(
        &tr("menu-copier"),
        "win.copier",
        Some("<Control><Shift>c"),
    ));
    edition.append_item(&item(
        &tr("menu-coller"),
        "win.coller",
        Some("<Control><Shift>v"),
    ));
    edition.append_item(&item(
        &tr("menu-rechercher"),
        "win.rechercher",
        Some("<Control><Shift>f"),
    ));
    m.append_section(None, &edition);
    let affichage = gio::Menu::new();
    affichage.append_item(&item(
        &tr("menu-agrandir"),
        "win.zoom-plus",
        Some("<Control>plus"),
    ));
    affichage.append_item(&item(
        &tr("menu-reduire"),
        "win.zoom-moins",
        Some("<Control>minus"),
    ));
    affichage.append_item(&item(
        &tr("menu-taille-normale"),
        "win.zoom-normal",
        Some("<Control>0"),
    ));
    affichage.append_item(&item(
        &tr("menu-plein-ecran"),
        "win.plein-ecran",
        Some("F11"),
    ));
    m.append_section(None, &affichage);
    let onglet = gio::Menu::new();
    onglet.append_item(&item(
        &tr("menu-renommer-onglet"),
        "win.renommer-onglet",
        None,
    ));
    onglet.append_item(&item(
        &tr("menu-detacher-onglet"),
        "win.detacher-onglet",
        None,
    ));
    onglet.append_item(&item(
        &tr("menu-fermer-onglet"),
        "win.fermer-onglet",
        Some("<Control><Shift>w"),
    ));
    m.append_section(None, &onglet);
    let b = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        // Pas de F10 : la touche reste aux programmes (mc, htop… quittent par F10).
        // Le menu s'atteint par Ctrl+Tab puis Entrée.
        .tooltip_text(tr("menu-aide"))
        .menu_model(&m)
        .primary(false)
        .build();
    b.update_property(&[gtk::accessible::Property::Label(&tr("menu-nom"))]);
    (b, sous_menu_shells)
}

/// Dialogue modal « question ? » avec Annuler (par défaut, et Échap) et Fermer.
fn confirmer(parent: &impl IsA<gtk::Window>, question: &str, si_oui: impl Fn() + 'static) {
    confirmer_action(
        parent,
        &tr("fermeture-titre"),
        question,
        &tr("bouton-fermer"),
        si_oui,
    );
}

/// Dialogue modal de confirmation : Annuler (par défaut, et Échap) ou `action`.
pub(crate) fn confirmer_action(
    parent: &impl IsA<gtk::Window>,
    titre: &str,
    question: &str,
    action: &str,
    si_oui: impl Fn() + 'static,
) {
    let dialogue = gtk::Window::builder()
        .title(titre)
        .modal(true)
        .transient_for(parent)
        .destroy_with_parent(true)
        .resizable(false)
        .accessible_role(gtk::AccessibleRole::Dialog)
        .build();
    // Rattachée à l'application : même identifiant (et donc même icône) que les fenêtres
    // principales pour le bureau.
    dialogue.set_application(parent.as_ref().application().as_ref());
    let texte = gtk::Label::builder()
        .label(question)
        .wrap(true)
        .max_width_chars(48)
        .build();
    let annuler = gtk::Button::with_label(&tr("bouton-annuler"));
    let fermer = gtk::Button::with_label(action);
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
    // Pas de relation « décrit par » vers la question : Orca lit déjà le texte du
    // dialogue, il la dirait deux fois.
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

/// Nom d'onglet tel que lu par le lecteur d'écran (« titre » ou « titre, cloche »).
fn nom_onglet(c: &gtk::Notebook, page: &gtk::Widget) -> Option<String> {
    let (titre, cloche) = titre_onglet(c, page)?;
    Some(if cloche {
        nom_avec_cloche(&titre)
    } else {
        titre
    })
}

/// Titre d'un onglet et indication de cloche, lus sur son étiquette.
fn titre_onglet(c: &gtk::Notebook, page: &gtk::Widget) -> Option<(String, bool)> {
    let tete = c.tab_label(page)?.downcast::<gtk::Box>().ok()?;
    let etiquette = tete.first_child()?.downcast::<gtk::Label>().ok()?;
    let texte = etiquette.text();
    Some(match texte.strip_prefix(MARQUE_CLOCHE) {
        Some(titre) => (titre.to_string(), true),
        None => (texte.to_string(), false),
    })
}

/// « titre, cloche » : le nom accessible d'un onglet où la cloche a sonné.
fn nom_avec_cloche(titre: &str) -> String {
    ta("onglet-nom-cloche", &[("titre", ArgI18n::from(titre))])
}

/// Titre de la fenêtre : celui de l'onglet courant, suivi du nom de l'application.
fn titre_fenetre(titre: &str) -> String {
    ta("fenetre-titre", &[("titre", ArgI18n::from(titre))])
}

/// Met à jour la description accessible du carnet : « essai, onglet 2 sur 3 », suivie
/// des autres onglets où la cloche a sonné. Orca la lit quand Ctrl+Tab arrive sur la
/// barre d'onglets, et la relit quand l'onglet change (son nom, lui, n'est pas relu
/// quand le focus remonte du terminal vers le carnet).
fn decrire_onglets(c: &gtk::Notebook) {
    let n = c.n_pages();
    let courant = c.current_page();
    let mut texte = String::new();
    let mut cloches = Vec::new();
    for i in 0..n {
        let Some(page) = c.nth_page(Some(i)) else {
            continue;
        };
        let nom = nom_onglet(c, &page).unwrap_or_default();
        let numero = ArgI18n::from(i64::from(i) + 1);
        if Some(i) == courant {
            texte = ta(
                "onglets-position",
                &[
                    ("nom", ArgI18n::from(nom)),
                    ("i", numero),
                    ("n", ArgI18n::from(i64::from(n))),
                ],
            );
        } else if let Some((titre, true)) = titre_onglet(c, &page) {
            cloches.push(ta(
                "onglets-autre-cloche",
                &[("titre", ArgI18n::from(titre)), ("i", numero)],
            ));
        }
    }
    if !cloches.is_empty() {
        texte = ta(
            "onglets-cloche-dans",
            &[
                ("texte", ArgI18n::from(texte)),
                ("liste", ArgI18n::from(cloches.join(", "))),
            ],
        );
    }
    c.update_property(&[gtk::accessible::Property::Description(&texte)]);
}
