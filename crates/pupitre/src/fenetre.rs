// SPDX-License-Identifier: GPL-3.0-or-later
//! Fenêtres et onglets. [`Pupitre`] tient la connexion au processus de fond et distribue ses
//! messages aux onglets ; chaque [`FenetrePupitre`] porte un carnet d'onglets.
//!
//! Reprise après plantage : au démarrage, les sessions déjà présentes dans le processus de
//! fond sont celles d'une interface disparue (l'application est unique par session de
//! bureau) ; elles sont rattachées et leur tampon de relecture est rejoué.

use crate::apparence::{suivre_preference_systeme, theme_systeme, Preference};
use crate::clavier::SuiviDisposition;
use crate::client::{connect_or_spawn_at, Client, CONNEXION_PERDUE};
use crate::commandes::{doit_annoncer, texte_annonce, Sens};
use crate::demande::{ouvrir_demande, DemandeAffichee};
use crate::messages::{
    annoncer_focus_une_fois_affiche, refaire_annonce_focus, ActionMessage, BandeauMessages,
};
use crate::notifications::{
    bulle_permise, id_bulle, Bulle, EnvoiBulle, Indicateur, SEUIL_BULLE_COMMANDE_MS,
};
use crate::preferences::{FichierPrefs, Lanceur};
use crate::raccourcis::{raccourci, raccourci_dans, Raccourci, TableRaccourcis};
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
use std::time::{Duration, Instant};
use vte4::prelude::*;

/// Grille annoncée pour une session tant que la vraie n'est pas connue.
const GRILLE_PAR_DEFAUT: (u16, u16) = (80, 24);
/// Délai avant de provoquer un redessin après la relecture (le temps d'une allocation).
const DELAI_REDESSIN: Duration = Duration::from_millis(150);
const GROUPE_ONGLETS: &str = "pupitre-onglets";
/// Délai avant de refaire l'annonce du lancement (voir `reannoncer_au_lancement`).
const DELAI_REANNONCE: Duration = Duration::from_millis(500);
/// Durée d'affichage des messages brefs (onglet fermé en arrière-plan, shell disparu).
const DUREE_MESSAGE_BREF: Duration = Duration::from_secs(5);
/// Durée du message « Un programme a copié N caractères ».
const DUREE_MESSAGE_COPIE: Duration = Duration::from_secs(3);

/// Clés des messages persistants (un seul message par clé).
const CLE_PANNE: &str = "panne";
const CLE_CONFIG: &str = "config";
const CLE_THEME: &str = "theme";
const CLE_CONTRASTE: &str = "contraste";
const CLE_POLICE: &str = "police";
const CLE_SURVEILLANCE: &str = "surveillance";
/// Refus dans `[raccourcis]` (valeur invalide, conflit) : message persistant.
const CLE_RACCOURCIS: &str = "raccourcis";
const CLE_FOND_ANCIEN: &str = "fond-ancien";
const CLE_CONNEXIONS: &str = "connexions-invalides";
/// Police embarquée, utilisée si celle demandée est introuvable.
const POLICE_DE_SECOURS: &str = "B612 Mono";
const TAILLE_DE_SECOURS: i32 = 11;

/// Un onglet : un terminal et son étiquette (titre ou nom choisi, modifiable sur place).
pub struct Onglet {
    terminal: TerminalOnglet,
    /// Page du carnet : le bandeau de disposition du clavier, puis le terminal.
    page: gtk::Box,
    /// Bandeau de disposition de cet onglet (suivi par `SuiviDisposition`).
    disposition: gtk::Label,
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
    /// Indicateur d'onglet : ✓, ✗ ou ● (voir `notifications`).
    indicateur: Cell<Indicateur>,
    /// Le ● vient de la cloche (et non d'une notification de programme) : nom « cloche ».
    par_cloche: Cell<bool>,
    /// Dernière bulle de bureau envoyée pour cet onglet (au plus une par intervalle).
    derniere_bulle: Cell<Option<Instant>>,
    /// Dernières annonces de commandes faites au lecteur d'écran (les plus anciennes sont
    /// oubliées).
    annonces: RefCell<VecDeque<String>>,
    /// Ce qui a été lancé dans l'onglet (`Defaut` pour une session reprise) : un nouvel
    /// onglet ouvert depuis celui-ci en reprend le shell.
    choix: RefCell<Choix>,
    /// Une réouverture du shell est demandée au fond et pas encore répondue : une seconde
    /// Entrée est ignorée (sinon le second shell resterait orphelin dans le fond).
    reouverture_en_cours: Cell<bool>,
}

/// Annonces de commandes gardées par onglet (observées par les tests).
const ANNONCES_GARDEES: usize = 32;

impl Onglet {
    /// Ce qu'ouvre un nouvel onglet demandé depuis celui-ci : le même shell, sauf pour une
    /// connexion (pas un shell), qui donne le shell des réglages.
    fn choix_a_reprendre(&self) -> Choix {
        match &*self.choix.borrow() {
            Choix::Commande(_) => Choix::Defaut,
            c => c.clone(),
        }
    }

    pub fn terminal(&self) -> &TerminalOnglet {
        &self.terminal
    }

    pub fn widget(&self) -> gtk::Widget {
        self.page.clone().upcast()
    }

    /// Bandeau de disposition du clavier, en tête de la page de l'onglet.
    pub fn bandeau_clavier(&self) -> gtk::Label {
        self.disposition.clone()
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
        self.indicateur.get() == Indicateur::Signale
    }

    /// Indicateur actuellement porté par l'onglet.
    pub fn indicateur(&self) -> Indicateur {
        self.indicateur.get()
    }

    /// Nom accessible de l'onglet, indicateur compris (« titre, commande réussie »…).
    pub fn nom_accessible(&self) -> String {
        self.nom_pour(&self.titre())
    }

    fn nom_pour(&self, titre: &str) -> String {
        if self.par_cloche.get() {
            nom_avec_cloche(titre)
        } else {
            self.indicateur.get().nom_accessible(titre)
        }
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
        // Sur l'objet accessible du terminal de l'onglet, poliment.
        self.terminal
            .terminal()
            .announce(texte, gtk::AccessibleAnnouncementPriority::Medium);
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
        // Toujours une annonce, échec compris : jamais de silence après la touche.
        self.terminal.naviguer(sens, move |n| {
            if let Some(o) = faible.upgrade() {
                o.annoncer_commande(&n.texte());
            }
        });
    }

    /// La cloche a sonné (`true`) ; ou tout indicateur est effacé (`false`).
    fn signaler(&self, oui: bool) {
        if oui {
            self.indiquer(Indicateur::Signale, true);
        } else {
            self.indiquer(Indicateur::Aucun, false);
        }
    }

    /// Pose l'indicateur de l'onglet (`cloche` : le ● est celui de la cloche).
    fn indiquer(&self, i: Indicateur, cloche: bool) {
        let cloche = cloche && i == Indicateur::Signale;
        let change_i = self.indicateur.replace(i) != i;
        let change_c = self.par_cloche.replace(cloche) != cloche;
        if change_i || change_c {
            self.rafraichir_titre();
        }
    }

    fn rafraichir_titre(&self) {
        let titre = self.titre();
        let indicateur = self.indicateur.get();
        if indicateur != Indicateur::Aucun {
            // La marque est doublée d'un texte pour le lecteur d'écran.
            let nom = self.nom_pour(&titre);
            self.etiquette
                .set_text(&format!("{}{titre}", indicateur.marque()));
            self.etiquette
                .set_tooltip_text(Some(&if self.par_cloche.get() {
                    ta(
                        "onglet-infobulle-cloche",
                        &[("titre", ArgI18n::from(titre.as_str()))],
                    )
                } else {
                    indicateur.infobulle(&titre)
                }));
            self.etiquette
                .update_property(&[gtk::accessible::Property::Label(&nom)]);
            // Relu par `nom_onglet` (description du carnet).
            self.etiquette.set_widget_name(&nom);
        } else {
            self.etiquette.set_text(&titre);
            self.etiquette.set_tooltip_text(Some(&titre));
            self.etiquette
                .reset_property(gtk::AccessibleProperty::Label);
            self.etiquette.set_widget_name("");
        }
        // Le nom accessible de l'onglet suit son étiquette ; le terminal porte le même nom.
        self.terminal
            .terminal()
            .update_property(&[gtk::accessible::Property::Label(&titre)]);
        // GTK ne signale pas le changement de texte de l'étiquette : les lecteurs d'écran
        // gardent l'ancien nom de l'onglet. Orca 48, voyant alors un onglet et un terminal de
        // noms différents, quitte le terminal à la sélection de l'onglet et ne lit plus sa
        // sortie. Le nom posé sur l'objet « onglet » de GTK, lui, est signalé.
        if let Some(onglet) = self
            .tete
            .parent()
            .filter(|w| w.accessible_role() == gtk::AccessibleRole::Tab)
        {
            let nom = self.nom_pour(&titre);
            onglet.update_property(&[gtk::accessible::Property::Label(&nom)]);
        }
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
        // Une connexion ou un éditeur n'est pas un shell (Entrée ouvre alors un shell).
        let programme = matches!(*self.choix.borrow(), Choix::Commande(_));
        let message = match (code, programme) {
            (Some(c), false) => ta("shell-termine", &[("code", ArgI18n::from(c.to_string()))]),
            (Some(c), true) => ta(
                "programme-termine",
                &[("code", ArgI18n::from(c.to_string()))],
            ),
            (None, false) => tr("shell-termine-signal"),
            (None, true) => tr("programme-termine-signal"),
        };
        self.terminal
            .on_output(format!("\r\n\x1b[0m{message}\r\n").as_bytes());
        crate::messages::annoncer(self.terminal.terminal(), &message);
    }
}

enum Attente {
    /// `Open` envoyé pour un nouvel onglet de cette fenêtre (titre provisoire, choix lancé).
    NouvelOnglet(Weak<FenetreInterne>, Option<String>, Choix),
    /// `Open` envoyé pour remplacer le shell terminé de cet onglet.
    Remplacer(Weak<Onglet>),
}

/// L'application : connexion au processus de fond, fenêtres, onglets, thème.
pub struct Pupitre {
    app: gtk::Application,
    socket: PathBuf,
    /// Configuration lue (dernière valide).
    config: RefCell<Config>,
    /// Raccourcis en vigueur (défauts et `[raccourcis]`).
    raccourcis: RefCell<TableRaccourcis>,
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
    /// Numéro de la dernière recherche des shells installés (une par relecture de la
    /// configuration) : seule la plus récente est appliquée.
    recherche_shells: Cell<u64>,
    /// Dernier résultat de la recherche : `(libellé, valeur de l'action)` du sous-menu.
    entrees_shells: RefCell<Option<Vec<(String, String)>>>,
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
    /// Envoi des bulles de bureau (remplaçable par les tests).
    envoi_bulle: RefCell<EnvoiBulle>,
    /// Disposition du clavier : un seul suivi pour toutes les fenêtres.
    disposition: Rc<SuiviDisposition>,
}

impl Pupitre {
    /// Se connecte au processus de fond (en le lançant au besoin), puis ouvre la première
    /// fenêtre : avec les sessions orphelines s'il y en a (reprise), sinon avec un shell neuf.
    pub fn demarrer(app: &gtk::Application, socket: PathBuf, config: Config) -> Rc<Pupitre> {
        let p = Rc::new(Pupitre {
            app: app.clone(),
            socket,
            config: RefCell::new(config.clone()),
            raccourcis: RefCell::new(TableRaccourcis::defaut()),
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
            recherche_shells: Cell::new(0),
            entrees_shells: RefCell::new(None),
            onglets: RefCell::new(Vec::new()),
            fenetres: RefCell::new(Vec::new()),
            demandes: RefCell::new(HashMap::new()),
            informations: RefCell::new(HashSet::new()),
            demandes_sans_fenetre: RefCell::new(Vec::new()),
            messages_sans_fenetre: RefCell::new(Vec::new()),
            garde: RefCell::new(Some(app.hold())),
            avertis: RefCell::new(HashSet::new()),
            envoi_bulle: RefCell::new(crate::notifications::envoi_reel(app)),
            disposition: SuiviDisposition::new(app, config.bandeau_clavier),
        });
        p.installer_action_montrer_onglet();
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

    /// Suivi de la disposition du clavier (un seul pour toute l'application).
    pub fn disposition(&self) -> Rc<SuiviDisposition> {
        self.disposition.clone()
    }

    /// Remplace l'envoi des bulles de bureau (les tests y comptent les bulles).
    pub fn remplacer_envoi_bulle(&self, envoi: EnvoiBulle) {
        *self.envoi_bulle.borrow_mut() = envoi;
    }

    /// Action `app.montrer-onglet` (paramètre : numéro de session, `t`) : le clic sur une
    /// bulle montre la fenêtre et l'onglet.
    fn installer_action_montrer_onglet(self: &Rc<Self>) {
        let action = gio::SimpleAction::new("montrer-onglet", Some(glib::VariantTy::UINT64));
        let faible = Rc::downgrade(self);
        action.connect_activate(move |_, valeur| {
            if let (Some(p), Some(id)) = (faible.upgrade(), valeur.and_then(|v| v.get::<u64>())) {
                p.montrer_onglet(id);
            }
        });
        self.app.add_action(&action);
    }

    /// Montre l'onglet de la session `id` : sa fenêtre est présentée, l'onglet choisi.
    /// Une session inconnue (onglet fermé depuis) est sans effet.
    fn montrer_onglet(&self, id: SessionId) {
        let Some(o) = self.onglet(id) else { return };
        let Some(f) = self.fenetre_de_onglet(&o) else {
            return;
        };
        if let Some(n) = f.0.carnet.page_num(&o.widget()) {
            f.0.carnet.set_current_page(Some(n));
        }
        f.fenetre().present();
        o.terminal.widget().grab_focus();
    }

    fn fenetre_de_onglet(&self, o: &Rc<Onglet>) -> Option<FenetrePupitre> {
        self.fenetres()
            .into_iter()
            .find(|f| f.onglets().iter().any(|x| Rc::ptr_eq(x, o)))
    }

    /// Aucune fenêtre de Pupitre n'est active : l'utilisateur est ailleurs.
    fn aucune_fenetre_active(&self) -> bool {
        !self
            .fenetres()
            .iter()
            .any(|f| crate::commandes::fenetre_active(f.fenetre().is_active()))
    }

    /// Envoie une bulle de bureau pour `o`, sauf si l'onglet en a déjà fait partir une il y
    /// a moins de `INTERVALLE_BULLES`. Renvoie `true` si la bulle est partie.
    fn envoyer_bulle(&self, o: &Rc<Onglet>, titre: String, corps: String) -> bool {
        let maintenant = Instant::now();
        if !bulle_permise(o.derniere_bulle.get(), maintenant) {
            return false;
        }
        o.derniere_bulle.set(Some(maintenant));
        let session = o.terminal.id();
        // Copié avant l'appel : le rappel ne tient aucun emprunt.
        let envoi = self.envoi_bulle.borrow().clone();
        envoi(&Bulle {
            id: id_bulle(session),
            session,
            titre,
            corps,
        });
        true
    }

    /// Fin d'une commande vue en direct dans `o` : annonce à Orca, indicateur ✓ ou ✗ si
    /// l'onglet n'est pas affiché dans une fenêtre active, bulle si l'utilisateur est parti
    /// (aucune fenêtre active) et que la commande a duré plus de 10 s.
    fn fin_commande(&self, o: &Rc<Onglet>, code: Option<i32>, duree: Option<u64>) {
        let (mode, reglages) = {
            let c = self.config.borrow();
            (c.annonce_fin_commande, c.notifications)
        };
        let affiche = o.visible_et_actif();
        if doit_annoncer(mode, duree) && affiche {
            o.annoncer_commande(&texte_annonce(code, duree));
        }
        // Jamais un seul canal : quand une bulle part, l'indicateur est posé même si la case
        // `fin_commande_onglet` est à false.
        let bulle = reglages.fin_commande_bulle
            && duree.is_some_and(|d| d > SEUIL_BULLE_COMMANDE_MS)
            && self.aucune_fenetre_active()
            && self.envoyer_bulle(o, o.titre(), texte_annonce(code, duree));
        // Une notification de programme en attente n'est pas remplacée par ✓ ou ✗.
        if !affiche
            && (reglages.fin_commande_onglet || bulle)
            && o.indicateur() != Indicateur::Signale
        {
            let echec = code.is_some_and(|c| c != 0);
            o.indiquer(
                if echec {
                    Indicateur::Echec
                } else {
                    Indicateur::Reussi
                },
                false,
            );
        }
    }

    /// Notification d'un programme (OSC 9, 777) : ● sur l'onglet, bulle du bureau.
    fn notification_programme(&self, id: SessionId, titre: Option<String>, corps: String) {
        let Some(o) = self.onglet(id).filter(|o| !o.attend_relecture.get()) else {
            return;
        };
        let reglages = self.config.borrow().notifications;
        let titre = titre
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| o.titre());
        let bulle = reglages.programme_bulle && self.envoyer_bulle(&o, titre, corps);
        // Jamais un seul canal : quand une bulle part, l'indicateur est posé même si la case
        // `programme_onglet` est à false.
        if reglages.programme_onglet || bulle {
            o.indiquer(Indicateur::Signale, false);
        }
    }

    /// Copie demandée par un programme (OSC 52, écriture) : dans le presse-papiers, et
    /// toujours signalée dans la fenêtre pendant 3 s (« Un programme a copié N caractères »).
    fn copie_programme(&self, id: SessionId, texte: &str) {
        if !self.config.borrow().notifications.presse_papiers || texte.is_empty() {
            return;
        }
        let Some(o) = self.onglet(id).filter(|o| !o.attend_relecture.get()) else {
            return;
        };
        o.terminal.terminal().clipboard().set_text(texte);
        let n = i64::try_from(texte.chars().count()).unwrap_or(i64::MAX);
        let message = ta("presse-papiers-copie", &[("n", ArgI18n::Nombre(n))]);
        // La fenêtre où l'on regarde : l'active, sinon celle de l'onglet.
        let cible = self
            .fenetres()
            .into_iter()
            .find(|f| crate::commandes::fenetre_active(f.fenetre().is_active()))
            .or_else(|| self.fenetre_de_onglet(&o))
            .or_else(|| self.fenetres().into_iter().next());
        if let Some(f) = cible {
            f.0.bandeau.afficher(&message, Some(DUREE_MESSAGE_COPIE));
        }
    }

    /// Applique une configuration relue : valide → appliquée à tous les onglets ;
    /// invalide → la précédente est gardée et un message persistant donne la ligne.
    pub fn appliquer_config(self: &Rc<Self>, r: Result<Config, ErreurConfig>) {
        match r {
            Ok(c) => {
                self.poser_message(CLE_CONFIG, None);
                self.disposition.definir_mode(c.bandeau_clavier);
                // Une ligne refusée de `[raccourcis]` n'invalide pas le reste : défaut gardé
                // pour l'action fautive, message persistant avec la ligne.
                let (table, refus) = TableRaccourcis::depuis_reglages(&c.raccourcis);
                *self.raccourcis.borrow_mut() = table;
                self.poser_message(CLE_RACCOURCIS, (!refus.is_empty()).then(|| refus.join(" ")));
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
        self.chercher_shells();
        for f in self.fenetres() {
            f.rafraichir_menu();
        }
    }

    /// Nombre de recherches des shells lancées depuis le démarrage (observé par les tests).
    pub fn recherches_shells(&self) -> u64 {
        self.recherche_shells.get()
    }

    /// Cherche les shells installés sur un fil à part (un dossier du `PATH` sur un montage
    /// réseau figé ne doit pas geler l'interface), une seule fois pour toutes les fenêtres.
    /// Les menus gardent leurs entrées en attendant ; le fil ne touche à aucun objet GTK.
    fn chercher_shells(self: &Rc<Self>) {
        let numero = self.recherche_shells.get() + 1;
        self.recherche_shells.set(numero);
        let config = self.config.borrow().clone();
        let path = shells::path_courant();
        let (tx, rx) = async_channel::bounded(1);
        let lance = std::thread::Builder::new()
            .name("recherche-shells".into())
            .spawn(move || {
                let _ = tx.send_blocking(shells::entrees_menu(&config, &path));
            });
        if let Err(e) = lance {
            eprintln!("pupitre: recherche des shells impossible : {e}");
            return;
        }
        let faible = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let Ok(entrees) = rx.recv().await else { return };
            let Some(p) = faible.upgrade() else { return };
            if p.recherche_shells.get() != numero {
                return; // une relecture plus récente est en route
            }
            *p.entrees_shells.borrow_mut() = Some(entrees);
            for f in p.fenetres() {
                f.rafraichir_shells();
            }
        });
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
        if !onglet.termine() || onglet.reouverture_en_cours.get() || self.client.borrow().is_none()
        {
            return;
        }
        let (cols, rows) = onglet.terminal.grille().unwrap_or(GRILLE_PAR_DEFAUT);
        let parti = self.ouvrir_session(
            &Choix::Defaut,
            cols,
            rows,
            Attente::Remplacer(Rc::downgrade(onglet)),
        );
        onglet.reouverture_en_cours.set(parti);
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
        // Les réponses attendues de l'ancienne connexion n'arriveront plus.
        self.vider_attentes();
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
    /// `attente` n'est mise en file que si la demande part : sans processus de fond, rien ne
    /// doit rester en attente (le `Opened` suivant serait apparié à une demande périmée).
    /// Faux si la demande n'a pas pu partir.
    fn ouvrir_session(&self, choix: &Choix, cols: u16, rows: u16, attente: Attente) -> bool {
        if self.client.borrow().is_none() {
            return false;
        }
        let lancement = shells::preparer(choix, &self.config.borrow(), &shells::path_courant());
        if let Some(a) = lancement.avertissement {
            if self.avertis.borrow_mut().insert(a.clone()) {
                self.poser_message(&format!("lancement:{a}"), Some(a));
            }
        }
        self.attentes.borrow_mut().push_back(attente);
        let envoye = self.client.borrow().as_ref().is_some_and(|c| {
            c.ouvrir(Some(lancement.argv), None, cols, rows, lancement.env)
                .is_ok()
        });
        if !envoye {
            // Connexion perdue (signalée par ailleurs sur le canal) : pas de réponse à attendre.
            self.attentes.borrow_mut().pop_back();
        }
        envoye
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
        // L'aide ouverte prend aussi les nouvelles couleurs et police.
        crate::aide::definir_apparence(&config.police, &theme);
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
            .find(|o| o.page.upcast_ref::<gtk::Widget>() == w)
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
                    Some(Attente::NouvelOnglet(f, titre, choix)) => match f.upgrade() {
                        Some(f) if !f.fermee.get() => {
                            if let Some(o) = FenetrePupitre(f).ajouter_onglet(id) {
                                *o.provisoire.borrow_mut() = titre;
                                *o.choix.borrow_mut() = choix;
                                o.rafraichir_titre();
                            }
                        }
                        _ => self.envoyer(ClientMsg::Close { id }),
                    },
                    Some(Attente::Remplacer(o)) => match o.upgrade().inspect(|o| {
                        o.reouverture_en_cours.set(false);
                    }) {
                        Some(o) if o.carnet().is_some() => {
                            o.terminal.relier(id);
                            o.termine.set(false);
                            o.occupe.set(false);
                            // `rouvrir` lance le shell des réglages.
                            *o.choix.borrow_mut() = Choix::Defaut;
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
                    Some(Attente::NouvelOnglet(f, _, _)) => {
                        if let Some(f) = f.upgrade() {
                            FenetrePupitre(f).afficher_message(&message);
                        }
                    }
                    Some(Attente::Remplacer(o)) => {
                        if let Some(o) = o.upgrade() {
                            o.reouverture_en_cours.set(false);
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
                    let shell = !matches!(*o.choix.borrow(), Choix::Commande(_));
                    if crate::terminal::fin_voulue(code, o.terminal.invite_vue(), shell) {
                        // Fin voulue (exit, Ctrl+D) : l'onglet se ferme. Avant la première
                        // invite ou par un signal, il reste, avec le code et le moyen d'ouvrir
                        // un nouveau shell.
                        o.termine.set(true);
                        self.fermer_onglet_fini(&o);
                    } else {
                        o.marquer_termine(code);
                    }
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
            ServerMsg::PressePapiers { id, texte } => self.copie_programme(id, &texte),
            ServerMsg::Notification { id, titre, corps } => {
                self.notification_programme(id, titre, corps);
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

    /// Les réponses attendues n'arriveront plus : aucune réouverture n'est plus en cours.
    fn vider_attentes(&self) {
        self.attentes.borrow_mut().clear();
        for o in self.onglets.borrow().iter() {
            o.reouverture_en_cours.set(false);
        }
    }

    fn connexion_perdue(self: &Rc<Self>) {
        *self.client.borrow_mut() = None;
        self.generation.set(self.generation.get() + 1);
        self.attend_liste.set(false);
        self.vider_attentes();
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

    /// Ferme l'onglet dont le shell s'est terminé. S'il n'était pas l'onglet affiché de la
    /// fenêtre active, la fermeture est annoncée (sinon le nombre d'onglets changerait en
    /// silence) : dans la fenêtre active, sinon dans la sienne si elle reste ouverte, sinon
    /// dans une autre.
    fn fermer_onglet_fini(&self, o: &Rc<Onglet>) {
        let sa_fenetre = self
            .fenetres()
            .into_iter()
            .find(|f| f.onglets().iter().any(|x| Rc::ptr_eq(x, o)));
        let affiche = sa_fenetre.as_ref().is_some_and(|f| {
            f.fenetre().is_active() && f.onglet_actif().is_some_and(|a| Rc::ptr_eq(&a, o))
        });
        let cle = if matches!(*o.choix.borrow(), Choix::Commande(_)) {
            "onglet-ferme-fin-programme"
        } else {
            "onglet-ferme-fin"
        };
        let texte = ta(cle, &[("titre", ArgI18n::from(o.titre()))]);
        self.fermer_onglet(o);
        if affiche {
            return;
        }
        // La fenêtre active d'abord : c'est là que le lecteur d'écran lit les annonces.
        let ouvertes: Vec<FenetrePupitre> = self
            .fenetres()
            .into_iter()
            .filter(|f| !f.onglets().is_empty())
            .collect();
        let cible = ouvertes
            .iter()
            .find(|f| f.fenetre().is_active())
            .cloned()
            .or_else(|| sa_fenetre.filter(|f| !f.onglets().is_empty()))
            .or_else(|| ouvertes.first().cloned());
        if let Some(f) = cible {
            f.0.bandeau.afficher(&texte, Some(DUREE_MESSAGE_BREF));
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
    /// Bouton « + » (même action que `win.nouvel-onglet`), à gauche du menu.
    bouton_plus: gtk::Button,
    /// Sous-menu « Nouvel onglet avec » (refait quand les réglages changent).
    sous_menu_shells: gio::Menu,
    /// Raccourcis affichés dans le menu (refait quand ils changent).
    raccourcis_du_menu: RefCell<TableRaccourcis>,
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
        let table = pupitre.raccourcis.borrow().clone();
        let (menu, sous_menu_shells) = bouton_menu(&table);
        let bouton_plus = bouton_nouvel_onglet(&table);
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        actions.append(&bouton_plus);
        actions.append(&menu);
        carnet.set_action_widget(&actions, gtk::PackType::End);

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
            bouton_plus,
            sous_menu_shells,
            raccourcis_du_menu: RefCell::new(table),
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
        // Fenêtre redevenue active : l'onglet affiché perd son indicateur (on le voit).
        let faible = Rc::downgrade(&f.0);
        f.0.fenetre.connect_is_active_notify(move |w| {
            if !w.is_active() {
                return;
            }
            if let Some(o) = FenetrePupitre::depuis_faible(&faible).and_then(|f| f.onglet_actif()) {
                o.signaler(false);
            }
        });
        pupitre.fenetres.borrow_mut().push(f.clone());
        pupitre.garde.borrow_mut().take();
        f
    }

    /// Bandeau de disposition de l'onglet actif (en tête de sa page, sous la barre d'onglets).
    pub fn bandeau_clavier(&self) -> Option<gtk::Label> {
        self.onglet_actif().map(|o| o.bandeau_clavier())
    }

    /// Carnet d'onglets (barre d'onglets et terminaux).
    pub fn carnet(&self) -> gtk::Notebook {
        self.0.carnet.clone()
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
        let choix = self
            .onglet_actif()
            .map_or(Choix::Defaut, |o| o.choix_a_reprendre());
        self.ouvrir(&choix, None);
    }

    /// Ouvre un nouvel onglet pour une entrée du sous-menu « Nouvel onglet avec » (valeur
    /// d'action rendue par [`entrees_nouvel_onglet_avec`](Self::entrees_nouvel_onglet_avec)).
    fn ouvrir_avec(&self, cible: &str) {
        let Some(p) = self.pupitre() else { return };
        let choix = shells::choix_de_cible(cible, &p.config.borrow());
        match choix {
            Some(c) => {
                self.ouvrir(&c, None);
            }
            // Menu pas encore refait après une modification de la configuration.
            None => {
                self.0
                    .bandeau
                    .afficher(&tr("shell-declare-disparu"), Some(DUREE_MESSAGE_BREF));
            }
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
    /// Inchangé : laissé tel quel (le refaire pendant qu'il est ouvert perd la surbrillance et
    /// le clic en cours).
    fn rafraichir_shells(&self) {
        let Some(p) = self.pupitre() else { return };
        // Pas encore de résultat : le sous-menu garde ses entrées.
        let Some(entrees) = p.entrees_shells.borrow().clone() else {
            return;
        };
        if entrees == self.entrees_nouvel_onglet_avec() {
            return;
        }
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

    /// Refait le menu quand les raccourcis en vigueur ont changé (inchangés : laissé tel quel,
    /// pour ne pas perdre la surbrillance d'un menu ouvert).
    fn rafraichir_menu(&self) {
        let Some(p) = self.pupitre() else { return };
        let table = p.raccourcis.borrow().clone();
        if *self.0.raccourcis_du_menu.borrow() == table {
            return;
        }
        self.0
            .menu
            .set_menu_model(Some(&construire_menu(&table, &self.0.sous_menu_shells)));
        self.0
            .bouton_plus
            .set_tooltip_text(Some(&infobulle_nouvel_onglet(&table)));
        *self.0.raccourcis_du_menu.borrow_mut() = table;
    }

    /// Ouvre `argv` (une commande `ssh …`) dans un nouvel onglet de cette fenêtre. `titre`
    /// reste affiché tant que le programme n'en donne pas.
    pub fn ouvrir_connexion(&self, argv: Vec<String>, titre: String) {
        self.ouvrir(&Choix::Commande(argv), Some(titre));
    }

    /// Faux si rien n'a pu être demandé (processus de fond arrêté) : la panne est alors
    /// réaffichée et réannoncée, avec son bouton « Ouvrir un nouveau terminal ».
    fn ouvrir(&self, choix: &Choix, titre: Option<String>) -> bool {
        let Some(p) = self.pupitre() else {
            return false;
        };
        let (cols, rows) = self
            .onglet_actif()
            .and_then(|o| o.terminal.grille())
            .unwrap_or(GRILLE_PAR_DEFAUT);
        let attente = Attente::NouvelOnglet(Rc::downgrade(&self.0), titre, choix.clone());
        let demande = p.ouvrir_session(choix, cols, rows, attente);
        if !demande {
            self.signaler_panne(&tr("fond-arrete"));
        }
        demande
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

    /// Ouvre un fichier de configuration dans l'éditeur par défaut ; le résultat s'affiche
    /// (et s'annonce) dans le bandeau de la fenêtre.
    /// Avec `$VISUAL` (sinon `$EDITOR`) dans un nouvel onglet s'il est défini et installé,
    /// sinon avec l'éditeur graphique du bureau.
    pub fn ouvrir_preference(&self, fichier: FichierPrefs) {
        let (visual, editor) = (std::env::var("VISUAL").ok(), std::env::var("EDITOR").ok());
        let lanceur = match crate::preferences::editeur_terminal(
            visual.as_deref(),
            editor.as_deref(),
            &shells::path_courant(),
        ) {
            Some(editeur) => self.lanceur_onglet(editeur),
            None => crate::preferences::lanceur_gtk(self.fenetre().clone().upcast()),
        };
        self.ouvrir_preference_avec(fichier, lanceur);
    }

    /// Ouvre le fichier avec `editeur` dans un nouvel onglet de cette fenêtre, nommé d'après
    /// le fichier.
    fn lanceur_onglet(&self, mut editeur: Vec<String>) -> Lanceur {
        let faible = self.faible();
        Box::new(move |chemin, fin| {
            let Some(f) = faible.upgrade() else {
                return fin(Err("fenêtre fermée".into()));
            };
            editeur.push(chemin.to_string_lossy().into_owned());
            let titre = chemin.file_name().map(|n| n.to_string_lossy().into_owned());
            if FenetrePupitre(f).ouvrir(&Choix::Commande(editeur), titre) {
                fin(Ok(()))
            } else {
                fin(Err("processus de fond arrêté".into()))
            }
        })
    }

    /// Comme `ouvrir_preference`, avec un lanceur au choix (tests : échec simulé).
    pub fn ouvrir_preference_avec(&self, fichier: FichierPrefs, lanceur: Lanceur) {
        let faible = self.faible();
        crate::preferences::ouvrir_avec(
            fichier,
            lanceur,
            Rc::new(move |texte| {
                if let Some(f) = faible.upgrade() {
                    FenetrePupitre(f).afficher_message(&texte);
                }
            }),
        );
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
        let trouve = match self.pupitre() {
            Some(p) => raccourci_dans(&p.raccourcis.borrow(), touche, code, mods),
            None => raccourci(touche, code, mods),
        };
        match trouve {
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
            Raccourci::Aide => {
                // Police, thème et zoom du moment, puis la fenêtre (une seule, rouverte au
                // premier plan).
                if let Some(p) = self.pupitre() {
                    crate::aide::definir_apparence(
                        &p.config_effective.borrow().police,
                        &p.theme.borrow(),
                    );
                }
                crate::aide::definir_zoom(actif.as_ref().map_or(100, |o| o.terminal.zoom()));
                if let Some(app) = self.fenetre().application() {
                    crate::aide::ouvrir_aide(&app, Some(self.fenetre().upcast_ref()), None);
                }
            }
            Raccourci::Reglages => {
                if let Some(app) = self.fenetre().application() {
                    crate::reglages::ouvrir_reglages(&app, self.fenetre().upcast_ref());
                }
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
    /// messages, saisie de recherche (si ouverte), onglets, bouton « + », menu, terminal actif.
    pub fn elements_du_cycle(&self) -> Vec<gtk::Widget> {
        self.cycle_de_focus()
    }

    fn cycle_de_focus(&self) -> Vec<gtk::Widget> {
        let mut v: Vec<gtk::Widget> = self.0.bandeau.boutons();
        if self.0.recherche.ouverte() {
            v.push(self.0.recherche.entree().clone().upcast());
        }
        if self.0.carnet.n_pages() > 0 {
            v.push(self.0.carnet.clone().upcast());
        }
        v.push(self.0.bouton_plus.clone().upcast());
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

    /// Bouton « + » de la barre d'onglets.
    pub fn bouton_nouvel_onglet(&self) -> gtk::Button {
        self.0.bouton_plus.clone()
    }

    /// Bouton du menu principal.
    pub fn bouton_menu(&self) -> gtk::MenuButton {
        self.0.menu.clone()
    }

    /// Entrées du menu principal (sans le sous-menu « Nouvel onglet avec ») avec leur
    /// raccourci affiché : `(libellé, accélérateur GTK)`.
    pub fn raccourcis_affiches_du_menu(&self) -> Vec<(String, Option<String>)> {
        fn parcourir(m: &gio::MenuModel, sortie: &mut Vec<(String, Option<String>)>) {
            let texte = |i: i32, attr: &str| {
                m.item_attribute_value(i, attr, Some(glib::VariantTy::STRING))
                    .and_then(|v| v.get::<String>())
            };
            for i in 0..m.n_items() {
                if let Some(section) = m.item_link(i, "section") {
                    parcourir(&section, sortie);
                } else if let Some(libelle) = texte(i, "label") {
                    if m.item_link(i, "submenu").is_none() {
                        sortie.push((libelle, texte(i, "accel")));
                    }
                }
            }
        }
        let mut sortie = Vec::new();
        if let Some(m) = self.0.menu.menu_model() {
            parcourir(&m, &mut sortie);
        }
        sortie
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
        ajouter(
            "a-propos",
            Box::new(|f| crate::apropos::ouvrir_apropos(f.fenetre().upcast_ref())),
        );
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
            ("aide", Raccourci::Aide),
            ("reglages", Raccourci::Reglages),
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
        for (nom, fichier) in [
            ("prefs-configuration", FichierPrefs::Configuration),
            ("prefs-invite", FichierPrefs::Invite),
            ("prefs-connexions", FichierPrefs::Connexions),
        ] {
            ajouter(nom, Box::new(move |f| f.ouvrir_preference(fichier)));
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
            // Décrire une fois l'onglet rendu actif et nommé : Orca ne lit que la première
            // description d'une rafale ; elle doit donner la bonne position.
            let c = c.downgrade();
            glib::idle_add_local_once(move || {
                if let Some(c) = c.upgrade() {
                    decrire_onglets(&c);
                }
            });
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

    let disposition = p.disposition.creer_bandeau();
    let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    page.append(&disposition);
    page.append(terminal.widget());
    let o = Rc::new(Onglet {
        terminal,
        page,
        disposition,
        tete,
        etiquette,
        entree,
        nom: RefCell::new(None),
        provisoire: RefCell::new(None),
        occupe: Cell::new(false),
        termine: Cell::new(false),
        attend_relecture: Cell::new(false),
        indicateur: Cell::new(Indicateur::Aucun),
        par_cloche: Cell::new(false),
        derniere_bulle: Cell::new(None),
        annonces: RefCell::new(VecDeque::new()),
        choix: RefCell::new(Choix::Defaut),
        reouverture_en_cours: Cell::new(false),
    });

    // Fin d'une commande vue en direct : annonce, indicateur, bulle (voir `fin_commande`).
    let (faible, pupitre) = (Rc::downgrade(&o), Rc::downgrade(p));
    o.terminal.connecter_fin_commande(move |code, duree| {
        if let (Some(o), Some(p)) = (faible.upgrade(), pupitre.upgrade()) {
            p.fin_commande(&o, code, duree);
        }
    });

    // Cloche : ● si l'onglet n'est pas visible, ou si le flash est remplacé (animations
    // réduites). Effacé quand l'onglet devient actif ou à la prochaine frappe.
    let (faible, pupitre) = (Rc::downgrade(&o), Rc::downgrade(p));
    o.terminal.connecter_cloche(move || {
        let (Some(o), Some(p)) = (faible.upgrade(), pupitre.upgrade()) else {
            return;
        };
        if p.config.borrow().notifications.cloche_onglet {
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
    let connue = || {
        let contexte = gtk::Label::new(None).pango_context();
        !famille.is_empty()
            && famille.split(',').any(|f| {
                let f = f.trim();
                contexte
                    .list_families()
                    .iter()
                    .any(|x| x.name().eq_ignore_ascii_case(f))
            })
    };
    if connue() {
        return None;
    }
    // Une police installée depuis le démarrage n'est pas encore connue : fontconfig est relu
    // une fois avant de se rabattre sur la police de secours.
    if !famille.is_empty() {
        crate::polices::relire_polices();
        if connue() {
            return None;
        }
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
fn construire_menu(table: &TableRaccourcis, sous_menu_shells: &gio::Menu) -> gio::Menu {
    let m = gio::Menu::new();
    // Les raccourcis affichés sont ceux en vigueur (défauts et `[raccourcis]`).
    let item = |libelle: &str, action: &str, accel: Option<Raccourci>| {
        let i = gio::MenuItem::new(Some(libelle), Some(action));
        if let Some(a) = accel.and_then(|r| table.accelerateur(r)) {
            i.set_attribute_value("accel", Some(&a.to_variant()));
        }
        i
    };
    m.append_item(&item(
        &tr("menu-nouvel-onglet"),
        "win.nouvel-onglet",
        Some(Raccourci::NouvelOnglet),
    ));
    m.append_submenu(Some(&tr("menu-nouvel-onglet-avec")), sous_menu_shells);
    m.append_item(&item(
        &tr("menu-nouvelle-fenetre"),
        "win.nouvelle-fenetre",
        Some(Raccourci::NouvelleFenetre),
    ));
    m.append_item(&item(
        &tr("menu-connexions"),
        "win.connexions",
        Some(Raccourci::Connexions),
    ));
    let edition = gio::Menu::new();
    edition.append_item(&item(
        &tr("menu-copier"),
        "win.copier",
        Some(Raccourci::Copier),
    ));
    edition.append_item(&item(
        &tr("menu-coller"),
        "win.coller",
        Some(Raccourci::Coller),
    ));
    edition.append_item(&item(
        &tr("menu-rechercher"),
        "win.rechercher",
        Some(Raccourci::Rechercher),
    ));
    m.append_section(None, &edition);
    let affichage = gio::Menu::new();
    affichage.append_item(&item(
        &tr("menu-agrandir"),
        "win.zoom-plus",
        Some(Raccourci::ZoomPlus),
    ));
    affichage.append_item(&item(
        &tr("menu-reduire"),
        "win.zoom-moins",
        Some(Raccourci::ZoomMoins),
    ));
    affichage.append_item(&item(
        &tr("menu-taille-normale"),
        "win.zoom-normal",
        Some(Raccourci::ZoomNormal),
    ));
    affichage.append_item(&item(
        &tr("menu-plein-ecran"),
        "win.plein-ecran",
        Some(Raccourci::PleinEcran),
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
        Some(Raccourci::FermerOnglet),
    ));
    m.append_section(None, &onglet);
    let preferences = gio::Menu::new();
    let prefs = gio::Menu::new();
    prefs.append_item(&item(
        &tr("menu-reglages"),
        "win.reglages",
        Some(Raccourci::Reglages),
    ));
    for (libelle, action) in [
        ("menu-modifier-config", "win.prefs-configuration"),
        ("menu-modifier-invite", "win.prefs-invite"),
        ("menu-modifier-connexions", "win.prefs-connexions"),
    ] {
        prefs.append_item(&item(&tr(libelle), action, None));
    }
    preferences.append_submenu(Some(&tr("menu-preferences")), &prefs);
    m.append_section(None, &preferences);
    let aide = gio::Menu::new();
    aide.append_item(&item(
        &tr("menu-aide-pupitre"),
        "win.aide",
        Some(Raccourci::Aide),
    ));
    aide.append_item(&item(&tr("menu-a-propos"), "win.a-propos", None));
    m.append_section(None, &aide);
    m
}

/// Infobulle du bouton « + » : le raccourci de `nouvel_onglet` en vigueur, s'il y en a un.
fn infobulle_nouvel_onglet(table: &TableRaccourcis) -> String {
    match table.combinaison(Raccourci::NouvelOnglet) {
        Some(c) => ta(
            "bouton-nouvel-onglet-aide",
            &[("raccourci", ArgI18n::from(c.texte()))],
        ),
        None => tr("bouton-nouvel-onglet"),
    }
}

/// Bouton « + » de la barre d'onglets, à gauche du menu : active `win.nouvel-onglet`.
fn bouton_nouvel_onglet(table: &TableRaccourcis) -> gtk::Button {
    let b = gtk::Button::builder()
        .icon_name("list-add-symbolic")
        .action_name("win.nouvel-onglet")
        .tooltip_text(infobulle_nouvel_onglet(table))
        .build();
    b.add_css_class("flat");
    b.update_property(&[gtk::accessible::Property::Label(&tr(
        "bouton-nouvel-onglet",
    ))]);
    b
}

/// Bouton du menu principal, à droite de la barre d'onglets.
fn bouton_menu(table: &TableRaccourcis) -> (gtk::MenuButton, gio::Menu) {
    let sous_menu_shells = gio::Menu::new();
    let m = construire_menu(table, &sous_menu_shells);
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

/// Nom d'onglet tel que lu par le lecteur d'écran (« titre », « titre, cloche », « titre,
/// commande réussie »…) : celui posé par `Onglet::rafraichir_titre` sur l'étiquette.
fn nom_onglet(c: &gtk::Notebook, page: &gtk::Widget) -> Option<String> {
    let etiquette = etiquette_onglet(c, page)?;
    let nom = etiquette.widget_name();
    Some(if nom.is_empty() || nom == "GtkLabel" {
        etiquette.text().to_string()
    } else {
        nom.to_string()
    })
}

/// Étiquette de la tête d'un onglet.
fn etiquette_onglet(c: &gtk::Notebook, page: &gtk::Widget) -> Option<gtk::Label> {
    let tete = c.tab_label(page)?.downcast::<gtk::Box>().ok()?;
    tete.first_child()?.downcast::<gtk::Label>().ok()
}

/// Titre d'un onglet et indicateur qu'il porte, lus sur son étiquette.
fn titre_onglet(c: &gtk::Notebook, page: &gtk::Widget) -> Option<(String, Indicateur)> {
    let texte = etiquette_onglet(c, page)?.text();
    let (indicateur, titre) = Indicateur::separer(&texte);
    Some((titre.to_string(), indicateur))
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
/// des autres onglets qui signalent quelque chose (cloche, commande finie, notification). Orca la lit quand Ctrl+Tab arrive sur la
/// barre d'onglets, et la relit quand l'onglet change (son nom, lui, n'est pas relu
/// quand le focus remonte du terminal vers le carnet).
fn decrire_onglets(c: &gtk::Notebook) {
    let n = c.n_pages();
    let courant = c.current_page();
    let mut texte = String::new();
    let mut signales = Vec::new();
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
        } else if titre_onglet(c, &page).is_some_and(|(_, i)| i != Indicateur::Aucun) {
            signales.push(ta(
                "onglets-autre-signale",
                &[("nom", ArgI18n::from(nom)), ("i", numero)],
            ));
        }
    }
    if !signales.is_empty() {
        texte = ta(
            "onglets-signale-dans",
            &[
                ("texte", ArgI18n::from(texte)),
                ("liste", ArgI18n::from(signales.join(", "))),
            ],
        );
    }
    c.update_property(&[gtk::accessible::Property::Description(&texte)]);
}
