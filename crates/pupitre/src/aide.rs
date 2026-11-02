// SPDX-License-Identifier: GPL-3.0-or-later
//! Aide intégrée de Pupitre (Ctrl+Maj+H) : le texte de `assets/aide/<langue>.md` est dans
//! l'exécutable (`pupitre_i18n::aide()`), donc disponible hors ligne. Pas de dépendance
//! Markdown : le découpage et la mise en forme minimale suffisent pour ce fichier, écrit
//! par Pupitre.

use crate::messages::{annoncer, annoncer_focus_une_fois_affiche};
use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use pupitre_config::{theme_sombre, Config, Theme};
use pupitre_i18n::{t, ta, ArgI18n};
use std::cell::RefCell;
use std::rc::{Rc, Weak};

/// Une section de l'aide : un titre Markdown (`#`, `##` ou `###`) et le texte qui le suit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub titre: String,
    pub niveau: u8,
    pub texte: String,
}

/// Titre Markdown de niveau 1 à 3 : `(niveau, texte)`.
fn titre_de(ligne: &str) -> Option<(u8, &str)> {
    let marques = ligne.chars().take_while(|&c| c == '#').count();
    let niveau = match marques {
        1 => 1,
        2 => 2,
        3 => 3,
        _ => return None,
    };
    ligne[marques..]
        .strip_prefix(' ')
        .map(|t| (niveau, t.trim()))
}

fn est_cloture(ligne: &str) -> bool {
    ligne.trim_start().starts_with("```")
}

/// Découpe l'aide aux titres `#`, `##`, `###` (hors blocs de code). Un texte placé avant le
/// premier titre, s'il y en a, forme une section sans titre : rien n'est perdu.
pub fn sections(aide: &str) -> Vec<Section> {
    let mut v: Vec<Section> = Vec::new();
    let mut lignes: Vec<&str> = Vec::new();
    let mut courante: Option<(u8, String)> = None;
    let mut dans_code = false;
    let clore = |courante: Option<(u8, String)>, lignes: &mut Vec<&str>, v: &mut Vec<Section>| {
        let texte = lignes.join("\n").trim_matches('\n').to_string();
        lignes.clear();
        match courante {
            Some((niveau, titre)) => v.push(Section {
                titre,
                niveau,
                texte,
            }),
            None if !texte.trim().is_empty() => v.push(Section {
                titre: String::new(),
                niveau: 1,
                texte,
            }),
            None => {}
        }
    };
    for l in aide.lines() {
        if est_cloture(l) {
            dans_code = !dans_code;
        }
        match (dans_code, titre_de(l)) {
            (false, Some((niveau, titre))) => {
                clore(courante.take(), &mut lignes, &mut v);
                courante = Some((niveau, titre.to_string()));
            }
            _ => lignes.push(l),
        }
    }
    clore(courante, &mut lignes, &mut v);
    v
}

fn cellules(ligne: &str) -> Option<Vec<String>> {
    let l = ligne.trim();
    let l = l.strip_prefix('|')?;
    let l = l.strip_suffix('|').unwrap_or(l);
    Some(l.split('|').map(|c| c.trim().to_string()).collect())
}

fn est_separateur(cases: &[String]) -> bool {
    !cases.is_empty()
        && cases
            .iter()
            .all(|c| !c.is_empty() && c.chars().all(|x| matches!(x, '-' | ':' | ' ')))
}

/// Une ligne de tableau Markdown « | a | b | » lue comme « a : b » (un tableau n'est pas
/// lisible tel quel par un lecteur d'écran). `None` pour une ligne qui n'en est pas une
/// et pour la ligne de séparation « |---|---| ».
pub fn ligne_tableau(ligne: &str) -> Option<String> {
    let c = cellules(ligne)?;
    (!est_separateur(&c)).then(|| c.join(" : "))
}

/// Style d'un morceau de texte affiché.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Normal,
    Gras,
    /// Police du terminal.
    Code,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub texte: String,
    pub style: Style,
}

fn ajouter(v: &mut Vec<Segment>, texte: &str, style: Style) {
    if texte.is_empty() {
        return;
    }
    if let Some(dernier) = v.last_mut() {
        if dernier.style == style {
            dernier.texte.push_str(texte);
            return;
        }
    }
    v.push(Segment {
        texte: texte.to_string(),
        style,
    });
}

/// Mise en forme d'une ligne : `**gras**` et `` `code` `` (marques retirées).
fn en_ligne(ligne: &str, v: &mut Vec<Segment>) {
    let (mut gras, mut code) = (false, false);
    let mut reste = ligne;
    while !reste.is_empty() {
        let fin = reste.find(['*', '`']).unwrap_or(reste.len());
        let style = if code {
            Style::Code
        } else if gras {
            Style::Gras
        } else {
            Style::Normal
        };
        ajouter(v, &reste[..fin], style);
        reste = &reste[fin..];
        if let Some(r) = reste.strip_prefix('`') {
            code = !code;
            reste = r;
        } else if let Some(r) = reste.strip_prefix("**").filter(|_| !code) {
            gras = !gras;
            reste = r;
        } else if let Some(c) = reste.chars().next() {
            // Une étoile isolée (ou dans du code) est du texte.
            ajouter(v, &reste[..c.len_utf8()], style);
            reste = &reste[c.len_utf8()..];
        }
    }
}

/// Une ligne de corps de tableau lue pour le lecteur d'écran. À deux colonnes :
/// « a : b ». Au-delà, les colonnes suivantes sont nommées par leur en-tête :
/// « a : Défaut b ; Valeurs c ».
fn ligne_de_tableau(en_tete: &[String], cases: &[String]) -> String {
    if cases.len() <= 2 || en_tete.len() != cases.len() {
        return cases.join(" : ");
    }
    let suite: Vec<String> = cases
        .iter()
        .zip(en_tete)
        .skip(1)
        .map(|(c, h)| format!("{h} {c}"))
        .collect();
    format!("{} : {}", cases[0], suite.join(" ; "))
}

/// Mise en forme minimale d'un texte de section : titres déjà retirés par `sections`.
/// Les tableaux deviennent des lignes « Action : raccourci » (leur en-tête disparaît), les
/// blocs de code gardent leur texte en `Style::Code` (sans les clôtures), les filets
/// « --- » disparaissent, `**gras**` et `` `code` `` perdent leurs marques.
pub fn rendre(texte: &str) -> Vec<Segment> {
    let lignes: Vec<&str> = texte.lines().collect();
    let mut v = Vec::new();
    let mut dans_code = false;
    let mut en_tete: Option<Vec<String>> = None;
    let mut i = 0;
    while i < lignes.len() {
        let l = lignes[i];
        i += 1;
        if est_cloture(l) {
            dans_code = !dans_code;
            continue;
        }
        if dans_code {
            ajouter(&mut v, l, Style::Code);
            ajouter(&mut v, "\n", Style::Code);
            continue;
        }
        if l.trim() == "---" {
            continue;
        }
        let mut ligne = l.to_string();
        if let Some(c) = cellules(l) {
            if est_separateur(&c) {
                continue;
            }
            let suivante_separe = lignes
                .get(i)
                .and_then(|x| cellules(x))
                .is_some_and(|x| est_separateur(&x));
            if suivante_separe {
                en_tete = Some(c);
                continue;
            }
            ligne = ligne_de_tableau(en_tete.as_deref().unwrap_or(&[]), &c);
        } else {
            en_tete = None;
        }
        en_ligne(&ligne, &mut v);
        ajouter(&mut v, "\n", Style::Normal);
    }
    v
}

/// Toute l'aide en texte simple (pour `pupitre aide`) : titres sans marques, tableaux en
/// lignes, sans Markdown.
pub fn texte_brut(aide: &str) -> String {
    let mut sortie = String::new();
    for s in sections(aide) {
        if !s.titre.is_empty() {
            sortie.push_str(&s.titre);
            sortie.push_str("\n\n");
        }
        for seg in rendre(&s.texte) {
            sortie.push_str(&seg.texte);
        }
        sortie.push('\n');
    }
    sortie
}

/// Minuscules sans accents : « Démarrer » est trouvé par « demarrer ».
fn normaliser(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'à' | 'â' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' => 'i',
            'ô' | 'ö' => 'o',
            'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            autre => autre,
        })
        .collect()
}

/// Indices des sections dont le titre ou le texte contient `requete` (sans tenir compte
/// des majuscules ni des accents). Une requête vide les garde toutes.
pub fn filtrer(sections: &[Section], requete: &str) -> Vec<usize> {
    let r = normaliser(requete.trim());
    sections
        .iter()
        .enumerate()
        .filter(|(_, s)| {
            r.is_empty() || normaliser(&s.titre).contains(&r) || normaliser(&s.texte).contains(&r)
        })
        .map(|(i, _)| i)
        .collect()
}

// --- Fenêtre « Aide de Pupitre » ---

/// Réglages d'affichage de l'aide : suivent la configuration de Pupitre (police, thème) et
/// le zoom de l'onglet actif.
struct Apparence {
    police: String,
    zoom: u16,
    theme: Theme,
}

thread_local! {
    static APPARENCE: RefCell<Apparence> = RefCell::new(Apparence {
        police: Config::default().police,
        zoom: 100,
        theme: theme_sombre(),
    });
    static STYLE: RefCell<Option<gtk::CssProvider>> = const { RefCell::new(None) };
    static OUVERTE: RefCell<Weak<Etat>> = const { RefCell::new(Weak::new()) };
}

const CLASSE: &str = "pupitre-aide";

/// Police et couleurs de l'aide : celles de la configuration et du thème de Pupitre.
pub fn definir_apparence(police: &str, theme: &Theme) {
    APPARENCE.with(|a| {
        let mut a = a.borrow_mut();
        a.police = police.to_string();
        a.theme = theme.clone();
    });
    appliquer_apparence();
}

/// Zoom (en pour cent) de l'onglet actif : la taille du texte de l'aide le suit.
pub fn definir_zoom(zoom: u16) {
    APPARENCE.with(|a| a.borrow_mut().zoom = zoom);
    appliquer_apparence();
}

/// Taille en points du texte de l'aide : celle de la police du terminal, zoom compris.
fn taille_pt(police: &str, zoom: u16) -> f64 {
    let pt = f64::from(gtk::pango::FontDescription::from_string(police).size())
        / f64::from(gtk::pango::SCALE);
    let pt = if pt >= 1.0 { pt } else { 11.0 };
    pt * f64::from(zoom) / 100.0
}

fn famille(police: &str) -> String {
    gtk::pango::FontDescription::from_string(police)
        .family()
        .map_or_else(|| "monospace".to_string(), |f| f.to_string())
}

fn css() -> String {
    APPARENCE.with(|a| {
        let a = a.borrow();
        let t = &a.theme;
        format!(
            ".{c} {{ background-color: {fond}; color: {texte}; font-size: {pt:.1}pt; }}\n\
             .{c} list, .{c} row, .{c} textview, .{c} textview text, .{c} scrolledwindow, \
             .{c} box, .{c} paned {{ background-color: {fond}; color: {texte}; }}\n\
             .{c} entry, .{c} entry text {{ background-color: {fond}; color: {texte}; \
             caret-color: {curseur}; }}\n\
             .{c} row:selected, .{c} row:selected label {{ background-color: {sf}; color: {st}; }}\n\
             .{c} textview text selection {{ background-color: {sf}; color: {st}; }}\n",
            c = CLASSE,
            fond = t.fond,
            texte = t.texte,
            curseur = t.curseur,
            sf = t.selection_fond,
            st = t.selection_texte,
            pt = taille_pt(&a.police, a.zoom),
        )
    })
}

/// (Ré)écrit la feuille de style et la police du code de la fenêtre ouverte.
fn appliquer_apparence() {
    let Some(affichage) = gtk::gdk::Display::default() else {
        return;
    };
    STYLE.with(|s| {
        let mut s = s.borrow_mut();
        let fournisseur = s.get_or_insert_with(|| {
            let f = gtk::CssProvider::new();
            gtk::style_context_add_provider_for_display(
                &affichage,
                &f,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
            f
        });
        fournisseur.load_from_string(&css());
    });
    if let Some(e) = OUVERTE.with(|o| o.borrow().upgrade()) {
        let police = APPARENCE.with(|a| a.borrow().police.clone());
        e.code.set_family(Some(&famille(&police)));
    }
}

struct Etat {
    /// Faible : la fenêtre garde l'état (pas de cycle), pas l'inverse.
    fenetre: glib::WeakRef<gtk::Window>,
    sections: Vec<Section>,
    lignes: Vec<gtk::ListBoxRow>,
    liste: gtk::ListBox,
    recherche: gtk::SearchEntry,
    texte: gtk::TextView,
    defilement: gtk::ScrolledWindow,
    code: gtk::TextTag,
    /// Une case par section : visible avec le filtre courant.
    visibles: Rc<RefCell<Vec<bool>>>,
}

fn nouvelle_ligne(s: &Section) -> gtk::ListBoxRow {
    let etiquette = gtk::Label::builder()
        .label(&s.titre)
        .xalign(0.0)
        .wrap(true)
        .margin_start(12 * (i32::from(s.niveau) - 1) + 6)
        .margin_end(6)
        .margin_top(4)
        .margin_bottom(4)
        .build();
    let ligne = gtk::ListBoxRow::builder().child(&etiquette).build();
    // Rôle « titre » et niveau pour le lecteur d'écran : le nom de la ligne est le titre.
    ligne.update_property(&[
        gtk::accessible::Property::Label(&s.titre),
        gtk::accessible::Property::Level(i32::from(s.niveau)),
    ]);
    ligne
}

/// Montre la section `i` dans le panneau de droite : son titre, puis son texte mis en forme.
fn afficher_section(e: &Etat, i: usize) {
    let Some(s) = e.sections.get(i) else { return };
    let tampon = e.texte.buffer();
    tampon.set_text("");
    let mut fin = tampon.end_iter();
    tampon.insert_with_tags_by_name(&mut fin, &s.titre, &["titre"]);
    tampon.insert(&mut fin, "\n\n");
    for seg in rendre(&s.texte) {
        let mut fin = tampon.end_iter();
        match seg.style {
            Style::Normal => tampon.insert(&mut fin, &seg.texte),
            Style::Gras => tampon.insert_with_tags_by_name(&mut fin, &seg.texte, &["gras"]),
            Style::Code => tampon.insert_with_tags(&mut fin, &seg.texte, &[&e.code]),
        }
    }
    // Curseur et défilement au début : le lecteur d'écran commence par le titre.
    tampon.place_cursor(&tampon.start_iter());
    e.defilement.vadjustment().set_value(0.0);
}

fn indice_selectionne(e: &Etat) -> Option<usize> {
    e.liste
        .selected_row()
        .and_then(|l| usize::try_from(l.index()).ok())
}

fn choisir(e: &Etat, i: usize) {
    if let Some(l) = e.lignes.get(i) {
        e.liste.select_row(Some(l));
    }
}

/// Applique la recherche : cache les sections sans le texte, garde une section choisie
/// visible et dit combien de sections correspondent.
fn rechercher(e: &Etat, annoncer_resultat: bool) {
    let requete = e.recherche.text().to_string();
    let trouvees = filtrer(&e.sections, &requete);
    {
        let mut v = e.visibles.borrow_mut();
        v.iter_mut().for_each(|x| *x = false);
        for &i in &trouvees {
            if let Some(x) = v.get_mut(i) {
                *x = true;
            }
        }
    }
    e.liste.invalidate_filter();
    match trouvees.first() {
        Some(&premiere) => {
            if !indice_selectionne(e).is_some_and(|i| trouvees.contains(&i)) {
                choisir(e, premiere);
            }
        }
        None => {
            e.liste.unselect_all();
            e.texte.buffer().set_text("");
        }
    }
    if annoncer_resultat && !requete.trim().is_empty() {
        let phrase = match trouvees.len() {
            0 => t("aide-aucun-resultat"),
            n => ta("aide-resultats", &[("n", ArgI18n::from(n))]),
        };
        if let Some(f) = e.fenetre.upgrade() {
            annoncer(&f, &phrase);
        }
    }
}

fn donner_focus_liste(e: &Etat) {
    let cible: gtk::Widget = match e.liste.selected_row() {
        Some(l) => l.upcast(),
        None => e.liste.clone().upcast(),
    };
    cible.grab_focus();
    annoncer_focus_une_fois_affiche(&cible);
}

/// Ctrl+Tab / Ctrl+Maj+Tab : de la recherche à la liste, au texte, et retour.
fn changer_de_zone(e: &Etat, sens: i32) {
    let zones: [gtk::Widget; 3] = [
        e.recherche.clone().upcast(),
        e.liste.clone().upcast(),
        e.texte.clone().upcast(),
    ];
    let Some(fenetre) = e.fenetre.upgrade() else {
        return;
    };
    let focus = GtkWindowExt::focus(&fenetre);
    let courante = focus
        .as_ref()
        .and_then(|f| zones.iter().position(|z| f == z || f.is_ancestor(z)))
        .unwrap_or(1);
    let suivante = (courante as i32 + sens).rem_euclid(3) as usize;
    let cible = match (suivante, e.liste.selected_row()) {
        (1, Some(ligne)) => ligne.upcast(),
        (i, _) => zones[i].clone(),
    };
    cible.grab_focus();
}

fn section_voulue(sections: &[Section], voulue: &str) -> Option<usize> {
    let v = normaliser(voulue.trim());
    sections
        .iter()
        .position(|s| normaliser(&s.titre) == v)
        .or_else(|| {
            sections
                .iter()
                .position(|s| normaliser(&s.titre).contains(&v))
        })
}

/// Ouvre l'aide (une seule fenêtre : la rouvrir la ramène au premier plan) sur la section
/// dont le titre contient `section`, ou sur la première.
pub fn ouvrir_aide(app: &gtk::Application, parent: Option<&gtk::Window>, section: Option<&str>) {
    // Déjà ouverte (et pas détruite entre-temps) : on la ramène au premier plan.
    let ouverte = OUVERTE.with(|o| o.borrow().upgrade()).and_then(|e| {
        e.fenetre
            .upgrade()
            .filter(|f| f.is_visible())
            .map(|f| (e, f))
    });
    if let Some((e, f)) = ouverte {
        if let Some(voulue) = section {
            e.recherche.set_text("");
            rechercher(&e, false);
            if let Some(i) = section_voulue(&e.sections, voulue) {
                choisir(&e, i);
            }
        }
        f.present();
        donner_focus_liste(&e);
        return;
    }
    appliquer_apparence();
    let sections: Vec<Section> = sections(pupitre_i18n::aide())
        .into_iter()
        .filter(|s| !s.titre.is_empty())
        .collect();
    let fenetre = gtk::Window::builder()
        .title(t("aide-titre"))
        .default_width(900)
        .default_height(600)
        .accessible_role(gtk::AccessibleRole::Dialog)
        .build();
    if let Some(p) = parent {
        fenetre.set_transient_for(Some(p));
        fenetre.set_destroy_with_parent(true);
    }
    // Rattachée à l'application : même identifiant (et donc même icône) que les fenêtres
    // principales pour le bureau.
    fenetre.set_application(Some(app));
    fenetre.add_css_class(CLASSE);

    let recherche = gtk::SearchEntry::builder()
        .placeholder_text(t("aide-recherche"))
        .hexpand(true)
        .build();
    recherche.set_widget_name("recherche");
    recherche.update_property(&[gtk::accessible::Property::Label(&t("aide-recherche"))]);
    let liste = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::Browse)
        .activate_on_single_click(true)
        .build();
    liste.set_widget_name("liste");
    liste.update_property(&[gtk::accessible::Property::Label(&t("aide-liste"))]);
    let lignes: Vec<gtk::ListBoxRow> = sections.iter().map(nouvelle_ligne).collect();
    for l in &lignes {
        liste.append(l);
    }
    let visibles = Rc::new(RefCell::new(vec![true; sections.len()]));
    let v = visibles.clone();
    liste.set_filter_func(move |ligne| {
        usize::try_from(ligne.index())
            .ok()
            .and_then(|i| v.borrow().get(i).copied())
            .unwrap_or(true)
    });
    let defile_liste = gtk::ScrolledWindow::builder()
        .child(&liste)
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();
    let gauche = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .margin_start(6)
        .margin_end(6)
        .margin_top(6)
        .margin_bottom(6)
        .build();
    gauche.append(&recherche);
    gauche.append(&defile_liste);

    let texte = gtk::TextView::builder()
        .editable(false)
        .cursor_visible(true)
        .wrap_mode(gtk::WrapMode::WordChar)
        .left_margin(12)
        .right_margin(12)
        .top_margin(8)
        .bottom_margin(8)
        .build();
    texte.set_widget_name("texte");
    texte.update_property(&[gtk::accessible::Property::Label(&t("aide-texte"))]);
    let tampon = texte.buffer();
    let _ = tampon.create_tag(Some("gras"), &[("weight", &700i32)]);
    let _ = tampon.create_tag(Some("titre"), &[("weight", &700i32), ("scale", &1.4f64)]);
    let police = APPARENCE.with(|a| a.borrow().police.clone());
    let code = gtk::TextTag::builder()
        .name("code")
        .family(famille(&police))
        .build();
    tampon.tag_table().add(&code);
    let defilement = gtk::ScrolledWindow::builder()
        .child(&texte)
        .hexpand(true)
        .vexpand(true)
        .build();

    let panneaux = gtk::Paned::builder()
        .orientation(gtk::Orientation::Horizontal)
        .start_child(&gauche)
        .end_child(&defilement)
        .resize_start_child(false)
        .shrink_start_child(false)
        .position(280)
        .build();
    fenetre.set_child(Some(&panneaux));

    let etat = Rc::new(Etat {
        fenetre: fenetre.downgrade(),
        sections,
        lignes,
        liste: liste.clone(),
        recherche: recherche.clone(),
        texte,
        defilement,
        code,
        visibles,
    });

    let faible = Rc::downgrade(&etat);
    liste.connect_row_selected(move |_, ligne| {
        if let (Some(e), Some(l)) = (faible.upgrade(), ligne) {
            if let Ok(i) = usize::try_from(l.index()) {
                afficher_section(&e, i);
            }
        }
    });
    let faible = Rc::downgrade(&etat);
    recherche.connect_search_changed(move |_| {
        if let Some(e) = faible.upgrade() {
            rechercher(&e, true);
        }
    });
    // Entrée dans la recherche : on passe au résultat.
    let faible = Rc::downgrade(&etat);
    recherche.connect_activate(move |_| {
        if let Some(e) = faible.upgrade() {
            donner_focus_liste(&e);
        }
    });
    // Échap ferme (même dans la recherche, qui sinon le garderait pour elle) ; Ctrl+Tab
    // passe d'une zone à l'autre (Tab seul y reste utilisable comme partout).
    let touches = gtk::EventControllerKey::new();
    touches.set_propagation_phase(gtk::PropagationPhase::Capture);
    let faible = Rc::downgrade(&etat);
    touches.connect_key_pressed(move |_, touche, _, mods| {
        let Some(e) = faible.upgrade() else {
            return glib::Propagation::Proceed;
        };
        let m = mods & (gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::SHIFT_MASK);
        if touche == gdk::Key::Escape {
            if let Some(f) = e.fenetre.upgrade() {
                f.destroy();
            }
            return glib::Propagation::Stop;
        }
        let tab = matches!(touche, gdk::Key::Tab | gdk::Key::ISO_Left_Tab);
        if tab && m.contains(gdk::ModifierType::CONTROL_MASK) {
            let arriere =
                m.contains(gdk::ModifierType::SHIFT_MASK) || touche == gdk::Key::ISO_Left_Tab;
            changer_de_zone(&e, if arriere { -1 } else { 1 });
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    fenetre.add_controller(touches);

    OUVERTE.with(|o| *o.borrow_mut() = Rc::downgrade(&etat));
    // La fenêtre est la seule à garder l'état : il disparaît avec elle.
    let garde = RefCell::new(Some(etat.clone()));
    fenetre.connect_destroy(move |_| {
        OUVERTE.with(|o| *o.borrow_mut() = Weak::new());
        drop(garde.borrow_mut().take());
    });

    let premiere = section
        .and_then(|s| section_voulue(&etat.sections, s))
        .unwrap_or(0);
    choisir(&etat, premiere);
    fenetre.present();
    donner_focus_liste(&etat);
}

#[cfg(test)]
mod tests {
    use super::*;
    use pupitre_i18n::aide;

    fn aide_fr() -> &'static str {
        pupitre_i18n::init(&["fr".parse().unwrap()]);
        aide()
    }

    /// Titres de l'aide cherchés sans passer par `sections` (autre lecture du même fichier).
    fn titres_attendus(a: &str) -> Vec<(u8, String)> {
        let mut dans_code = false;
        let mut v = Vec::new();
        for l in a.lines() {
            if l.trim_start().starts_with("```") {
                dans_code = !dans_code;
            } else if !dans_code {
                for (n, marque) in [(3u8, "### "), (2, "## "), (1, "# ")] {
                    if let Some(t) = l.strip_prefix(marque) {
                        v.push((n, t.trim().to_string()));
                        break;
                    }
                }
            }
        }
        v
    }

    #[test]
    fn sections_decoupees() {
        let a = aide_fr();
        let s = sections(a);
        let attendus = titres_attendus(a);
        assert!(
            attendus.len() > 20,
            "l'aide devrait avoir de nombreux titres"
        );
        let lus: Vec<(u8, String)> = s.iter().map(|x| (x.niveau, x.titre.clone())).collect();
        assert_eq!(lus, attendus);
        assert_eq!(s[0].titre, "Aide de Pupitre");
        assert_eq!(s[0].niveau, 1);
        assert_eq!(s[1].titre, "Sommaire");
        assert!(s
            .iter()
            .any(|x| x.titre == "2. Raccourcis clavier" && x.niveau == 2));
        assert!(s
            .iter()
            .any(|x| x.titre == "Copier et coller" && x.niveau == 3));
        // Aucun texte perdu : titres et textes remis bout à bout redonnent tout le
        // contenu, hors marques « # » des titres.
        let mut tout = String::new();
        for x in &s {
            tout.push_str(&x.titre);
            tout.push('\n');
            tout.push_str(&x.texte);
            tout.push('\n');
        }
        let mut dans_code = false;
        let mut sans_marques = String::new();
        for l in a.lines() {
            if l.trim_start().starts_with("```") {
                dans_code = !dans_code;
            }
            let titre = !dans_code && ["# ", "## ", "### "].iter().any(|m| l.starts_with(m));
            sans_marques.push_str(if titre { l.trim_start_matches('#') } else { l });
            sans_marques.push('\n');
        }
        let normal = |t: &str| t.split_whitespace().collect::<Vec<_>>().join(" ");
        assert_eq!(normal(&tout), normal(&sans_marques));
    }

    #[test]
    fn titres_dans_un_bloc_de_code_ignores() {
        let s = sections("# A\ntexte\n```sh\n# commentaire\n```\n## B\nfin\n");
        let titres: Vec<&str> = s.iter().map(|x| x.titre.as_str()).collect();
        assert_eq!(titres, ["A", "B"]);
        assert!(s[0].texte.contains("# commentaire"));
    }

    #[test]
    fn tableau_lisible() {
        assert_eq!(
            ligne_tableau("| Nouvel onglet | Ctrl+Maj+T |").as_deref(),
            Some("Nouvel onglet : Ctrl+Maj+T")
        );
        assert_eq!(ligne_tableau("|---|---|"), None);
        assert_eq!(ligne_tableau("pas un tableau"), None);
    }

    fn plat(texte: &str) -> String {
        rendre(texte).iter().map(|s| s.texte.as_str()).collect()
    }

    #[test]
    fn mise_en_forme() {
        let r = rendre("Un **mot** et `code` ici\n\n| Action | Raccourci |\n|---|---|\n| Copier | Ctrl+Maj+C |\n\n```toml\n# x\n```\n---\n");
        assert_eq!(
            r[..4],
            [
                Segment {
                    texte: "Un ".into(),
                    style: Style::Normal
                },
                Segment {
                    texte: "mot".into(),
                    style: Style::Gras
                },
                Segment {
                    texte: " et ".into(),
                    style: Style::Normal
                },
                Segment {
                    texte: "code".into(),
                    style: Style::Code
                },
            ]
        );
        assert_eq!(
            plat("a\n| Action | Raccourci |\n|---|---|\n| Copier | Ctrl+Maj+C |\n"),
            "a\nCopier : Ctrl+Maj+C\n"
        );
        assert_eq!(plat("```toml\n# x\n```\n---\nfin"), "# x\nfin\n");
        // Plus de deux colonnes : les colonnes nommées par leur en-tête.
        assert_eq!(
            plat("| Clé | Défaut | Valeurs |\n|---|---|---|\n| `a` | `1` | 0 à 9 |\n"),
            "a : Défaut 1 ; Valeurs 0 à 9\n"
        );
        assert_eq!(plat("2 * 3 = 6"), "2 * 3 = 6\n");
    }

    #[test]
    fn aide_en_texte_simple() {
        let t = texte_brut(aide_fr());
        assert!(t.contains("Raccourcis clavier"));
        assert!(t.contains("Nouvel onglet : Ctrl+Maj+T"));
        assert!(!t.contains("|---"));
        assert!(!t.contains("**"));
    }

    #[test]
    fn recherche_sans_accents_ni_casse() {
        let s = sections(aide_fr());
        let tous = filtrer(&s, "  ");
        assert_eq!(tous.len(), s.len());
        let r = filtrer(&s, "DEMARRER");
        assert!(r.iter().any(|&i| s[i].titre == "1. Démarrer"));
        assert!(filtrer(&s, "zzzzqqqq").is_empty());
    }
}
