// SPDX-License-Identifier: GPL-3.0-or-later
//! Un terminal VTE relié à une session du processus de fond : la sortie de la session
//! arrive par `feed()`, les frappes (signal `commit`) et la taille de la grille repartent
//! vers le processus de fond.

use crate::attenuation::{attenuation_lisible, FiltreAttenue};
use crate::client::Client;
use crate::commandes::{Flux, Navigation, Sens};
use gtk::gdk;
use gtk::glib;
use gtk::pango;
use gtk::prelude::*;
use gtk::subclass::prelude::ObjectSubclassIsExt;
use pupitre_config::{Config, Rgb, Theme};
use pupitre_proto::{segments_de_rejeu, ClientMsg, MarqueCommande, SessionId};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};
use vte4::prelude::*;

/// Propriété de terminal portant le titre envoyé par le programme.
const TITRE: &str = "xterm.title";

/// Zoom : de 50 % au maximum de la configuration, par pas de 10 %.
const ZOOM_MIN: u16 = 50;
const ZOOM_PAS: u16 = 10;
/// Cloche : une inversion de 150 ms, au plus une toutes les 350 ms (≤ 3 flashs/s).
const DUREE_FLASH: Duration = Duration::from_millis(150);
const ECART_FLASHS: Duration = Duration::from_millis(350);
const CLASSE_FLASH: &str = "pupitre-flash";

/// Adresses reconnues dans le texte (ouvertes au Ctrl+clic). Ponctuation finale exclue.
const REGEX_URL: &str = r#"\b(?:https?|file)://[^\s<>"'`]*[^\s<>"'`.,;:!?)\]}]"#;
/// Options PCRE2 utilisées par VTE : UTF, MULTILINE, CASELESS.
pub const PCRE2_UTF: u32 = 0x0008_0000;
pub const PCRE2_MULTILINE: u32 = 0x0000_0400;
pub const PCRE2_CASELESS: u32 = 0x0000_0008;

/// Rappel de la cloche (posé par la fenêtre).
type RappelCloche = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

/// Ouvre une adresse (remplaçable dans les tests).
pub type Lanceur = Rc<dyn Fn(&str)>;

/// Vrai si le bureau autorise les animations (« réduire les animations » désactivé).
pub fn animations_actives() -> bool {
    gtk::Settings::default().is_none_or(|r| r.is_gtk_enable_animations())
}

/// Style du flash de la cloche, installé une fois pour l'affichage.
fn installer_style() {
    thread_local! {
        static FAIT: Cell<bool> = const { Cell::new(false) };
    }
    if FAIT.with(|f| f.replace(true)) {
        return;
    }
    if let Some(affichage) = gdk::Display::default() {
        let css = gtk::CssProvider::new();
        css.load_from_string(&format!(".{CLASSE_FLASH} {{ filter: invert(100%); }}"));
        gtk::style_context_add_provider_for_display(
            &affichage,
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

// Conteneur autour de `vte4::Terminal`. Deux raisons (cf. Task 0, essai-jetable/feed) :
// VTE n'a pas de propriété « column-count » à surveiller, et dériver `vte4::Terminal`
// fait planter vte4-rs 0.9/0.10 (double libération dans le trampoline de `commit`).
// Le conteneur alloue son enfant puis compare la grille obtenue à la précédente.
mod imp {
    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;
    use std::cell::{Cell, RefCell};
    use vte4::prelude::*;

    /// Rappel prévenu de la nouvelle grille `(colonnes, lignes)`.
    pub type SurTaille = Box<dyn Fn(u16, u16)>;
    /// Rappel prévenu juste avant ou après un changement de largeur.
    pub type SurLargeur = Box<dyn Fn()>;

    pub struct CadreTerminal {
        pub terminal: vte4::Terminal,
        pub grille: Cell<(i64, i64)>,
        pub sur_taille: RefCell<Option<SurTaille>>,
        pub largeur: Cell<i32>,
        /// Police ou zoom changé : la prochaine allocation peut changer le nombre de colonnes.
        pub probable: Cell<bool>,
        /// Avant et après l'allocation d'une nouvelle largeur (VTE réagence ses lignes).
        pub autour_largeur: RefCell<Option<(SurLargeur, SurLargeur, SurLargeur)>>,
    }

    impl Default for CadreTerminal {
        fn default() -> Self {
            CadreTerminal {
                terminal: vte4::Terminal::new(),
                grille: Cell::new((0, 0)),
                sur_taille: RefCell::new(None),
                largeur: Cell::new(0),
                probable: Cell::new(false),
                autour_largeur: RefCell::new(None),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CadreTerminal {
        const NAME: &'static str = "PupitreCadreTerminal";
        type Type = super::CadreTerminal;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for CadreTerminal {
        fn constructed(&self) {
            self.parent_constructed();
            self.terminal.set_hexpand(true);
            self.terminal.set_vexpand(true);
            self.terminal.set_parent(&*self.obj());
        }

        fn dispose(&self) {
            self.terminal.unparent();
        }
    }

    impl WidgetImpl for CadreTerminal {
        fn measure(&self, o: gtk::Orientation, pour: i32) -> (i32, i32, i32, i32) {
            self.terminal.measure(o, pour)
        }

        fn size_allocate(&self, largeur: i32, hauteur: i32, ligne_de_base: i32) {
            let t = &self.terminal;
            let largeur_changee = largeur != self.largeur.replace(largeur);
            let probable = self.probable.replace(false);
            // Le nombre de colonnes que VTE va donner à cette largeur : sans changement,
            // rien n'est converti (un pixel de plus en tirant le bord ne coûte rien).
            let avant_cols = t.column_count();
            let change =
                probable || (largeur_changee && self.colonnes_predites(largeur) != avant_cols);
            if change {
                if let Some((avant, _, _)) = self.autour_largeur.borrow().as_ref() {
                    avant();
                }
            }
            t.allocate(largeur, hauteur, ligne_de_base, None);
            if change {
                if let Some((_, apres, _)) = self.autour_largeur.borrow().as_ref() {
                    apres();
                }
            } else if t.column_count() != avant_cols {
                // Prévision fausse : les lignes ont été réagencées sans être suivies.
                if let Some((_, _, rate)) = self.autour_largeur.borrow().as_ref() {
                    rate();
                }
            }
            let g = (t.column_count(), t.row_count());
            if g.0 > 0 && g.1 > 0 && g != self.grille.replace(g) {
                if let Some(f) = self.sur_taille.borrow().as_ref() {
                    f(borne_u16(g.0), borne_u16(g.1));
                }
            }
        }

        fn grab_focus(&self) -> bool {
            self.terminal.grab_focus()
        }
    }

    impl CadreTerminal {
        /// Colonnes que VTE donnera à `largeur` pixels : la largeur moins les marges CSS,
        /// divisée par celle d'une cellule.
        #[allow(deprecated)]
        fn colonnes_predites(&self, largeur: i32) -> i64 {
            let t = &self.terminal;
            let cellule = t.char_width();
            if cellule <= 0 {
                return -1;
            }
            let marges = t.style_context().padding();
            let utile = i64::from(largeur) - i64::from(marges.left()) - i64::from(marges.right());
            (utile / cellule).max(1)
        }
    }

    fn borne_u16(v: i64) -> u16 {
        u16::try_from(v.max(1)).unwrap_or(u16::MAX)
    }
}

glib::wrapper! {
    /// Conteneur d'un `vte4::Terminal` ; prévient de chaque changement de grille.
    pub struct CadreTerminal(ObjectSubclass<imp::CadreTerminal>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for CadreTerminal {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl CadreTerminal {
    pub fn terminal(&self) -> &vte4::Terminal {
        &self.imp().terminal
    }

    /// La police ou le zoom change : la prochaine allocation sera encadrée même si sa largeur
    /// en pixels est la même (le nombre de colonnes change).
    pub fn reagencement_probable(&self) {
        self.imp().probable.set(true);
    }

    /// `avant()` puis `apres()` encadrent chaque allocation d'une largeur nouvelle.
    /// `rate()` signale un réagencement que la prévision n'avait pas annoncé.
    pub fn autour_largeur(
        &self,
        avant: impl Fn() + 'static,
        apres: impl Fn() + 'static,
        rate: impl Fn() + 'static,
    ) {
        *self.imp().autour_largeur.borrow_mut() =
            Some((Box::new(avant), Box::new(apres), Box::new(rate)));
    }

    /// `f(colonnes, lignes)` est appelé à chaque nouvelle grille (y compris la première).
    pub fn connecter_taille(&self, f: impl Fn(u16, u16) + 'static) {
        *self.imp().sur_taille.borrow_mut() = Some(Box::new(f));
    }
}

/// Un onglet de terminal : un `CadreTerminal` relié à la session `id`.
pub struct TerminalOnglet {
    cadre: CadreTerminal,
    client: Rc<Client>,
    id: Rc<Cell<SessionId>>,
    /// Session terminée : frappes et tailles ne sont plus envoyées.
    muet: Rc<Cell<bool>>,
    attenuation: Rc<RefCell<FiltreAttenue>>,
    /// Octets et marques de commandes dans l'ordre, et lignes des commandes.
    flux: Rc<Flux>,
    zoom: Cell<u16>,
    /// Police appliquée en dernier (un changement réagence les lignes).
    police: RefCell<String>,
    /// Clignotement demandé par la configuration (appliqué si les animations sont permises).
    clignote: Rc<Cell<bool>>,
    lanceur: Rc<RefCell<Lanceur>>,
    flashs: Rc<Cell<u32>>,
    sur_cloche: RappelCloche,
    reglages: Option<(gtk::Settings, glib::SignalHandlerId)>,
}

impl Drop for TerminalOnglet {
    fn drop(&mut self) {
        if let Some((r, id)) = self.reglages.take() {
            r.disconnect(id);
        }
    }
}

impl TerminalOnglet {
    pub fn new(client: Rc<Client>, id: SessionId, config: &Config, theme: &Theme) -> Self {
        let cadre = CadreTerminal::default();
        let terminal = cadre.terminal().clone();
        let id = Rc::new(Cell::new(id));
        let muet = Rc::new(Cell::new(false));

        // Frappes, collages, réponses aux requêtes et souris → session. Une erreur d'envoi
        // signifie connexion perdue : elle est déjà signalée sur le canal du client.
        let attenuation = Rc::new(RefCell::new(FiltreAttenue::default()));
        let flux = {
            let (a, t) = (attenuation.clone(), terminal.clone());
            Flux::new(
                terminal.clone(),
                Box::new(move |data: &[u8]| {
                    let mut filtree = Vec::with_capacity(data.len());
                    a.borrow_mut().filtrer(data, &mut filtree);
                    t.feed(&filtree);
                }),
            )
        };
        let (c, i, m, fl) = (client.clone(), id.clone(), muet.clone(), flux.clone());
        terminal.connect_commit(move |_, texte, _| {
            // Réponse à notre propre demande de position du curseur : pas pour le shell.
            if fl.garder_reponse(texte) {
                return;
            }
            if !m.get() {
                let _ = c.send(ClientMsg::Input {
                    id: i.get(),
                    data: texte.as_bytes().to_vec(),
                });
            }
        });
        let (c, i, m) = (client.clone(), id.clone(), muet.clone());
        cadre.connecter_taille(move |cols, rows| {
            if !m.get() {
                let _ = c.send(ClientMsg::Resize {
                    id: i.get(),
                    cols,
                    rows,
                });
            }
        });

        // Largeur changée (zoom, fenêtre, plein écran) : VTE réagence ses lignes ; les
        // lignes des commandes sont suivies à travers ce réagencement.
        let (avant, apres, rate) = (
            Rc::downgrade(&flux),
            Rc::downgrade(&flux),
            Rc::downgrade(&flux),
        );
        cadre.autour_largeur(
            move || {
                if let Some(f) = avant.upgrade() {
                    f.avant_reagencement();
                }
            },
            move || {
                if let Some(f) = apres.upgrade() {
                    f.apres_reagencement();
                }
            },
            move || {
                if let Some(f) = rate.upgrade() {
                    f.reagencement_rate();
                }
            },
        );

        // Adresses en texte brut : reconnues, ouvertes seulement au Ctrl+clic.
        match vte4::Regex::for_match(REGEX_URL, PCRE2_UTF | PCRE2_MULTILINE | PCRE2_CASELESS) {
            Ok(re) => {
                let tag = terminal.match_add_regex(&re, 0);
                terminal.match_set_cursor_name(tag, "pointer");
            }
            Err(e) => eprintln!("pupitre: expression des adresses refusée : {e}"),
        }
        let defaut: Lanceur = {
            let t = terminal.clone();
            Rc::new(move |uri: &str| {
                let parent = t.root().and_downcast::<gtk::Window>();
                gtk::UriLauncher::new(uri).launch(
                    parent.as_ref(),
                    gtk::gio::Cancellable::NONE,
                    |r| {
                        if let Err(e) = r {
                            eprintln!("pupitre: ouverture du lien impossible : {e}");
                        }
                    },
                );
            })
        };
        let lanceur = Rc::new(RefCell::new(defaut));
        let clic = gtk::GestureClick::new();
        clic.set_button(1);
        clic.set_propagation_phase(gtk::PropagationPhase::Capture);
        let (t, l) = (terminal.clone(), lanceur.clone());
        clic.connect_pressed(move |g, _, x, y| {
            if ouvrir_lien(&t, &l, x, y, g.current_event_state()) {
                g.set_state(gtk::EventSequenceState::Claimed);
            }
        });
        terminal.add_controller(clic);

        // Cloche : flash limité (sauf animations réduites), puis rappel de la fenêtre.
        installer_style();
        let flashs = Rc::new(Cell::new(0u32));
        let sur_cloche: RappelCloche = Rc::new(RefCell::new(None));
        let dernier: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));
        let (c, f, sc) = (cadre.clone(), flashs.clone(), sur_cloche.clone());
        terminal.connect_bell(move |_| {
            let maintenant = Instant::now();
            let assez_loin = dernier
                .get()
                .is_none_or(|d| maintenant.duration_since(d) >= ECART_FLASHS);
            if animations_actives() && assez_loin {
                dernier.set(Some(maintenant));
                f.set(f.get() + 1);
                c.add_css_class(CLASSE_FLASH);
                let c = c.clone();
                glib::timeout_add_local_once(DUREE_FLASH, move || c.remove_css_class(CLASSE_FLASH));
            }
            let rappel = sc.borrow().clone();
            if let Some(r) = rappel {
                r();
            }
        });

        // « Réduire les animations » du bureau : curseur fixe, suivi en direct.
        let clignote = Rc::new(Cell::new(false));
        let reglages = gtk::Settings::default().map(|r| {
            let (t, cl) = (terminal.downgrade(), clignote.clone());
            let id = r.connect_notify_local(Some("gtk-enable-animations"), move |_, _| {
                if let Some(t) = t.upgrade() {
                    t.set_cursor_blink_mode(mode_curseur(cl.get()));
                }
            });
            (r, id)
        });

        let onglet = TerminalOnglet {
            cadre,
            client,
            id,
            muet,
            attenuation,
            flux,
            zoom: Cell::new(100),
            police: RefCell::new(String::new()),
            clignote,
            lanceur,
            flashs,
            sur_cloche,
            reglages,
        };
        onglet.appliquer(config, theme);
        onglet
    }

    /// Zoom en pour cent (100 = taille de la configuration).
    pub fn zoom(&self) -> u16 {
        self.zoom.get()
    }

    /// Change le zoom de `pas` crans de 10 % (borné de 50 % à `maximum`) ; `None` : 100 %.
    pub fn zoomer(&self, pas: Option<i32>, maximum: u16) {
        let z = match pas {
            None => 100,
            Some(p) => {
                let v = i32::from(self.zoom.get()) + p * i32::from(ZOOM_PAS);
                let v = v.clamp(i32::from(ZOOM_MIN), i32::from(maximum.max(ZOOM_MIN)));
                u16::try_from(v).unwrap_or(100)
            }
        };
        if z != self.zoom.replace(z) {
            self.cadre.reagencement_probable();
        }
        self.terminal().set_font_scale(f64::from(z) / 100.0);
    }

    pub fn copier(&self) {
        self.terminal().copy_clipboard_format(vte4::Format::Text);
    }

    pub fn coller(&self) {
        self.terminal().paste_clipboard();
    }

    /// Remplace l'ouverture des liens (tests).
    pub fn definir_lanceur(&self, l: Lanceur) {
        *self.lanceur.borrow_mut() = l;
    }

    /// Clic à `(x, y)` : ouvre le lien qui s'y trouve si Ctrl est enfoncé. Vrai si ouvert.
    pub fn clic(&self, x: f64, y: f64, mods: gdk::ModifierType) -> bool {
        ouvrir_lien(self.terminal(), &self.lanceur, x, y, mods)
    }

    /// Nombre de flashs de cloche affichés depuis la création.
    pub fn nombre_flashs(&self) -> u32 {
        self.flashs.get()
    }

    /// `f` est appelé à chaque cloche (après l'éventuel flash).
    pub fn connecter_cloche(&self, f: impl Fn() + 'static) {
        *self.sur_cloche.borrow_mut() = Some(Rc::new(f));
    }

    pub fn id(&self) -> SessionId {
        self.id.get()
    }

    /// Relie l'onglet à une autre session (shell rouvert dans le même onglet).
    pub fn relier(&self, id: SessionId) {
        self.id.set(id);
        self.muet.set(false);
    }

    /// Coupe (ou rétablit) l'envoi des frappes et des tailles vers la session.
    pub fn rendre_muet(&self, muet: bool) {
        self.muet.set(muet);
    }

    pub fn est_muet(&self) -> bool {
        self.muet.get()
    }

    pub fn widget(&self) -> &CadreTerminal {
        &self.cadre
    }

    pub fn terminal(&self) -> &vte4::Terminal {
        self.cadre.terminal()
    }

    /// Grille actuelle `(colonnes, lignes)`, ou `None` avant la première allocation.
    pub fn grille(&self) -> Option<(u16, u16)> {
        let t = self.terminal();
        let (c, r) = (t.column_count(), t.row_count());
        (t.is_mapped() && c > 0 && r > 0).then(|| {
            (
                u16::try_from(c).unwrap_or(u16::MAX),
                u16::try_from(r).unwrap_or(u16::MAX),
            )
        })
    }

    /// Provoque un SIGWINCH dans la session (deux tailles successives) pour que les
    /// programmes plein écran se redessinent après une reprise.
    pub fn forcer_redessin(&self) {
        let Some((cols, rows)) = self.grille() else {
            return;
        };
        if self.muet.get() {
            return;
        }
        let id = self.id.get();
        let autre = if rows > 1 { rows - 1 } else { rows + 1 };
        let _ = self.client.send(ClientMsg::Resize {
            id,
            cols,
            rows: autre,
        });
        let _ = self.client.send(ClientMsg::Resize { id, cols, rows });
    }

    /// Titre envoyé par le programme (OSC 0/2), s'il n'est pas vide.
    pub fn titre_programme(&self) -> Option<String> {
        let (titre, _) = self.terminal().termprop_string(TITRE);
        let titre = titre?;
        let titre = titre.trim();
        (!titre.is_empty()).then(|| titre.to_string())
    }

    /// Titre envoyé par le programme (OSC 0/2), sinon dernier élément du dossier courant
    /// (OSC 7), sinon `None`.
    pub fn titre(&self) -> Option<String> {
        if let Some(titre) = self.titre_programme() {
            return Some(titre);
        }
        let t = self.terminal();
        // Accesseur marqué obsolète depuis VTE 0.78 au profit des termprops de type URI,
        // non exposées par vte4-rs ; il reste fonctionnel.
        #[allow(deprecated)]
        let uri = t.current_directory_uri()?;
        let chemin = gtk::gio::File::for_uri(&uri).path()?;
        let nom = chemin
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| chemin.to_string_lossy().into_owned());
        (!nom.is_empty()).then_some(nom)
    }

    /// Affiche des octets reçus de la session (y compris invalides : VTE les remplace).
    pub fn on_output(&self, data: &[u8]) {
        self.flux.octets(data);
    }

    /// Marque de commande (OSC 133) reçue du shell, à sa place dans le flux ; `rejouee` :
    /// vue lors d'une reprise (sert à la navigation, jamais annoncée).
    pub fn marque(&self, marque: MarqueCommande, rejouee: bool) {
        self.flux.marque(marque, rejouee);
    }

    /// Rejeu d'une reprise : les octets avec leurs marques (décalages dans `data`).
    pub fn rejouer(&self, data: &[u8], marques: &[(u32, MarqueCommande)]) {
        for (marque, octets) in segments_de_rejeu(data, marques) {
            if let Some(m) = marque {
                self.flux.marque(m, true);
            }
            self.flux.octets(octets);
        }
    }

    /// Change le délai de secours des attentes de VTE (tests).
    #[doc(hidden)]
    pub fn definir_delai_attente(&self, delai: Duration) {
        self.flux.definir_delai_attente(delai);
    }

    /// Réponses de position du curseur gardées pour soi depuis la création (tests).
    #[doc(hidden)]
    pub fn reponses_gardees(&self) -> u32 {
        self.flux.reponses_gardees()
    }

    /// Temps passé à convertir les positions, nombre de conversions et de réagencements non
    /// suivis (tests).
    #[doc(hidden)]
    pub fn conversions(&self) -> (Duration, u32, u32) {
        self.flux.conversions()
    }

    /// Change la borne de la file d'octets en attente (tests).
    #[doc(hidden)]
    pub fn definir_file_max(&self, octets: usize) {
        self.flux.definir_file_max(octets);
    }

    /// Nombre de commandes repérées dans le flux.
    pub fn nombre_commandes(&self) -> usize {
        self.flux.nombre_commandes()
    }

    /// `f(code, durée en ms)` est appelé à la fin de chaque commande vue en direct.
    pub fn connecter_fin_commande(&self, f: impl Fn(Option<i32>, Option<u64>) + 'static) {
        self.flux.connecter_fin(f);
    }

    /// Défile jusqu'à la commande précédente ou suivante ; `fini` reçoit le résultat.
    pub fn naviguer(&self, sens: Sens, fini: impl FnOnce(Option<Navigation>) + 'static) {
        self.flux.naviguer(sens, fini);
    }

    /// Applique police, couleurs et comportement ; peut être rappelé à tout moment.
    pub fn appliquer(&self, config: &Config, theme: &Theme) {
        let t = self.terminal();
        // Texte atténué illisible avec ce thème : affiché dans sa couleur normale.
        self.attenuation
            .borrow_mut()
            .activer(!attenuation_lisible(theme));
        let palette: Vec<gdk::RGBA> = theme.palette.iter().map(|&c| rgba(c)).collect();
        let refs: Vec<&gdk::RGBA> = palette.iter().collect();
        t.set_colors(Some(&rgba(theme.texte)), Some(&rgba(theme.fond)), &refs);
        t.set_color_bold(None);
        t.set_color_cursor(Some(&rgba(theme.curseur)));
        t.set_color_cursor_foreground(Some(&rgba(theme.fond)));
        t.set_color_highlight(Some(&rgba(theme.selection_fond)));
        t.set_color_highlight_foreground(Some(&rgba(theme.selection_texte)));
        if *self.police.borrow() != config.police {
            self.police.replace(config.police.clone());
            self.cadre.reagencement_probable();
        }
        t.set_font(Some(&pango::FontDescription::from_string(&config.police)));
        self.clignote.set(config.curseur_clignote);
        t.set_cursor_blink_mode(mode_curseur(config.curseur_clignote));
        t.set_scrollback_lines(i64::from(config.historique_lignes));
        // Autant de commandes que de lignes d'historique au plus (une ligne chacune).
        self.flux.definir_capacite(
            usize::try_from(config.historique_lignes).map_or(10_000, |n| n.clamp(256, 100_000)),
        );
        t.set_allow_hyperlink(true);
        t.set_audible_bell(false);
    }
}

fn mode_curseur(clignote: bool) -> vte4::CursorBlinkMode {
    if clignote && animations_actives() {
        vte4::CursorBlinkMode::On
    } else {
        vte4::CursorBlinkMode::Off
    }
}

/// Ouvre le lien (OSC 8 ou adresse reconnue) sous `(x, y)`, seulement avec Ctrl.
fn ouvrir_lien(
    t: &vte4::Terminal,
    lanceur: &RefCell<Lanceur>,
    x: f64,
    y: f64,
    mods: gdk::ModifierType,
) -> bool {
    if !mods.contains(gdk::ModifierType::CONTROL_MASK) {
        return false;
    }
    let uri = t
        .check_hyperlink_at(x, y)
        .or_else(|| t.check_match_at(x, y).0);
    match uri {
        Some(u) if !u.is_empty() => {
            let l = lanceur.borrow().clone();
            l(u.as_str());
            true
        }
        _ => false,
    }
}

fn rgba(c: Rgb) -> gdk::RGBA {
    gdk::RGBA::new(
        f32::from(c.r) / 255.0,
        f32::from(c.g) / 255.0,
        f32::from(c.b) / 255.0,
        1.0,
    )
}
