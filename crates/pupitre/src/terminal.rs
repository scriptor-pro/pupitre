// SPDX-License-Identifier: GPL-3.0-or-later
//! Un terminal VTE relié à une session du processus de fond : la sortie de la session
//! arrive par `feed()`, les frappes (signal `commit`) et la taille de la grille repartent
//! vers le processus de fond.

use crate::attenuation::{attenuation_lisible, FiltreAttenue};
use crate::client::Client;
use gtk::gdk;
use gtk::glib;
use gtk::pango;
use gtk::prelude::*;
use gtk::subclass::prelude::ObjectSubclassIsExt;
use pupitre_config::{Config, Rgb, Theme};
use pupitre_proto::{ClientMsg, SessionId};
use std::cell::RefCell;
use std::rc::Rc;
use vte4::prelude::*;

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

    pub struct CadreTerminal {
        pub terminal: vte4::Terminal,
        pub grille: Cell<(i64, i64)>,
        pub sur_taille: RefCell<Option<SurTaille>>,
    }

    impl Default for CadreTerminal {
        fn default() -> Self {
            CadreTerminal {
                terminal: vte4::Terminal::new(),
                grille: Cell::new((0, 0)),
                sur_taille: RefCell::new(None),
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
            t.allocate(largeur, hauteur, ligne_de_base, None);
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

    /// `f(colonnes, lignes)` est appelé à chaque nouvelle grille (y compris la première).
    pub fn connecter_taille(&self, f: impl Fn(u16, u16) + 'static) {
        *self.imp().sur_taille.borrow_mut() = Some(Box::new(f));
    }
}

/// Un onglet de terminal : un `CadreTerminal` relié à la session `id`.
pub struct TerminalOnglet {
    cadre: CadreTerminal,
    id: SessionId,
    attenuation: RefCell<FiltreAttenue>,
}

impl TerminalOnglet {
    pub fn new(client: Rc<Client>, id: SessionId, config: &Config, theme: &Theme) -> Self {
        let cadre = CadreTerminal::default();
        let terminal = cadre.terminal().clone();

        // Frappes, collages, réponses aux requêtes et souris → session. Une erreur d'envoi
        // signifie connexion perdue : elle est déjà signalée sur le canal du client.
        let c = client.clone();
        terminal.connect_commit(move |_, texte, _| {
            let _ = c.send(ClientMsg::Input {
                id,
                data: texte.as_bytes().to_vec(),
            });
        });
        cadre.connecter_taille(move |cols, rows| {
            let _ = client.send(ClientMsg::Resize { id, cols, rows });
        });

        // Ctrl+Maj+V : VTE ne fournit pas de raccourci de collage lui-même.
        let touches = gtk::EventControllerKey::new();
        touches.set_propagation_phase(gtk::PropagationPhase::Capture);
        let t = terminal.clone();
        touches.connect_key_pressed(move |_, touche, _, mods| {
            let ctrl_maj = gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::SHIFT_MASK;
            if mods.contains(ctrl_maj) && matches!(touche, gdk::Key::V | gdk::Key::v) {
                t.paste_clipboard();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        terminal.add_controller(touches);

        let onglet = TerminalOnglet {
            cadre,
            id,
            attenuation: RefCell::new(FiltreAttenue::default()),
        };
        onglet.appliquer(config, theme);
        onglet
    }

    pub fn id(&self) -> SessionId {
        self.id
    }

    pub fn widget(&self) -> &CadreTerminal {
        &self.cadre
    }

    pub fn terminal(&self) -> &vte4::Terminal {
        self.cadre.terminal()
    }

    /// Affiche des octets reçus de la session (y compris invalides : VTE les remplace).
    pub fn on_output(&self, data: &[u8]) {
        let mut filtree = Vec::with_capacity(data.len());
        self.attenuation.borrow_mut().filtrer(data, &mut filtree);
        self.terminal().feed(&filtree);
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
        t.set_font(Some(&pango::FontDescription::from_string(&config.police)));
        t.set_cursor_blink_mode(if config.curseur_clignote {
            vte4::CursorBlinkMode::On
        } else {
            vte4::CursorBlinkMode::Off
        });
        t.set_scrollback_lines(i64::from(config.historique_lignes));
        t.set_allow_hyperlink(true);
        t.set_audible_bell(false);
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
