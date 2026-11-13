// SPDX-License-Identifier: GPL-3.0-or-later
//! Raccourcis clavier de Pupitre (conception §12.1).
//!
//! Reconnus d'abord par le caractère produit (Ctrl+Maj+T là où se trouve « T »), avec repli
//! sur la position physique pour les chiffres (et les lettres des dispositions non latines) :
//! en fr dvorak par exemple, la rangée du haut donne « = / - è \ ^ ( … » sans Maj ; Alt+« 2 »
//! arrive donc en Alt+« / ». Les raccourcis par défaut sont ceux de l'aide ; la section
//! `[raccourcis]` de la configuration en change certains (`TableRaccourcis`).

use gtk::gdk;
use pupitre_config::ReglageRaccourci;
use pupitre_i18n::{t, ta, ArgI18n};

/// Codes matériels (evdev + 8, identiques sous X11 et Wayland) des touches 1 à 0 de la
/// rangée du haut.
const CODE_TOUCHE_1: u32 = 10;
const CODE_TOUCHE_9: u32 = 18;
const CODE_TOUCHE_0: u32 = 19;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Raccourci {
    NouvelOnglet,
    FermerOnglet,
    NouvelleFenetre,
    /// Ctrl+Maj+O : fenêtre des connexions SSH.
    Connexions,
    Copier,
    Coller,
    Rechercher,
    OngletSuivant,
    OngletPrecedent,
    /// Onglet numéro 1 à 9.
    Onglet(u32),
    PleinEcran,
    ZoomPlus,
    ZoomMoins,
    ZoomNormal,
    /// Ctrl+Tab : quitter le terminal (qui garde Tab pour le shell) vers l'élément suivant.
    FocusSuivant,
    /// Ctrl+Maj+Tab : élément précédent.
    FocusPrecedent,
    /// Ctrl+Maj+↑ : commande précédente (↑ seule reste au shell).
    CommandePrecedente,
    /// Ctrl+Maj+↓ : commande suivante.
    CommandeSuivante,
    /// Ctrl+Maj+H : l'aide de Pupitre (F1 reste aux programmes).
    Aide,
    /// Fenêtre de réglages (pas de raccourci par défaut).
    Reglages,
}

/// Une combinaison de touches : modificateurs (Ctrl, Maj, Alt, Super) et une touche, écrite
/// comme dans l'aide (« Ctrl+Maj+T », « F11 »). La touche est gardée sous sa forme normalisée
/// (minuscule, pavé numérique ramené à la touche ordinaire).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Combinaison {
    mods: gdk::ModifierType,
    touche: gdk::Key,
}

/// Raison pour laquelle un raccourci écrit dans `[raccourcis]` est refusé.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErreurRaccourci {
    ToucheManquante,
    ModificateurInconnu(String),
    ModificateurRepete(String),
    ToucheInconnue(String),
    /// Combinaison toujours disponible ou laissée aux programmes (texte de la combinaison).
    Reservee(String),
    SansModificateur,
    /// Ctrl seul, Alt seul ou Ctrl+Alt sur une lettre (ou un signe de contrôle avec Ctrl) :
    /// la frappe reste au shell et aux programmes (texte de la combinaison).
    LaisseeAuxProgrammes(String),
}

impl std::fmt::Display for ErreurRaccourci {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let avec = |id: &str, nom: &str| ta(id, &[("nom", ArgI18n::from(nom))]);
        f.write_str(&match self {
            ErreurRaccourci::ToucheManquante => t("raccourci-err-touche-manquante"),
            ErreurRaccourci::ModificateurInconnu(n) => {
                avec("raccourci-err-modificateur-inconnu", n)
            }
            ErreurRaccourci::ModificateurRepete(n) => avec("raccourci-err-modificateur-repete", n),
            ErreurRaccourci::ToucheInconnue(n) => avec("raccourci-err-touche-inconnue", n),
            ErreurRaccourci::Reservee(c) => avec("raccourci-err-reservee", c),
            ErreurRaccourci::SansModificateur => t("raccourci-err-sans-modificateur"),
            ErreurRaccourci::LaisseeAuxProgrammes(c) => {
                avec("raccourci-err-laissee-aux-programmes", c)
            }
        })
    }
}

impl std::error::Error for ErreurRaccourci {}

/// Modificateurs qui comptent pour un raccourci (Verr. maj., pavé numérique… sont ignorés).
fn mods_utiles() -> gdk::ModifierType {
    use gdk::ModifierType as M;
    M::CONTROL_MASK | M::SHIFT_MASK | M::ALT_MASK | M::SUPER_MASK | M::META_MASK
}

/// Noms de touches de l'aide (en minuscules) et leur touche.
const NOMS_DE_TOUCHES: &[(&str, gdk::Key)] = &[
    ("plus", gdk::Key::plus),
    ("moins", gdk::Key::minus),
    ("page suivante", gdk::Key::Page_Down),
    ("page précédente", gdk::Key::Page_Up),
    ("↑", gdk::Key::Up),
    ("↓", gdk::Key::Down),
    ("←", gdk::Key::Left),
    ("→", gdk::Key::Right),
    ("entrée", gdk::Key::Return),
    ("espace", gdk::Key::space),
    ("retour arrière", gdk::Key::BackSpace),
    ("suppr", gdk::Key::Delete),
    ("inser", gdk::Key::Insert),
    ("début", gdk::Key::Home),
    ("fin", gdk::Key::End),
    ("échap", gdk::Key::Escape),
    ("tab", gdk::Key::Tab),
];

/// Ramène une touche à sa forme de comparaison : minuscule, pavé numérique et Maj+Tab
/// comme les touches ordinaires, « = » comme « + » (« + » demande Maj sur certaines
/// dispositions).
fn normaliser(touche: gdk::Key) -> gdk::Key {
    use gdk::Key as K;
    match touche {
        K::KP_Add | K::equal => K::plus,
        K::KP_Subtract => K::minus,
        K::KP_0 => K::_0,
        K::KP_1 => K::_1,
        K::KP_2 => K::_2,
        K::KP_3 => K::_3,
        K::KP_4 => K::_4,
        K::KP_5 => K::_5,
        K::KP_6 => K::_6,
        K::KP_7 => K::_7,
        K::KP_8 => K::_8,
        K::KP_9 => K::_9,
        K::KP_Up => K::Up,
        K::KP_Down => K::Down,
        K::KP_Left => K::Left,
        K::KP_Right => K::Right,
        K::KP_Page_Up => K::Page_Up,
        K::KP_Page_Down => K::Page_Down,
        K::KP_Home => K::Home,
        K::KP_End => K::End,
        K::KP_Insert => K::Insert,
        K::KP_Delete => K::Delete,
        K::KP_Enter => K::Return,
        K::ISO_Left_Tab => K::Tab,
        autre => autre.to_lower(),
    }
}

/// Touche qui produit ce caractère (touche « = » pour `=`, « é » pour `é`).
fn touche_du_caractere(c: char) -> Option<gdk::Key> {
    // Conversion sûre : une valeur `u32` rangée dans une `Value` se relit comme `Key`.
    let touche = gtk::glib::Value::from(gdk::unicode_to_keyval(c as u32))
        .get::<gdk::Key>()
        .ok()?;
    (touche != gdk::Key::VoidSymbol && touche.name().is_some()).then_some(touche)
}

/// Code matériel (disposition de référence : QWERTY, evdev + 8) d'une touche de la rangée des
/// chiffres ou d'une lettre.
fn code_de_reference(touche: gdk::Key) -> Option<u32> {
    let c = touche.to_unicode()?;
    match c.to_ascii_lowercase() {
        '1'..='9' => Some(CODE_TOUCHE_1 + c as u32 - '1' as u32),
        '0' => Some(CODE_TOUCHE_0),
        'q' => Some(24),
        'w' => Some(25),
        'e' => Some(26),
        'r' => Some(27),
        't' => Some(28),
        'y' => Some(29),
        'u' => Some(30),
        'i' => Some(31),
        'o' => Some(32),
        'p' => Some(33),
        'a' => Some(38),
        's' => Some(39),
        'd' => Some(40),
        'f' => Some(41),
        'g' => Some(42),
        'h' => Some(43),
        'j' => Some(44),
        'k' => Some(45),
        'l' => Some(46),
        'z' => Some(52),
        'x' => Some(53),
        'c' => Some(54),
        'v' => Some(55),
        'b' => Some(56),
        'n' => Some(57),
        'm' => Some(58),
        _ => None,
    }
}

fn est_chiffre(touche: gdk::Key) -> bool {
    touche.to_unicode().is_some_and(|c| c.is_ascii_digit())
}

/// Touche de fonction F1 à F24.
fn est_touche_de_fonction(touche: gdk::Key) -> bool {
    touche
        .name()
        .and_then(|n| n.strip_prefix('F').map(str::to_owned))
        .and_then(|n| n.parse::<u32>().ok())
        .is_some_and(|n| (1..=24).contains(&n))
}

impl Combinaison {
    pub fn mods(&self) -> gdk::ModifierType {
        self.mods
    }

    pub fn touche(&self) -> gdk::Key {
        self.touche
    }

    /// Écriture de l'aide : « Ctrl+Maj+T », « F11 », « Ctrl+Page suivante ».
    pub fn texte(&self) -> String {
        use gdk::ModifierType as M;
        let mut s = String::new();
        for (m, nom) in [
            (M::CONTROL_MASK, "Ctrl"),
            (M::SHIFT_MASK, "Maj"),
            (M::ALT_MASK, "Alt"),
            (M::SUPER_MASK, "Super"),
        ] {
            if self.mods.contains(m) {
                s.push_str(nom);
                s.push('+');
            }
        }
        match NOMS_DE_TOUCHES.iter().find(|(_, k)| *k == self.touche) {
            Some((nom, _)) => {
                let mut lettres = nom.chars();
                // « Page suivante », « Entrée » : première lettre en capitale ; « plus » reste
                // en minuscules comme dans l'aide.
                match (*nom, lettres.next()) {
                    ("plus" | "moins" | "↑" | "↓" | "←" | "→", _) | (_, None) => {
                        s.push_str(nom)
                    }
                    (_, Some(c)) => {
                        s.extend(c.to_uppercase());
                        s.push_str(lettres.as_str());
                    }
                }
            }
            None => match self.touche.to_unicode() {
                Some(c) if !c.is_control() => s.extend(c.to_uppercase()),
                _ => s.push_str(self.touche.name().as_deref().unwrap_or("?")),
            },
        }
        s
    }

    /// Notation des accélérateurs GTK (« <Control><Shift>t »), pour l'affichage du menu.
    pub fn accelerateur(&self) -> String {
        use gdk::ModifierType as M;
        let mut s = String::new();
        for (m, nom) in [
            (M::CONTROL_MASK, "<Control>"),
            (M::SHIFT_MASK, "<Shift>"),
            (M::ALT_MASK, "<Alt>"),
            (M::SUPER_MASK, "<Super>"),
        ] {
            if self.mods.contains(m) {
                s.push_str(nom);
            }
        }
        s.push_str(self.touche.name().as_deref().unwrap_or(""));
        s
    }

    /// Vrai si la touche frappée est celle de la combinaison : même touche, ou même position
    /// physique. Position : chiffres toujours (la rangée du haut change de contenu d'une
    /// disposition à l'autre : fr dvorak, azerty…) ; lettres quand la disposition n'en donne
    /// pas de latine (russe, grec…), la lettre produite faisant foi sinon.
    fn touche_correspond(&self, touche: gdk::Key, code: u32) -> bool {
        if normaliser(touche) == self.touche {
            return true;
        }
        let Some(position) = code_de_reference(self.touche) else {
            return false;
        };
        code == position
            && (est_chiffre(self.touche) || !touche.to_unicode().is_some_and(|c| c.is_ascii()))
    }

    /// Reconnaissance exacte, ou à Maj près pour « + », « - » et les chiffres (qui demandent
    /// Maj sur certaines dispositions : fr, fr dvorak).
    fn reconnait(&self, touche: gdk::Key, code: u32, m: gdk::ModifierType, large: bool) -> bool {
        use gdk::ModifierType as M;
        let mods_ok = if large {
            !self.mods.contains(M::SHIFT_MASK)
                && m == self.mods | M::SHIFT_MASK
                && (est_chiffre(self.touche)
                    || matches!(self.touche, gdk::Key::plus | gdk::Key::minus))
        } else {
            m == self.mods
        };
        mods_ok && self.touche_correspond(touche, code)
    }

    /// Réservée : toujours disponible (Ctrl+Tab, Alt+1…9, Ctrl+PgSuiv/PgPréc) ou laissée aux
    /// programmes (Tab, F1, F10).
    fn reservee(&self) -> bool {
        use gdk::ModifierType as M;
        matches!(self.touche, gdk::Key::Tab | gdk::Key::F1 | gdk::Key::F10)
            || (self.mods == M::CONTROL_MASK
                && matches!(self.touche, gdk::Key::Page_Down | gdk::Key::Page_Up))
            || (self.mods == M::ALT_MASK && est_chiffre(self.touche) && self.touche != gdk::Key::_0)
    }
}

/// Analyse la combinaison, sans refuser les réservées (pour les défauts).
fn analyser_libre(texte: &str) -> Result<Combinaison, ErreurRaccourci> {
    use gdk::ModifierType as M;
    let texte = texte.trim();
    let (modificateurs, nom) = if texte == "+" {
        ("", "plus")
    } else if let Some(p) = texte.strip_suffix("++") {
        (p, "plus")
    } else {
        match texte.rsplit_once('+') {
            Some((m, n)) => (m, n.trim()),
            None => ("", texte),
        }
    };
    let est_modificateur = |n: &str| {
        matches!(
            n.to_lowercase().as_str(),
            "ctrl" | "control" | "maj" | "shift" | "alt" | "super"
        )
    };
    if nom.is_empty() || est_modificateur(nom) {
        return Err(ErreurRaccourci::ToucheManquante);
    }
    let mut mods = M::empty();
    if !modificateurs.trim().is_empty() {
        for m in modificateurs.split('+').map(str::trim) {
            let masque = match m.to_lowercase().as_str() {
                "ctrl" | "control" => M::CONTROL_MASK,
                "maj" | "shift" => M::SHIFT_MASK,
                "alt" => M::ALT_MASK,
                "super" => M::SUPER_MASK,
                _ => return Err(ErreurRaccourci::ModificateurInconnu(m.to_owned())),
            };
            if mods.contains(masque) {
                return Err(ErreurRaccourci::ModificateurRepete(m.to_owned()));
            }
            mods |= masque;
        }
    }
    let inconnue = || ErreurRaccourci::ToucheInconnue(nom.to_owned());
    let minuscules = nom.to_lowercase();
    let touche = if let Some((_, k)) = NOMS_DE_TOUCHES.iter().find(|(n, _)| *n == minuscules) {
        *k
    } else {
        let mut c = nom.chars();
        match (c.next(), c.next()) {
            (Some(un), None) if !un.is_control() && !un.is_whitespace() => {
                touche_du_caractere(un).ok_or_else(inconnue)?
            }
            _ => gdk::Key::from_name(nom).ok_or_else(inconnue)?,
        }
    };
    // Les touches qui ne sont que des modificateurs ne forment pas une combinaison.
    let nom_gdk = touche.name().map(|n| n.to_string()).unwrap_or_default();
    if [
        "Shift_",
        "Control_",
        "Alt_",
        "Super_",
        "Meta_",
        "Hyper_",
        "ISO_Level",
        "Caps_",
        "Num_",
        "Scroll_",
    ]
    .iter()
    .any(|p| nom_gdk.starts_with(p))
    {
        return Err(inconnue());
    }
    Ok(Combinaison {
        mods,
        touche: normaliser(touche),
    })
}

/// Analyse une combinaison écrite dans `[raccourcis]` (« Ctrl+Maj+T », « maj+ctrl+t »,
/// « F11 ») et refuse celles qui ne peuvent pas être prises.
pub fn analyser_raccourci(texte: &str) -> Result<Combinaison, ErreurRaccourci> {
    let c = analyser_libre(texte)?;
    verifier(&c)?;
    Ok(c)
}

fn verifier(c: &Combinaison) -> Result<(), ErreurRaccourci> {
    use gdk::ModifierType as M;
    if c.reservee() {
        return Err(ErreurRaccourci::Reservee(c.texte()));
    }
    // Une lettre (ou Maj+lettre) volerait la frappe au shell ; seules les touches de
    // fonction se passent de Ctrl, Alt et Super.
    if !c
        .mods
        .intersects(M::CONTROL_MASK | M::ALT_MASK | M::SUPER_MASK)
        && !est_touche_de_fonction(c.touche)
    {
        return Err(ErreurRaccourci::SansModificateur);
    }
    // Ctrl+lettre (Ctrl+C, Ctrl+D, Ctrl+Z, Ctrl+L…), Alt+lettre (readline : Alt+B, Alt+F…)
    // et Ctrl+Alt+lettre restent au shell et aux programmes ; Ctrl seul garde aussi
    // `[ \ ] ^ _ @` et Espace (caractères de contrôle).
    let lettre = c.touche.to_unicode().is_some_and(char::is_alphabetic);
    let controle = c
        .touche
        .to_unicode()
        .is_some_and(|k| matches!(k, '[' | '\\' | ']' | '^' | '_' | '@' | ' '));
    let ctrl_seul = c.mods == M::CONTROL_MASK;
    let seul = ctrl_seul || c.mods == M::ALT_MASK || c.mods == M::CONTROL_MASK | M::ALT_MASK;
    if (lettre && seul) || (ctrl_seul && controle) {
        return Err(ErreurRaccourci::LaisseeAuxProgrammes(c.texte()));
    }
    Ok(())
}

/// Les actions dont le raccourci se change : clé de `[raccourcis]`, action, défaut.
const DEFAUTS: &[(&str, Raccourci, &str)] = &[
    ("nouvel_onglet", Raccourci::NouvelOnglet, "Ctrl+Maj+T"),
    ("fermer_onglet", Raccourci::FermerOnglet, "Ctrl+Maj+W"),
    ("nouvelle_fenetre", Raccourci::NouvelleFenetre, "Ctrl+Maj+N"),
    ("connexions", Raccourci::Connexions, "Ctrl+Maj+O"),
    ("copier", Raccourci::Copier, "Ctrl+Maj+C"),
    ("coller", Raccourci::Coller, "Ctrl+Maj+V"),
    ("rechercher", Raccourci::Rechercher, "Ctrl+Maj+F"),
    (
        "onglet_suivant",
        Raccourci::OngletSuivant,
        "Ctrl+Page suivante",
    ),
    (
        "onglet_precedent",
        Raccourci::OngletPrecedent,
        "Ctrl+Page précédente",
    ),
    ("plein_ecran", Raccourci::PleinEcran, "F11"),
    ("zoom_plus", Raccourci::ZoomPlus, "Ctrl+plus"),
    ("zoom_moins", Raccourci::ZoomMoins, "Ctrl+moins"),
    ("zoom_normal", Raccourci::ZoomNormal, "Ctrl+0"),
    (
        "commande_precedente",
        Raccourci::CommandePrecedente,
        "Ctrl+Maj+↑",
    ),
    (
        "commande_suivante",
        Raccourci::CommandeSuivante,
        "Ctrl+Maj+↓",
    ),
    ("aide", Raccourci::Aide, "Ctrl+Maj+H"),
    ("reglages", Raccourci::Reglages, ""),
];

/// Raccourcis en vigueur : ceux par défaut, avec les remplacements de `[raccourcis]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRaccourcis {
    /// `(clé, action, combinaison)` ; pas de combinaison : l'action n'a pas de raccourci.
    entrees: Vec<(&'static str, Raccourci, Option<Combinaison>)>,
}

impl Default for TableRaccourcis {
    fn default() -> Self {
        Self::defaut()
    }
}

impl TableRaccourcis {
    /// La table par défaut (celle de l'aide).
    pub fn defaut() -> Self {
        TableRaccourcis {
            entrees: DEFAUTS
                .iter()
                .map(|(cle, action, texte)| (*cle, *action, analyser_libre(texte).ok()))
                .collect(),
        }
    }

    /// Combinaison en vigueur d'une action, s'il y en a une.
    pub fn combinaison(&self, action: Raccourci) -> Option<&Combinaison> {
        self.entrees
            .iter()
            .find(|(_, a, _)| *a == action)
            .and_then(|(_, _, c)| c.as_ref())
    }

    /// Accélérateur GTK de l'action, pour le menu.
    pub fn accelerateur(&self, action: Raccourci) -> Option<String> {
        self.combinaison(action).map(Combinaison::accelerateur)
    }

    /// Table par défaut avec les lignes de `[raccourcis]`. Une ligne refusée (clé inconnue,
    /// valeur invalide), ou deux actions sur la même combinaison, ne change rien pour les
    /// actions concernées, qui gardent leur défaut ; chaque refus donne un message (avec la
    /// ligne du fichier) pour le bandeau.
    pub fn depuis_reglages(reglages: &[ReglageRaccourci]) -> (Self, Vec<String>) {
        let mut table = Self::defaut();
        let defauts = table.clone();
        let mut messages = Vec::new();
        let avec_ligne = |ligne: Option<usize>, message: String| match ligne {
            Some(l) => ta(
                "config-ligne",
                &[
                    ("ligne", ArgI18n::from(l)),
                    ("message", ArgI18n::from(message)),
                ],
            ),
            None => message,
        };
        // Lignes des actions remplacées (pour nommer les fautives dans un conflit).
        let mut lignes: Vec<Option<usize>> = vec![None; table.entrees.len()];
        for r in reglages {
            let Some(i) = table.entrees.iter().position(|(c, _, _)| *c == r.cle) else {
                messages.push(avec_ligne(
                    r.ligne,
                    ta(
                        "raccourci-cle-inconnue",
                        &[
                            ("cle", ArgI18n::from(r.cle.as_str())),
                            (
                                "cles",
                                ArgI18n::from(pupitre_config::ACTIONS_RACCOURCIS.join(", ")),
                            ),
                        ],
                    ),
                ));
                continue;
            };
            let valeur = r.valeur.trim();
            let analyse = if valeur.is_empty() {
                Ok(None)
            } else {
                analyser_libre(valeur).and_then(|c| {
                    // Écrire le défaut dans le fichier reste permis, même s'il est réservé.
                    if defauts.entrees[i].2 != Some(c) {
                        verifier(&c)?;
                    }
                    Ok(Some(c))
                })
            };
            match analyse {
                Ok(c) => {
                    table.entrees[i].2 = c;
                    lignes[i] = r.ligne;
                }
                Err(e) => messages.push(avec_ligne(
                    r.ligne,
                    ta(
                        "raccourci-refuse",
                        &[
                            ("cle", ArgI18n::from(r.cle.as_str())),
                            ("valeur", ArgI18n::from(valeur)),
                            ("raison", ArgI18n::from(e.to_string())),
                        ],
                    ),
                )),
            }
        }
        // Conflits : toutes les actions concernées reprennent leur défaut. Ce retour peut en
        // créer un nouveau (une valeur identique au défaut d'une autre) : on recommence
        // jusqu'à stabilité (chaque tour remet au moins une action à son défaut).
        loop {
            let n = table.entrees.len();
            let mut groupes: Vec<(Combinaison, Vec<usize>)> = Vec::new();
            for i in 0..n {
                let Some(c) = table.entrees[i].2 else {
                    continue;
                };
                match groupes.iter_mut().find(|(g, _)| *g == c) {
                    Some((_, v)) => v.push(i),
                    None => groupes.push((c, vec![i])),
                }
            }
            let mut a_remettre = Vec::new();
            for (c, membres) in groupes.iter().filter(|(_, v)| v.len() > 1) {
                let noms: Vec<String> = membres
                    .iter()
                    .map(|&i| match lignes[i] {
                        Some(l) => ta(
                            "raccourci-membre-ligne",
                            &[
                                ("cle", ArgI18n::from(table.entrees[i].0)),
                                ("ligne", ArgI18n::from(l)),
                            ],
                        ),
                        None => table.entrees[i].0.to_owned(),
                    })
                    .collect();
                messages.push(ta(
                    "raccourci-conflit",
                    &[
                        ("combinaison", ArgI18n::from(c.texte())),
                        ("actions", ArgI18n::from(noms.join(", "))),
                    ],
                ));
                a_remettre.extend(membres.iter().copied());
            }
            if a_remettre.is_empty() {
                break;
            }
            for i in a_remettre {
                table.entrees[i].2 = defauts.entrees[i].2;
                lignes[i] = None;
            }
        }
        (table, messages)
    }
}

/// Raccourcis toujours disponibles, quels que soient les réglages (accessibilité) :
/// Ctrl+Tab et Ctrl+Maj+Tab sortent du terminal, Alt+1…9 vont aux onglets, Ctrl+PgSuiv et
/// Ctrl+PgPréc changent d'onglet.
fn raccourci_fixe(touche: gdk::Key, code: u32, m: gdk::ModifierType) -> Option<Raccourci> {
    use gdk::ModifierType as M;
    let tab = matches!(touche, gdk::Key::Tab | gdk::Key::ISO_Left_Tab);
    if m == M::CONTROL_MASK | M::SHIFT_MASK && tab {
        return Some(Raccourci::FocusPrecedent);
    }
    if m == M::CONTROL_MASK {
        return match touche {
            gdk::Key::Page_Down | gdk::Key::KP_Page_Down => Some(Raccourci::OngletSuivant),
            gdk::Key::Page_Up | gdk::Key::KP_Page_Up => Some(Raccourci::OngletPrecedent),
            gdk::Key::Tab => Some(Raccourci::FocusSuivant),
            gdk::Key::ISO_Left_Tab => Some(Raccourci::FocusPrecedent),
            _ => None,
        };
    }
    if m == M::ALT_MASK {
        let par_caractere = touche
            .to_unicode()
            .and_then(|c| c.to_digit(10))
            .filter(|n| (1..=9).contains(n));
        let par_position = (CODE_TOUCHE_1..=CODE_TOUCHE_9)
            .contains(&code)
            .then(|| code - CODE_TOUCHE_1 + 1);
        return par_caractere.or(par_position).map(Raccourci::Onglet);
    }
    None
}

/// Raccourci correspondant à une touche (`code` = code matériel) d'après la table donnée.
pub fn raccourci_dans(
    table: &TableRaccourcis,
    touche: gdk::Key,
    code: u32,
    mods: gdk::ModifierType,
) -> Option<Raccourci> {
    let m = mods & mods_utiles();
    if let Some(r) = raccourci_fixe(touche, code, m) {
        return Some(r);
    }
    // Exact d'abord ; à Maj près ensuite (« + » et « 0 » demandent Maj sur certaines
    // dispositions), pour qu'une combinaison avec Maj ne soit jamais volée.
    [false, true].into_iter().find_map(|large| {
        table
            .entrees
            .iter()
            .find(|(_, _, c)| c.is_some_and(|c| c.reconnait(touche, code, m, large)))
            .map(|(_, a, _)| *a)
    })
}

/// Raccourci correspondant à une touche d'après la table par défaut.
pub fn raccourci(touche: gdk::Key, code: u32, mods: gdk::ModifierType) -> Option<Raccourci> {
    raccourci_dans(&TableRaccourcis::defaut(), touche, code, mods)
}

/// Intercepte les raccourcis de la fenêtre avant le terminal (sinon VTE consomme les
/// touches et les envoie au programme).
pub fn installer_raccourcis(fenetre: &crate::fenetre::FenetrePupitre) {
    use gtk::prelude::*;
    let touches = gtk::EventControllerKey::new();
    touches.set_propagation_phase(gtk::PropagationPhase::Capture);
    // Référence faible : le contrôleur appartient à la fenêtre (pas de cycle).
    let faible = fenetre.faible();
    touches.connect_key_pressed(move |_, touche, code, mods| {
        let f = crate::fenetre::FenetrePupitre::depuis_faible(&faible);
        if f.is_some_and(|f| f.traiter_touche(touche, code, mods)) {
            gtk::glib::Propagation::Stop
        } else {
            gtk::glib::Propagation::Proceed
        }
    });
    fenetre.fenetre().add_controller(touches);
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdk::ModifierType as M;

    const CS: M = M::CONTROL_MASK.union(M::SHIFT_MASK);

    #[test]
    fn ctrl_maj_lettres() {
        assert_eq!(
            raccourci(gdk::Key::T, 28, CS),
            Some(Raccourci::NouvelOnglet)
        );
        assert_eq!(
            raccourci(gdk::Key::w, 25, CS),
            Some(Raccourci::FermerOnglet)
        );
        assert_eq!(
            raccourci(gdk::Key::N, 57, CS),
            Some(Raccourci::NouvelleFenetre)
        );
        assert_eq!(raccourci(gdk::Key::O, 32, CS), Some(Raccourci::Connexions));
        assert_eq!(raccourci(gdk::Key::o, 32, M::CONTROL_MASK), None);
        assert_eq!(raccourci(gdk::Key::C, 54, CS), Some(Raccourci::Copier));
        assert_eq!(raccourci(gdk::Key::V, 55, CS), Some(Raccourci::Coller));
        assert_eq!(raccourci(gdk::Key::F, 41, CS), Some(Raccourci::Rechercher));
        // Ctrl seul : laissé au programme (Ctrl+C interrompt, Ctrl+W efface un mot…).
        assert_eq!(raccourci(gdk::Key::c, 54, M::CONTROL_MASK), None);
        assert_eq!(raccourci(gdk::Key::w, 25, M::CONTROL_MASK), None);
        // Verrouillage majuscules et pavé numérique ignorés ; Alt en plus refusé.
        let verr = CS | M::LOCK_MASK;
        assert_eq!(
            raccourci(gdk::Key::T, 28, verr),
            Some(Raccourci::NouvelOnglet)
        );
        assert_eq!(raccourci(gdk::Key::T, 28, CS | M::ALT_MASK), None);
    }

    #[test]
    fn alt_chiffres_qwerty_et_azerty() {
        // QWERTY : Alt+2 donne « 2 ».
        assert_eq!(
            raccourci(gdk::Key::_2, 11, M::ALT_MASK),
            Some(Raccourci::Onglet(2))
        );
        // AZERTY : Alt+« é » (touche 2) → repli sur la position.
        assert_eq!(
            raccourci(gdk::Key::eacute, 11, M::ALT_MASK),
            Some(Raccourci::Onglet(2))
        );
        assert_eq!(raccourci(gdk::Key::_0, 19, M::ALT_MASK), None);
    }

    #[test]
    fn alt_chiffres_fr_dvorak() {
        // fr dvorak, sans Maj : = / - è \ ^ ( ` ) sur les touches 1 à 9.
        let rangee = [
            gdk::Key::equal,
            gdk::Key::slash,
            gdk::Key::minus,
            gdk::Key::egrave,
            gdk::Key::backslash,
            gdk::Key::dead_circumflex,
            gdk::Key::parenleft,
            gdk::Key::ISO_Level3_Latch,
            gdk::Key::parenright,
        ];
        for (i, t) in rangee.into_iter().enumerate() {
            let n = i as u32 + 1;
            assert_eq!(
                raccourci(t, CODE_TOUCHE_1 + i as u32, M::ALT_MASK),
                Some(Raccourci::Onglet(n)),
                "touche {n}"
            );
        }
    }

    #[test]
    fn zoom() {
        let c = M::CONTROL_MASK;
        assert_eq!(raccourci(gdk::Key::plus, 21, c), Some(Raccourci::ZoomPlus));
        assert_eq!(
            raccourci(gdk::Key::KP_Add, 86, c),
            Some(Raccourci::ZoomPlus)
        );
        assert_eq!(
            raccourci(gdk::Key::minus, 20, c),
            Some(Raccourci::ZoomMoins)
        );
        assert_eq!(raccourci(gdk::Key::_0, 19, c), Some(Raccourci::ZoomNormal));
        // fr dvorak : « + » et « 0 » demandent Maj ; Ctrl seul sur la touche 0 donne « " ».
        assert_eq!(raccourci(gdk::Key::plus, 20, CS), Some(Raccourci::ZoomPlus));
        assert_eq!(raccourci(gdk::Key::_0, 19, CS), Some(Raccourci::ZoomNormal));
        assert_eq!(
            raccourci(gdk::Key::quotedbl, 19, c),
            Some(Raccourci::ZoomNormal)
        );
    }

    #[test]
    fn sortir_du_terminal() {
        let c = M::CONTROL_MASK;
        assert_eq!(
            raccourci(gdk::Key::Tab, 23, c),
            Some(Raccourci::FocusSuivant)
        );
        assert_eq!(
            raccourci(gdk::Key::ISO_Left_Tab, 23, CS),
            Some(Raccourci::FocusPrecedent)
        );
        assert_eq!(
            raccourci(gdk::Key::Tab, 23, CS),
            Some(Raccourci::FocusPrecedent)
        );
        // Tab seul reste au shell (complétion).
        assert_eq!(raccourci(gdk::Key::Tab, 23, M::empty()), None);
    }

    #[test]
    fn commandes_au_clavier() {
        // Ctrl+Maj+↑/↓ : navigation de commande en commande, jamais envoyée au shell.
        assert_eq!(
            raccourci(gdk::Key::Up, 111, CS),
            Some(Raccourci::CommandePrecedente)
        );
        assert_eq!(
            raccourci(gdk::Key::Down, 116, CS),
            Some(Raccourci::CommandeSuivante)
        );
        // Les flèches seules, avec Ctrl seul ou avec Maj seul, restent au shell.
        for mods in [M::empty(), M::CONTROL_MASK, M::SHIFT_MASK, M::ALT_MASK] {
            assert_eq!(raccourci(gdk::Key::Up, 111, mods), None, "{mods:?}");
            assert_eq!(raccourci(gdk::Key::Down, 116, mods), None, "{mods:?}");
        }
        // Avec Alt en plus : au programme aussi.
        assert_eq!(raccourci(gdk::Key::Up, 111, CS | M::ALT_MASK), None);
    }

    #[test]
    fn onglets_et_plein_ecran() {
        let c = M::CONTROL_MASK;
        assert_eq!(
            raccourci(gdk::Key::Page_Down, 117, c),
            Some(Raccourci::OngletSuivant)
        );
        assert_eq!(
            raccourci(gdk::Key::Page_Up, 112, c),
            Some(Raccourci::OngletPrecedent)
        );
        assert_eq!(raccourci(gdk::Key::Page_Down, 117, M::empty()), None);
        assert_eq!(
            raccourci(gdk::Key::F11, 95, M::empty()),
            Some(Raccourci::PleinEcran)
        );
        assert_eq!(raccourci(gdk::Key::F11, 95, c), None);
    }

    #[test]
    fn raccourci_aide() {
        assert_eq!(raccourci(gdk::Key::H, 43, CS), Some(Raccourci::Aide));
        assert_eq!(raccourci(gdk::Key::h, 43, CS), Some(Raccourci::Aide));
        // F1 reste aux programmes (aide de mc, htop, vim…), de même que Ctrl+H et Alt+H.
        assert_eq!(raccourci(gdk::Key::F1, 67, M::empty()), None);
        assert_eq!(raccourci(gdk::Key::F1, 67, M::CONTROL_MASK), None);
        assert_eq!(raccourci(gdk::Key::h, 43, M::CONTROL_MASK), None);
        assert_eq!(raccourci(gdk::Key::h, 43, M::ALT_MASK), None);
    }

    fn reglage(cle: &str, valeur: &str, ligne: usize) -> pupitre_config::ReglageRaccourci {
        pupitre_config::ReglageRaccourci {
            cle: cle.into(),
            valeur: valeur.into(),
            ligne: Some(ligne),
        }
    }

    fn table(reglages: &[pupitre_config::ReglageRaccourci]) -> (TableRaccourcis, Vec<String>) {
        pupitre_i18n::init(&["fr".parse().unwrap()]);
        TableRaccourcis::depuis_reglages(reglages)
    }

    #[test]
    fn analyser_cas() {
        let ctrl_maj_t = analyser_raccourci("Ctrl+Maj+T").unwrap();
        assert_eq!(ctrl_maj_t.mods(), CS);
        assert_eq!(ctrl_maj_t.touche(), gdk::Key::t);
        // Ordre, casse et espaces des modificateurs sans importance.
        assert_eq!(analyser_raccourci(" maj+ctrl+t ").unwrap(), ctrl_maj_t);
        assert_eq!(analyser_raccourci("CTRL+MAJ+T").unwrap(), ctrl_maj_t);
        let f11 = analyser_raccourci("F11").unwrap();
        assert_eq!((f11.mods(), f11.touche()), (M::empty(), gdk::Key::F11));
        let alt_q = analyser_raccourci("Maj+Alt+Q").unwrap();
        assert_eq!(
            (alt_q.mods(), alt_q.touche()),
            (M::ALT_MASK | M::SHIFT_MASK, gdk::Key::q)
        );
        let s = analyser_raccourci("Super+Alt+k").unwrap();
        assert_eq!(s.mods(), M::SUPER_MASK | M::ALT_MASK);
        // Noms de touches de l'aide.
        assert_eq!(
            analyser_raccourci("Ctrl+Page suivante").unwrap_err(),
            ErreurRaccourci::Reservee("Ctrl+Page suivante".into())
        );
        assert_eq!(
            analyser_raccourci("Alt+Page suivante").unwrap().touche(),
            gdk::Key::Page_Down
        );
        assert_eq!(
            analyser_raccourci("Ctrl+plus").unwrap().touche(),
            gdk::Key::plus
        );
        assert_eq!(
            analyser_raccourci("Ctrl++").unwrap().touche(),
            gdk::Key::plus
        );
        assert_eq!(
            analyser_raccourci("Ctrl+moins").unwrap().touche(),
            gdk::Key::minus
        );
        assert_eq!(
            analyser_raccourci("Ctrl+Maj+↑").unwrap().touche(),
            gdk::Key::Up
        );
        assert_eq!(analyser_raccourci("Ctrl+0").unwrap().touche(), gdk::Key::_0);
        // Écriture et relecture : le texte de l'aide revient à l'identique.
        for t in [
            "Ctrl+Maj+T",
            "F11",
            "Alt+Page suivante",
            "Ctrl+plus",
            "Ctrl+Maj+↓",
            "Maj+Alt+Q",
        ] {
            assert_eq!(analyser_raccourci(t).unwrap().texte(), t);
        }
        // Erreurs.
        assert!(matches!(
            analyser_raccourci("Ctrl+Truc"),
            Err(ErreurRaccourci::ToucheInconnue(_))
        ));
        assert_eq!(
            analyser_raccourci("Ctrl+"),
            Err(ErreurRaccourci::ToucheManquante)
        );
        assert_eq!(
            analyser_raccourci("Ctrl+Maj"),
            Err(ErreurRaccourci::ToucheManquante)
        );
        assert!(matches!(
            analyser_raccourci("Ctrl+Ctrl+T"),
            Err(ErreurRaccourci::ModificateurRepete(_))
        ));
        // Une lettre seule, ou avec Maj seule, volerait la frappe au shell.
        assert_eq!(
            analyser_raccourci("T"),
            Err(ErreurRaccourci::SansModificateur)
        );
        assert_eq!(
            analyser_raccourci("Maj+T"),
            Err(ErreurRaccourci::SansModificateur)
        );
        assert!(analyser_raccourci("F5").is_ok());
        assert!(analyser_raccourci("Maj+F5").is_ok());
    }

    #[test]
    fn touches_des_programmes_refusees() {
        for t in [
            "Ctrl+C",
            "Ctrl+D",
            "Ctrl+Z",
            "Ctrl+L",
            "Alt+B",
            "Alt+f",
            "Ctrl+Alt+L",
            "Ctrl+Espace",
            "Ctrl+[",
            "Ctrl+]",
            "Ctrl+^",
            "Ctrl+_",
            "Ctrl+@",
            "Ctrl+\\",
        ] {
            assert!(
                matches!(
                    analyser_raccourci(t),
                    Err(ErreurRaccourci::LaisseeAuxProgrammes(_))
                ),
                "{t} : {:?}",
                analyser_raccourci(t)
            );
        }
        for t in [
            "Ctrl+Maj+C",
            "Maj+Alt+B",
            "Ctrl+0",
            "Ctrl+5",
            "Ctrl+plus",
            "Ctrl+moins",
            "Super+T",
            "Ctrl+Alt+3",
            "Alt+Maj+Ctrl+L",
            "F2",
            "F9",
            "F11",
            "F12",
            "Ctrl+F5",
            "Alt+F4",
        ] {
            assert!(
                analyser_raccourci(t).is_ok(),
                "{t} : {:?}",
                analyser_raccourci(t)
            );
        }
        // Message affiché, avec sa ligne.
        let (_, m) = table(&[reglage("copier", "Ctrl+C", 6)]);
        assert!(m[0].contains("ligne 6") && m[0].contains("Ctrl+C"), "{m:?}");
    }

    #[test]
    fn touches_reservees_refusees() {
        for t in [
            "Tab",
            "Maj+Tab",
            "Alt+Tab",
            "F1",
            "Ctrl+F1",
            "F10",
            "Maj+F10",
            "Ctrl+Tab",
            "Ctrl+Maj+Tab",
            "Alt+1",
            "Alt+9",
            "Ctrl+Page suivante",
            "Ctrl+Page précédente",
        ] {
            assert!(
                matches!(analyser_raccourci(t), Err(ErreurRaccourci::Reservee(_))),
                "{t} : {:?}",
                analyser_raccourci(t)
            );
        }
        assert!(analyser_raccourci("Alt+0").is_ok());
    }

    #[test]
    fn defauts_sans_conflit() {
        let (t, messages) = table(&[]);
        assert_eq!(messages, Vec::<String>::new());
        assert_eq!(t, TableRaccourcis::defaut());
        let mut vues = Vec::new();
        for (_, _, c) in &t.entrees {
            let Some(c) = c.as_ref() else { continue };
            assert!(!vues.contains(c), "doublon : {}", c.texte());
            vues.push(*c);
        }
        assert_eq!(t.entrees.len(), pupitre_config::ACTIONS_RACCOURCIS.len());
        for (cle, _, _) in &t.entrees {
            assert!(pupitre_config::ACTIONS_RACCOURCIS.contains(cle), "{cle}");
        }
        // Écrire le défaut dans le fichier est permis, même pour un raccourci réservé.
        let (_, messages) = table(&[
            reglage("onglet_suivant", "Ctrl+Page suivante", 2),
            reglage("copier", "Ctrl+Maj+C", 3),
        ]);
        assert_eq!(messages, Vec::<String>::new());
    }

    #[test]
    fn raccourci_remplace() {
        let (t, messages) = table(&[reglage("copier", "Ctrl+Maj+X", 3)]);
        assert_eq!(messages, Vec::<String>::new());
        // Le nouveau raccourci copie ; l'ancien ne fait plus rien.
        assert_eq!(
            raccourci_dans(&t, gdk::Key::X, 53, CS),
            Some(Raccourci::Copier)
        );
        assert_eq!(raccourci_dans(&t, gdk::Key::C, 54, CS), None);
        // Le reste est inchangé.
        assert_eq!(
            raccourci_dans(&t, gdk::Key::V, 55, CS),
            Some(Raccourci::Coller)
        );
        assert_eq!(
            t.combinaison(Raccourci::Copier).map(Combinaison::texte),
            Some("Ctrl+Maj+X".to_string())
        );
        // Permutation de deux actions : pas un conflit.
        let (t, messages) = table(&[
            reglage("copier", "Ctrl+Maj+V", 3),
            reglage("coller", "Ctrl+Maj+C", 4),
        ]);
        assert_eq!(messages, Vec::<String>::new());
        assert_eq!(
            raccourci_dans(&t, gdk::Key::V, 55, CS),
            Some(Raccourci::Copier)
        );
        assert_eq!(
            raccourci_dans(&t, gdk::Key::C, 54, CS),
            Some(Raccourci::Coller)
        );
        // Une touche de fonction seule est permise ; plein écran passe sur F9.
        let (t, _) = table(&[reglage("plein_ecran", "F9", 2)]);
        assert_eq!(
            raccourci_dans(&t, gdk::Key::F9, 75, M::empty()),
            Some(Raccourci::PleinEcran)
        );
        assert_eq!(raccourci_dans(&t, gdk::Key::F11, 95, M::empty()), None);
    }

    #[test]
    fn raccourci_vide_desactive() {
        let (t, messages) = table(&[reglage("aide", "", 5), reglage("plein_ecran", "  ", 6)]);
        assert_eq!(messages, Vec::<String>::new());
        assert_eq!(raccourci_dans(&t, gdk::Key::H, 43, CS), None);
        assert_eq!(raccourci_dans(&t, gdk::Key::F11, 95, M::empty()), None);
        assert_eq!(t.combinaison(Raccourci::Aide), None);
        // Les raccourcis toujours disponibles le restent.
        let (t, _) = table(&[reglage("onglet_suivant", "", 2)]);
        assert_eq!(
            raccourci_dans(&t, gdk::Key::Page_Down, 117, M::CONTROL_MASK),
            Some(Raccourci::OngletSuivant)
        );
    }

    #[test]
    fn raccourci_invalide_garde_le_defaut() {
        let (t, messages) = table(&[
            reglage("copier", "Ctrl+Truc", 7),
            reglage("coller", "Ctrl+Maj+Y", 8),
            reglage("rechercher", "Tab", 9),
            reglage("copie", "Ctrl+Maj+X", 10),
        ]);
        // Copier : défaut gardé ; coller : changé quand même.
        assert_eq!(
            raccourci_dans(&t, gdk::Key::C, 54, CS),
            Some(Raccourci::Copier)
        );
        assert_eq!(
            raccourci_dans(&t, gdk::Key::Y, 29, CS),
            Some(Raccourci::Coller)
        );
        assert_eq!(
            raccourci_dans(&t, gdk::Key::F, 41, CS),
            Some(Raccourci::Rechercher)
        );
        assert_eq!(messages.len(), 3, "{messages:?}");
        for (m, ligne, mot) in [
            (&messages[0], "7", "copier"),
            (&messages[1], "9", "rechercher"),
            (&messages[2], "10", "copie"),
        ] {
            assert!(m.contains(&format!("ligne {ligne}")), "{m}");
            assert!(m.contains(mot), "{m}");
        }
        assert!(messages[0].contains("Truc"), "{}", messages[0]);
    }

    #[test]
    fn conflit_signale() {
        // Deux actions sur la même combinaison : les deux gardent leur défaut.
        let (t, messages) = table(&[
            reglage("copier", "Ctrl+Maj+X", 3),
            reglage("coller", "ctrl+maj+x", 4),
        ]);
        assert_eq!(
            raccourci_dans(&t, gdk::Key::C, 54, CS),
            Some(Raccourci::Copier)
        );
        assert_eq!(
            raccourci_dans(&t, gdk::Key::V, 55, CS),
            Some(Raccourci::Coller)
        );
        assert_eq!(raccourci_dans(&t, gdk::Key::X, 53, CS), None);
        assert_eq!(messages.len(), 1, "{messages:?}");
        for mot in ["copier", "coller", "ligne 3", "ligne 4", "Ctrl+Maj+X"] {
            assert!(messages[0].contains(mot), "{mot} : {}", messages[0]);
        }
        // Une action prend le défaut d'une autre, laquelle garde le sien : conflit.
        let (t, messages) = table(&[reglage("coller", "Ctrl+Maj+C", 2)]);
        assert_eq!(
            raccourci_dans(&t, gdk::Key::C, 54, CS),
            Some(Raccourci::Copier)
        );
        assert_eq!(
            raccourci_dans(&t, gdk::Key::V, 55, CS),
            Some(Raccourci::Coller)
        );
        assert_eq!(messages.len(), 1, "{messages:?}");
        // Le retour au défaut ne doit pas créer de nouveau conflit : coller prend le défaut de
        // copier pendant que copier et rechercher se heurtent ; copier revient à son défaut.
        let (t, messages) = table(&[
            reglage("copier", "Ctrl+Maj+X", 1),
            reglage("coller", "Ctrl+Maj+C", 2),
            reglage("rechercher", "Ctrl+Maj+X", 3),
        ]);
        assert!(!messages.is_empty());
        let mut vues = Vec::new();
        for (_, _, c) in &t.entrees {
            if let Some(c) = c {
                assert!(!vues.contains(c), "doublon : {}", c.texte());
                vues.push(*c);
            }
        }
    }

    #[test]
    fn dvorak_touche_physique() {
        // « Ctrl+Alt+3 » configuré : reconnu par la touche 3 (code 12) quel que soit le
        // caractère qu'elle donne (fr dvorak « - », azerty « " »).
        let (t, messages) = table(&[reglage("zoom_normal", "Ctrl+Alt+3", 2)]);
        assert_eq!(messages, Vec::<String>::new());
        let ca = M::CONTROL_MASK | M::ALT_MASK;
        assert_eq!(
            raccourci_dans(&t, gdk::Key::_3, 12, ca),
            Some(Raccourci::ZoomNormal)
        );
        assert_eq!(
            raccourci_dans(&t, gdk::Key::minus, 12, ca),
            Some(Raccourci::ZoomNormal)
        );
        assert_eq!(
            raccourci_dans(&t, gdk::Key::quotedbl, 12, ca),
            Some(Raccourci::ZoomNormal)
        );
        assert_eq!(raccourci_dans(&t, gdk::Key::minus, 20, ca), None);
        // Disposition non latine (russe) : « Ctrl+Maj+T » sur la touche physique T (code 28),
        // qui donne « е » (Cyrillic_ie).
        let (t, _) = table(&[]);
        assert_eq!(
            raccourci_dans(&t, gdk::Key::Cyrillic_IE, 28, CS),
            Some(Raccourci::NouvelOnglet)
        );
        // fr dvorak : la lettre produite fait foi (« T » est où se trouve « T »).
        assert_eq!(
            raccourci_dans(&t, gdk::Key::T, 45, CS),
            Some(Raccourci::NouvelOnglet)
        );
        assert_eq!(raccourci_dans(&t, gdk::Key::Y, 28, CS), None);
    }

    /// Touche, code matériel et modificateurs d'un raccourci écrit dans l'aide
    /// (« Ctrl+Maj+T », « Ctrl+Page suivante », « Alt+3 »…).
    fn touche_de_l_aide(texte: &str) -> (gdk::Key, u32, M) {
        let mut mods = M::empty();
        let mut reste = texte.trim();
        loop {
            if let Some(r) = reste.strip_prefix("Ctrl+") {
                mods |= M::CONTROL_MASK;
                reste = r;
            } else if let Some(r) = reste.strip_prefix("Maj+") {
                mods |= M::SHIFT_MASK;
                reste = r;
            } else if let Some(r) = reste.strip_prefix("Alt+") {
                mods |= M::ALT_MASK;
                reste = r;
            } else {
                break;
            }
        }
        let (touche, code) = match reste {
            "Page suivante" => (gdk::Key::Page_Down, 117),
            "Page précédente" => (gdk::Key::Page_Up, 112),
            "plus" => (gdk::Key::plus, 21),
            "moins" => (gdk::Key::minus, 20),
            "0" => (gdk::Key::_0, CODE_TOUCHE_0),
            "Tab" if mods.contains(M::SHIFT_MASK) => (gdk::Key::ISO_Left_Tab, 23),
            "Tab" => (gdk::Key::Tab, 23),
            "↑" => (gdk::Key::Up, 111),
            "↓" => (gdk::Key::Down, 116),
            "F11" => (gdk::Key::F11, 95),
            "F1" => (gdk::Key::F1, 67),
            chiffre if chiffre.len() == 1 && chiffre.as_bytes()[0].is_ascii_digit() => {
                let n = u32::from(chiffre.as_bytes()[0] - b'0');
                (gdk::Key::from_name(chiffre).unwrap(), CODE_TOUCHE_1 + n - 1)
            }
            nom => (
                gdk::Key::from_name(nom).unwrap_or_else(|| panic!("touche inconnue : {nom}")),
                0,
            ),
        };
        (touche, code, mods)
    }

    #[test]
    fn raccourcis_de_l_aide_conformes() {
        pupitre_i18n::init(&["fr".parse().unwrap()]);
        let aide = pupitre_i18n::aide();
        let section = aide
            .split("\n## ")
            .find(|s| s.starts_with("2. Raccourcis clavier"))
            .expect("section 2");
        let mut lignes = 0;
        for l in section.lines().filter(|l| l.starts_with('|')) {
            let cases: Vec<&str> = l.trim_matches('|').split('|').map(str::trim).collect();
            if cases[0] == "Action" || cases[0].starts_with("---") {
                continue;
            }
            let (action, cite) = (cases[0], cases[1]);
            // « Alt+1 … Alt+9 » : une plage, vérifiée touche par touche.
            let citees: Vec<String> = match cite.split_once(" … ") {
                Some(("Alt+1", "Alt+9")) => (1..=9).map(|n| format!("Alt+{n}")).collect(),
                Some(_) => panic!("plage inconnue : {cite}"),
                None => vec![cite.to_string()],
            };
            for c in citees {
                let (t, code, mods) = touche_de_l_aide(&c);
                let attendu = match action {
                    "Nouvel onglet" => Some(Raccourci::NouvelOnglet),
                    "Fermer l'onglet" => Some(Raccourci::FermerOnglet),
                    "Nouvelle fenêtre" => Some(Raccourci::NouvelleFenetre),
                    "Connexions SSH" => Some(Raccourci::Connexions),
                    "Copier" => Some(Raccourci::Copier),
                    "Coller" => Some(Raccourci::Coller),
                    "Rechercher dans l'historique" => Some(Raccourci::Rechercher),
                    "Onglet suivant" => Some(Raccourci::OngletSuivant),
                    "Onglet précédent" => Some(Raccourci::OngletPrecedent),
                    "Aller à l'onglet 1 à 9" => Some(Raccourci::Onglet(
                        c.trim_start_matches("Alt+").parse().unwrap(),
                    )),
                    "Agrandir le texte" => Some(Raccourci::ZoomPlus),
                    "Réduire le texte" => Some(Raccourci::ZoomMoins),
                    "Taille normale" => Some(Raccourci::ZoomNormal),
                    "Plein écran" => Some(Raccourci::PleinEcran),
                    "Quitter le terminal vers l'élément suivant" => Some(Raccourci::FocusSuivant),
                    "Revenir à l'élément précédent" => Some(Raccourci::FocusPrecedent),
                    "Commande précédente" => Some(Raccourci::CommandePrecedente),
                    "Commande suivante" => Some(Raccourci::CommandeSuivante),
                    "Ouvrir cette aide" => Some(Raccourci::Aide),
                    autre => panic!("action de l'aide sans test : « {autre} »"),
                };
                assert_eq!(raccourci(t, code, mods), attendu, "{action} : {c}");
                lignes += 1;
            }
        }
        assert!(
            lignes >= 25,
            "le tableau de la section 2 n'a pas été lu ({lignes})"
        );
    }
}
