// SPDX-License-Identifier: GPL-3.0-or-later
//! Commandes d'un onglet : annonce de leur fin au lecteur d'écran et navigation de
//! commande en commande.
//!
//! Le processus de fond transmet les marques OSC 133 (A invite, B saisie, C exécution,
//! D fin) dans l'ordre exact du flux. Pour savoir où en était le curseur de VTE à chaque
//! marque, les octets qui la précèdent sont donnés à VTE, puis une séquence repère
//! (`OSC 666`, que VTE signale quand il l'a lue) attend qu'il les ait traités : VTE
//! interprète ce qu'on lui donne plus tard, pas dans l'appel à `feed`. Entre-temps les
//! octets et marques suivants attendent dans une file. Les lignes sont celles de VTE,
//! absolues (`cursor_position`).
//!
//! Pour défiler, il faut la ligne relative au haut de l'historique actuel : elle est
//! calibrée au moment de la navigation, par une demande de position du curseur (`CSI 6 n`)
//! dont la réponse est gardée pour soi au lieu d'aller au shell.

use pupitre_config::AnnonceFin;
use pupitre_i18n::{t as tr, ta, ArgI18n};
use pupitre_proto::MarqueCommande;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::{Rc, Weak};
use std::time::Duration;
use vte4::prelude::*;

/// Durée à partir de laquelle une commande est « longue » (réglage `longues`).
pub const SEUIL_LONGUE_MS: u64 = 5_000;
/// Délai après lequel une attente de VTE est abandonnée (VTE ne répond jamais : on ne
/// bloque pas pour autant les octets qui suivent).
const DELAI_ATTENTE: Duration = Duration::from_millis(1000);
/// Octets en attente au-delà desquels les marques sont abandonnées pour ne pas retenir la
/// sortie (mémoire bornée).
const FILE_MAX_OCTETS: usize = 8 * 1024 * 1024;
/// Lignes de terminal lues au plus pour le texte d'une commande.
const LIGNES_LUES_MAX: i64 = 200;
/// Propriété de terminal servant de repère (valeur : le numéro du repère).
const REPERE: &str = "vte.shell.postexec";

/// Durée lisible : « 12 s », « 2 min 5 s », « moins d'une seconde ».
pub fn duree_lisible(duree_ms: u64) -> String {
    let s = duree_ms / 1000;
    let nombre = |n: u64| ArgI18n::from(i64::try_from(n).unwrap_or(i64::MAX));
    match s {
        0 => tr("duree-moins-seconde"),
        1..=59 => ta("duree-secondes", &[("s", nombre(s))]),
        _ if s % 60 == 0 => ta("duree-minutes-rondes", &[("m", nombre(s / 60))]),
        _ => ta(
            "duree-minutes",
            &[("m", nombre(s / 60)), ("s", nombre(s % 60))],
        ),
    }
}

thread_local! {
    static ACTIVE_FORCEE: Cell<bool> = const { Cell::new(false) };
}

/// Pour les tests sous un compositeur sans clavier (Weston sans écran), où aucune fenêtre
/// ne devient jamais « active » : toute fenêtre est alors tenue pour active. Réservé aux tests.
#[doc(hidden)]
pub fn forcer_fenetre_active(oui: bool) {
    ACTIVE_FORCEE.with(|a| a.set(oui));
}

/// Vrai si la fenêtre `active` l'est vraiment (ou si les tests l'ont forcé).
pub fn fenetre_active(active: bool) -> bool {
    active || ACTIVE_FORCEE.with(Cell::get)
}

/// Phrase annoncée à la fin d'une commande.
pub fn texte_annonce(code: Option<i32>, duree_ms: Option<u64>) -> String {
    let echec = code.filter(|&c| c != 0);
    match (echec, duree_ms) {
        (None, None) => tr("commande-terminee"),
        (None, Some(d)) => ta(
            "commande-terminee-duree",
            &[("duree", ArgI18n::from(duree_lisible(d)))],
        ),
        (Some(c), None) => ta("commande-echec", &[("code", ArgI18n::from(i64::from(c)))]),
        (Some(c), Some(d)) => ta(
            "commande-echec-duree",
            &[
                ("code", ArgI18n::from(i64::from(c))),
                ("duree", ArgI18n::from(duree_lisible(d))),
            ],
        ),
    }
}

/// Vrai si une commande finie doit être annoncée avec ce réglage.
pub fn doit_annoncer(mode: AnnonceFin, duree_ms: Option<u64>) -> bool {
    match mode {
        AnnonceFin::Aucune => false,
        AnnonceFin::Toutes => true,
        AnnonceFin::Longues => duree_ms.is_some_and(|d| d >= SEUIL_LONGUE_MS),
    }
}

/// Position dans VTE : ligne absolue et colonne.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Pos {
    pub ligne: i64,
    pub col: i64,
}

/// Une commande repérée dans le flux.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commande {
    pub numero: u64,
    pub invite: Option<Pos>,
    pub saisie: Option<Pos>,
    pub execution: Option<Pos>,
    /// `Some(code)` quand la fin est connue.
    pub fin: Option<Option<i32>>,
}

impl Commande {
    /// Ligne où commence la commande (invite, à défaut saisie ou exécution).
    pub fn ligne(&self) -> Option<i64> {
        self.invite
            .or(self.saisie)
            .or(self.execution)
            .map(|p| p.ligne)
    }

    /// L'invite est affichée mais rien n'a été lancé.
    fn en_saisie(&self) -> bool {
        self.execution.is_none() && self.fin.is_none()
    }

    /// Code de sortie d'une commande en échec.
    pub fn echec(&self) -> Option<i32> {
        self.fin.flatten().filter(|&c| c != 0)
    }
}

/// Sens de la navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sens {
    Precedente,
    Suivante,
}

/// Lignes VTE absolues des commandes d'un onglet. Bornée en nombre de commandes ; les
/// lignes sorties de l'historique de VTE sont oubliées (`oublier_avant`).
#[derive(Debug)]
pub struct MarquesOnglet {
    commandes: VecDeque<Commande>,
    prochain: u64,
    capacite: usize,
    /// Commande visée par la dernière navigation (`None` : on est en bas).
    courant: Option<u64>,
}

impl MarquesOnglet {
    pub fn new(capacite: usize) -> Self {
        MarquesOnglet {
            commandes: VecDeque::new(),
            prochain: 1,
            capacite: capacite.max(1),
            courant: None,
        }
    }

    pub fn definir_capacite(&mut self, capacite: usize) {
        self.capacite = capacite.max(1);
        self.borner();
    }

    pub fn len(&self) -> usize {
        self.commandes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.commandes.is_empty()
    }

    pub fn vider(&mut self) {
        self.commandes.clear();
        self.courant = None;
    }

    fn borner(&mut self) {
        while self.commandes.len() > self.capacite {
            self.commandes.pop_front();
        }
    }

    /// Ajoute une commande, que `remplir` renseigne.
    fn nouvelle(&mut self, remplir: impl FnOnce(&mut Commande)) {
        let numero = self.prochain;
        self.prochain += 1;
        let mut c = Commande {
            numero,
            invite: None,
            saisie: None,
            execution: None,
            fin: None,
        };
        remplir(&mut c);
        self.commandes.push_back(c);
        self.borner();
    }

    /// Enregistre une marque vue à `pos`. Rend `(code, durée)` quand une commande vient de
    /// se terminer (à annoncer selon le réglage), `None` sinon.
    ///
    /// Une marque A ou B répétée avant C met à jour l'invite courante (nushell redessine
    /// l'invite à chaque frappe) ; une marque qui manque (invite absente après `exec`, shell
    /// imbriqué, débit borné du fond) donne une commande moins complète, jamais une erreur.
    pub fn appliquer(
        &mut self,
        marque: MarqueCommande,
        pos: Pos,
    ) -> Option<(Option<i32>, Option<u64>)> {
        match marque {
            MarqueCommande::Invite => {
                self.courant = None;
                if self.commandes.back().is_some_and(Commande::en_saisie) {
                    if let Some(c) = self.commandes.back_mut() {
                        c.invite = Some(pos);
                        c.saisie = None;
                    }
                } else {
                    self.nouvelle(|c| c.invite = Some(pos));
                }
                None
            }
            MarqueCommande::Saisie => {
                self.courant = None;
                if self.commandes.back().is_some_and(Commande::en_saisie) {
                    if let Some(c) = self.commandes.back_mut() {
                        c.saisie = Some(pos);
                    }
                } else {
                    self.nouvelle(|c| c.saisie = Some(pos));
                }
                None
            }
            MarqueCommande::Execution => {
                self.courant = None;
                if self.commandes.back().is_some_and(Commande::en_saisie) {
                    if let Some(c) = self.commandes.back_mut() {
                        c.execution = Some(pos);
                    }
                } else {
                    self.nouvelle(|c| c.execution = Some(pos));
                }
                None
            }
            MarqueCommande::Fin { code, duree_ms } => {
                // La dernière commande lancée et pas encore finie.
                let ouverte = self
                    .commandes
                    .iter_mut()
                    .rev()
                    .find(|c| c.execution.is_some() && c.fin.is_none());
                match ouverte {
                    Some(c) => {
                        c.fin = Some(code);
                        Some((code, duree_ms))
                    }
                    // Fin sans exécution connue (marque C perdue) : l'invite affichée a bien
                    // été validée, la commande est close pour que l'invite suivante en
                    // ouvre une autre. Annoncée si le fond a mesuré une durée ; sinon c'est
                    // l'invite initiale d'un shell (D sans commande), rien à dire.
                    None => {
                        self.courant = None;
                        if let Some(c) = self.commandes.back_mut().filter(|c| c.en_saisie()) {
                            c.fin = Some(code);
                        }
                        duree_ms.map(|d| (code, Some(d)))
                    }
                }
            }
        }
    }

    /// Commandes où l'on peut aller : celles qui ont été lancées, plus toute commande
    /// ancienne dont la marque C a manqué ; pas l'invite du bas, où l'on est déjà.
    fn navigables(&self) -> impl Iterator<Item = &Commande> {
        let n = self.commandes.len();
        self.commandes
            .iter()
            .enumerate()
            .filter(move |(i, c)| c.execution.is_some() || i + 1 < n)
            .map(|(_, c)| c)
    }

    /// Toutes les positions notées, dans un ordre fixe (commande par commande : invite,
    /// saisie, exécution).
    pub fn positions(&self) -> Vec<Pos> {
        self.commandes
            .iter()
            .flat_map(|c| [c.invite, c.saisie, c.execution])
            .flatten()
            .collect()
    }

    /// Remplace chaque position, dans l'ordre de `positions` ; `None` : position perdue.
    /// Une commande qui n'a plus aucune position est oubliée.
    pub fn remplacer_positions(&mut self, mut f: impl FnMut(Pos) -> Option<Pos>) {
        for c in &mut self.commandes {
            for p in [&mut c.invite, &mut c.saisie, &mut c.execution] {
                if let Some(q) = *p {
                    *p = f(q);
                }
            }
        }
        self.commandes
            .retain(|c| c.invite.is_some() || c.saisie.is_some() || c.execution.is_some());
    }

    /// Passe à la commande précédente ou suivante ; `None` au bout de la liste.
    pub fn deplacer(&mut self, sens: Sens) -> Option<Commande> {
        let cible = match (sens, self.courant) {
            (Sens::Precedente, None) => self.navigables().last(),
            (Sens::Precedente, Some(n)) => self.navigables().filter(|c| c.numero < n).last(),
            (Sens::Suivante, None) => None,
            (Sens::Suivante, Some(n)) => self.navigables().find(|c| c.numero > n),
        }
        .cloned()?;
        self.courant = Some(cible.numero);
        Some(cible)
    }

    /// Oublie les commandes dont la première ligne est sortie de l'historique de VTE
    /// (ligne absolue inférieure à `premiere`).
    pub fn oublier_avant(&mut self, premiere: i64) {
        while self
            .commandes
            .front()
            .is_some_and(|c| c.ligne().is_none_or(|l| l < premiere))
        {
            self.commandes.pop_front();
        }
    }

    /// Début et fin du texte saisi de la commande `numero` : de la fin de l'invite (marque
    /// B, sinon début de la ligne de l'invite) jusqu'à l'exécution (marque C), à défaut le
    /// début de la commande suivante ; `None` : jusqu'au curseur.
    pub fn plage_texte(&self, numero: u64) -> Option<(Pos, Option<Pos>)> {
        let i = self.commandes.iter().position(|c| c.numero == numero)?;
        let c = &self.commandes[i];
        let debut = c.saisie.or(c.invite.map(|p| Pos {
            ligne: p.ligne,
            col: 0,
        }))?;
        let fin = c.execution.or_else(|| {
            self.commandes
                .get(i + 1)
                .and_then(|n| n.invite.or(n.saisie).or(n.execution))
        });
        Some((debut, fin))
    }
}

/// Résultat d'une navigation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Navigation {
    /// Texte de la commande atteinte et code d'échec éventuel.
    Commande { texte: String, echec: Option<i32> },
    /// Plus de commande dans ce sens.
    Bout(Sens),
}

impl Navigation {
    /// Phrase à annoncer.
    pub fn texte(&self) -> String {
        match self {
            Navigation::Bout(Sens::Precedente) => tr("commande-aucune-avant"),
            Navigation::Bout(Sens::Suivante) => tr("commande-aucune-apres"),
            Navigation::Commande { texte, echec } => {
                let code = echec.map(|c| ArgI18n::from(i64::from(c)));
                match (texte.is_empty(), code) {
                    (true, None) => tr("commande-vide"),
                    (true, Some(c)) => ta("commande-vide-echec", &[("code", c)]),
                    (false, None) => ta(
                        "commande-navigation",
                        &[("texte", ArgI18n::from(texte.as_str()))],
                    ),
                    (false, Some(c)) => ta(
                        "commande-navigation-echec",
                        &[("texte", ArgI18n::from(texte.as_str())), ("code", c)],
                    ),
                }
            }
        }
    }
}

/// Correspondance entre lignes absolues de VTE et valeurs de son ajustement vertical.
#[derive(Debug, Clone, Copy)]
struct Calibrage {
    /// Première ligne absolue encore dans l'historique.
    premiere: i64,
}

/// Rangée (`g`) de la ligne logique d'une position, comptée à partir de celle du curseur, et
/// décalage en cellules dans cette ligne logique : ne dépend pas de la largeur.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PosLogique {
    g: usize,
    decalage: i64,
}

/// Positions notées avant un réagencement de VTE (changement de largeur).
struct Instantane {
    positions: Vec<Option<PosLogique>>,
    g_max: usize,
}

/// Première ligne de chaque ligne logique en remontant du curseur : `debuts[g]` pour la
/// ligne logique du curseur (`g = 0`), la précédente (`g = 1`)… `dur(r)` dit si un vrai retour
/// à la ligne sépare les lignes `r - 1` et `r`. S'arrête à la ligne `borne` ou quand
/// `debuts[g_max]` est complet.
fn debuts_logiques(curseur: i64, borne: i64, g_max: usize, dur: &dyn Fn(i64) -> bool) -> Vec<i64> {
    let mut debuts = vec![curseur];
    let mut r = curseur;
    while r > borne.max(0) {
        let p = r - 1;
        if dur(r) {
            if debuts.len() > g_max {
                break;
            }
            debuts.push(p);
        } else if let Some(d) = debuts.last_mut() {
            *d = p;
        }
        r = p;
    }
    debuts
}

/// Position logique de `pos`, ou `None` si elle est hors de la zone connue.
fn vers_logique(pos: Pos, curseur: i64, debuts: &[i64], colonnes: i64) -> Option<PosLogique> {
    if pos.ligne > curseur {
        return None;
    }
    // La plus petite rangée dont la première ligne ne dépasse pas la position.
    let g = debuts.iter().position(|&d| d <= pos.ligne)?;
    let debut = *debuts.get(g)?;
    Some(PosLogique {
        g,
        decalage: (pos.ligne - debut) * colonnes + pos.col,
    })
}

fn depuis_logique(l: PosLogique, debuts: &[i64], colonnes: i64) -> Option<Pos> {
    let debut = *debuts.get(l.g)?;
    let colonnes = colonnes.max(1);
    Some(Pos {
        ligne: debut + l.decalage / colonnes,
        col: l.decalage % colonnes,
    })
}

type RappelCalibrage = Box<dyn FnOnce(&Flux, Option<Calibrage>)>;
/// Remet des octets à VTE.
type Donneur = Box<dyn Fn(&[u8])>;
type RappelFin = Rc<dyn Fn(Option<i32>, Option<u64>)>;

enum Travail {
    Octets(Vec<u8>),
    Marque(MarqueCommande, bool),
    Calibrage(RappelCalibrage),
}

enum Genre {
    Marque(MarqueCommande, bool),
    Calibrage(RappelCalibrage),
}

struct Attente {
    repere: u32,
    genre: Genre,
    /// Ligne (à partir de 1) de la réponse à la demande de position du curseur.
    reponse: Option<i64>,
}

#[derive(Default)]
struct Etat {
    file: VecDeque<Travail>,
    octets_en_file: usize,
    attente: Option<Attente>,
    repere: u32,
    derniere_pos: Option<Pos>,
    /// Des octets sont arrivés depuis la dernière position relevée.
    octets_depuis_pos: bool,
    /// Demandes de position du curseur dont la réponse n'est pas encore revenue (repères,
    /// dans l'ordre) : la réponse est gardée pour soi même si l'attente a expiré.
    reponses_attendues: VecDeque<u32>,
    reponses_gardees: u32,
}

/// File ordonnée des octets et des marques d'un onglet, avec les lignes des commandes.
pub struct Flux {
    terminal: vte4::Terminal,
    donner: Donneur,
    etat: RefCell<Etat>,
    marques: RefCell<MarquesOnglet>,
    sur_fin: RefCell<Option<RappelFin>>,
    delai: Cell<Duration>,
    instantane: RefCell<Option<Instantane>>,
    soi: RefCell<Weak<Flux>>,
}

impl Flux {
    /// `donner` remet des octets à VTE (après le filtre d'affichage de l'onglet).
    pub fn new(terminal: vte4::Terminal, donner: Donneur) -> Rc<Flux> {
        let f = Rc::new(Flux {
            terminal,
            donner,
            etat: RefCell::new(Etat::default()),
            marques: RefCell::new(MarquesOnglet::new(10_000)),
            sur_fin: RefCell::new(None),
            delai: Cell::new(DELAI_ATTENTE),
            instantane: RefCell::new(None),
            soi: RefCell::new(Weak::new()),
        });
        *f.soi.borrow_mut() = Rc::downgrade(&f);
        let faible = Rc::downgrade(&f);
        f.terminal
            .connect_termprop_changed(Some(REPERE), move |t, _| {
                if let Some(f) = faible.upgrade() {
                    let valeur = t.termprop_uint(REPERE);
                    f.repere_atteint(valeur);
                }
            });
        let faible = Rc::downgrade(&f);
        f.terminal.connect_map(move |_| {
            if let Some(f) = faible.upgrade() {
                f.affiche();
            }
        });
        f
    }

    pub fn definir_capacite(&self, capacite: usize) {
        self.marques.borrow_mut().definir_capacite(capacite);
    }

    pub fn nombre_commandes(&self) -> usize {
        self.marques.borrow().len()
    }

    /// Appelé à la fin de chaque commande en direct (jamais pour une marque rejouée).
    pub fn connecter_fin(&self, f: impl Fn(Option<i32>, Option<u64>) + 'static) {
        *self.sur_fin.borrow_mut() = Some(Rc::new(f));
    }

    /// Vrai retour à la ligne (et non repli de VTE) entre les lignes `r - 1` et `r`.
    fn retour_dur(&self, r: i64) -> bool {
        let (t, _) = self
            .terminal
            .text_range_format(vte4::Format::Text, r - 1, 0, r, 0);
        t.is_some_and(|t| t.contains('\n'))
    }

    /// À appeler juste avant que VTE réagence ses lignes (largeur de la grille changée) :
    /// note les positions en lignes logiques.
    pub fn avant_reagencement(&self) {
        let positions = self.marques.borrow().positions();
        let colonnes = self.terminal.column_count();
        let (_, curseur) = self.terminal.cursor_position();
        if positions.is_empty() || colonnes <= 0 {
            *self.instantane.borrow_mut() = None;
            return;
        }
        let haut = positions.iter().map(|p| p.ligne).min().unwrap_or(curseur);
        let debuts = debuts_logiques(curseur, haut, usize::MAX, &|r| self.retour_dur(r));
        let logiques: Vec<Option<PosLogique>> = positions
            .iter()
            .map(|&p| vers_logique(p, curseur, &debuts, colonnes))
            .collect();
        let g_max = logiques.iter().flatten().map(|l| l.g).max().unwrap_or(0);
        *self.instantane.borrow_mut() = Some(Instantane {
            positions: logiques,
            g_max,
        });
        // Les positions relevées plus tard ne dépendent pas de celles-ci.
        let mut e = self.etat.borrow_mut();
        e.derniere_pos = None;
    }

    /// À appeler juste après le réagencement : retrouve les lignes des positions notées.
    pub fn apres_reagencement(&self) {
        let Some(inst) = self.instantane.borrow_mut().take() else {
            return;
        };
        let colonnes = self.terminal.column_count();
        let (_, curseur) = self.terminal.cursor_position();
        let debuts = debuts_logiques(curseur, 0, inst.g_max, &|r| self.retour_dur(r));
        let mut suivantes = inst.positions.iter();
        self.marques.borrow_mut().remplacer_positions(|_| {
            suivantes
                .next()
                .copied()
                .flatten()
                .and_then(|l| depuis_logique(l, &debuts, colonnes))
        });
    }

    /// Octets reçus du shell, dans l'ordre.
    pub fn octets(&self, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        let direct = {
            let e = self.etat.borrow();
            e.attente.is_none() && e.file.is_empty()
        };
        if direct {
            self.donner_octets(data);
            return;
        }
        {
            let mut e = self.etat.borrow_mut();
            e.octets_en_file += data.len();
            e.file.push_back(Travail::Octets(data.to_vec()));
        }
        self.borner_file();
    }

    /// Marque reçue du shell, dans l'ordre ; `rejouee` : vue lors d'une reprise.
    pub fn marque(&self, marque: MarqueCommande, rejouee: bool) {
        self.etat
            .borrow_mut()
            .file
            .push_back(Travail::Marque(marque, rejouee));
        self.avancer();
    }

    fn donner_octets(&self, data: &[u8]) {
        self.etat.borrow_mut().octets_depuis_pos = true;
        (self.donner)(data);
    }

    /// Trop d'octets attendent : on cesse d'attendre VTE, les octets passent et les marques
    /// restantes sont perdues.
    fn borner_file(&self) {
        if self.etat.borrow().octets_en_file <= FILE_MAX_OCTETS {
            return;
        }
        let travaux: Vec<Travail> = {
            let mut e = self.etat.borrow_mut();
            e.attente = None;
            e.octets_en_file = 0;
            e.file.drain(..).collect()
        };
        for t in travaux {
            match t {
                Travail::Octets(d) => self.donner_octets(&d),
                Travail::Calibrage(f) => f(self, None),
                Travail::Marque(..) => {}
            }
        }
    }

    /// Traite la file jusqu'à la prochaine attente.
    fn avancer(&self) {
        loop {
            let travail = {
                let mut e = self.etat.borrow_mut();
                if e.attente.is_some() {
                    return;
                }
                let t = e.file.pop_front();
                if let Some(Travail::Octets(d)) = &t {
                    e.octets_en_file = e.octets_en_file.saturating_sub(d.len());
                }
                t
            };
            match travail {
                None => return,
                Some(Travail::Octets(d)) => self.donner_octets(&d),
                Some(Travail::Marque(m, rejouee)) => {
                    let connue = {
                        let e = self.etat.borrow();
                        match (m, e.derniere_pos) {
                            // La fin n'a pas besoin de position ; une marque sans octet
                            // depuis la précédente est à la même place.
                            (MarqueCommande::Fin { .. }, _) => Some(Pos::default()),
                            (_, Some(p)) if !e.octets_depuis_pos => Some(p),
                            _ => None,
                        }
                    };
                    match connue {
                        Some(p) => self.noter(m, p, rejouee),
                        None => {
                            if self.attendre(Genre::Marque(m, rejouee), b"") {
                                return;
                            }
                        }
                    }
                }
                Some(Travail::Calibrage(f)) => {
                    if self.attendre(Genre::Calibrage(f), b"\x1b[6n") {
                        return;
                    }
                }
            }
        }
    }

    /// Donne `avant` (éventuellement vide) puis le repère à VTE et attend qu'il le signale.
    fn attendre(&self, genre: Genre, avant: &[u8]) -> bool {
        let repere = {
            let mut e = self.etat.borrow_mut();
            e.repere = e.repere % 1_000_000_000 + 1;
            e.attente = Some(Attente {
                repere: e.repere,
                genre,
                reponse: None,
            });
            e.repere
        };
        let mut data = avant.to_vec();
        data.extend_from_slice(format!("\x1b]666;{REPERE}={repere}\x1b\\").as_bytes());
        if avant.starts_with(b"\x1b[6n") {
            self.etat.borrow_mut().reponses_attendues.push_back(repere);
        }
        self.terminal.feed(&data);
        self.armer_delai(repere);
        true
    }

    /// Délai de secours : si VTE n'a pas signalé le repère, on reprend quand même. Un
    /// terminal non affiché ne traite rien : le délai ne court qu'une fois affiché.
    fn armer_delai(&self, repere: u32) {
        if !self.terminal.is_mapped() {
            return;
        }
        let faible = self.soi.borrow().clone();
        gtk::glib::timeout_add_local_once(self.delai.get(), move || {
            if let Some(f) = faible.upgrade() {
                f.repere_expire(repere);
            }
        });
    }

    /// Le terminal vient d'être affiché : le délai de l'attente en cours démarre.
    fn affiche(&self) {
        let repere = self.etat.borrow().attente.as_ref().map(|a| a.repere);
        if let Some(r) = repere {
            self.armer_delai(r);
        }
    }

    /// Change le délai de secours (tests).
    #[doc(hidden)]
    pub fn definir_delai_attente(&self, delai: Duration) {
        self.delai.set(delai);
    }

    /// Nombre de réponses de position gardées pour soi depuis la création (tests).
    #[doc(hidden)]
    pub fn reponses_gardees(&self) -> u32 {
        self.etat.borrow().reponses_gardees
    }

    fn repere_atteint(&self, valeur: Option<u64>) {
        let attendu = self.etat.borrow().attente.as_ref().map(|a| a.repere);
        match (attendu, valeur) {
            (Some(a), Some(v)) if u64::from(a) == v => self.resoudre(),
            // Valeur illisible : le seul repère en attente est le nôtre.
            (Some(_), None) => self.resoudre(),
            _ => {}
        }
    }

    fn repere_expire(&self, repere: u32) {
        let en_cours = self
            .etat
            .borrow()
            .attente
            .as_ref()
            .is_some_and(|a| a.repere == repere);
        if en_cours && self.terminal.is_mapped() {
            self.resoudre();
        }
    }

    /// VTE a lu tout ce qu'on lui a donné : relève la position (ou calibre) et reprend.
    fn resoudre(&self) {
        let attente = self.etat.borrow_mut().attente.take();
        let Some(a) = attente else { return };
        match a.genre {
            Genre::Marque(m, rejouee) => {
                let (col, ligne) = self.terminal.cursor_position();
                let pos = Pos { ligne, col };
                {
                    let mut e = self.etat.borrow_mut();
                    e.derniere_pos = Some(pos);
                    e.octets_depuis_pos = false;
                }
                self.noter(m, pos, rejouee);
            }
            Genre::Calibrage(f) => {
                let calibrage = a.reponse.and_then(|r| self.calibrer_avec(r));
                f(self, calibrage);
            }
        }
        self.avancer();
    }

    /// La ligne de l'écran où est le curseur (`reponse`, à partir de 1) et sa ligne absolue
    /// donnent la ligne absolue du haut de l'écran ; l'ajustement donne combien de lignes
    /// d'historique le précèdent.
    fn calibrer_avec(&self, reponse: i64) -> Option<Calibrage> {
        let (_, curseur) = self.terminal.cursor_position();
        let ajustement = self.terminal.vadjustment()?;
        let lignes = self.terminal.row_count();
        // Valeurs de VTE : petits entiers stockés en flottants.
        #[allow(clippy::cast_possible_truncation)]
        let haut_relatif = ajustement.upper() as i64 - lignes;
        let haut_absolu = curseur - (reponse - 1);
        Some(Calibrage {
            premiere: haut_absolu - haut_relatif.max(0),
        })
    }

    fn noter(&self, m: MarqueCommande, pos: Pos, rejouee: bool) {
        let fin = self.marques.borrow_mut().appliquer(m, pos);
        if let (Some((code, duree)), false) = (fin, rejouee) {
            let rappel = self.sur_fin.borrow().clone();
            if let Some(r) = rappel {
                r(code, duree);
            }
        }
    }

    /// Réponse de VTE à notre demande de position du curseur : vraie (et gardée) si elle
    /// est attendue, sinon c'est une réponse du programme, qui doit aller au shell.
    pub fn garder_reponse(&self, texte: &str) -> bool {
        let Some(ligne) = ligne_de_reponse(texte) else {
            return false;
        };
        let mut e = self.etat.borrow_mut();
        // Les réponses reviennent dans l'ordre des demandes ; une réponse tardive (attente
        // expirée) est gardée aussi, sans quoi elle partirait au programme comme une frappe.
        let Some(repere) = e.reponses_attendues.pop_front() else {
            return false;
        };
        e.reponses_gardees += 1;
        if let Some(a) = e.attente.as_mut() {
            if a.repere == repere && matches!(a.genre, Genre::Calibrage(_)) {
                a.reponse = Some(ligne);
            }
        }
        true
    }

    /// Va à la commande précédente ou suivante : défile pour mettre sa ligne en haut puis
    /// appelle `fini` avec ce qu'il faut annoncer (`None` : navigation impossible).
    pub fn naviguer(&self, sens: Sens, fini: impl FnOnce(Option<Navigation>) + 'static) {
        let rappel: RappelCalibrage = Box::new(move |flux, calibrage| {
            fini(calibrage.map(|c| flux.aller(sens, c)));
        });
        self.etat
            .borrow_mut()
            .file
            .push_back(Travail::Calibrage(rappel));
        self.avancer();
    }

    fn aller(&self, sens: Sens, calibrage: Calibrage) -> Navigation {
        let (cible, plage) = {
            let mut m = self.marques.borrow_mut();
            m.oublier_avant(calibrage.premiere);
            match m.deplacer(sens) {
                Some(c) => {
                    let plage = m.plage_texte(c.numero);
                    (c, plage)
                }
                None => return Navigation::Bout(sens),
            }
        };
        if let (Some(ligne), Some(ajustement)) = (cible.ligne(), self.terminal.vadjustment()) {
            let relative = (ligne - calibrage.premiere) as f64;
            let max = (ajustement.upper() - ajustement.page_size()).max(ajustement.lower());
            ajustement.set_value(relative.clamp(ajustement.lower(), max));
        }
        let texte = plage.map(|(d, f)| self.lire(d, f)).unwrap_or_default();
        Navigation::Commande {
            texte,
            echec: cible.echec(),
        }
    }

    /// Texte de `debut` à `fin` (exclue) ou au curseur, sur une seule ligne.
    fn lire(&self, debut: Pos, fin: Option<Pos>) -> String {
        let fin = fin.unwrap_or_else(|| {
            let (col, ligne) = self.terminal.cursor_position();
            Pos { ligne, col }
        });
        if (fin.ligne, fin.col) <= (debut.ligne, debut.col) {
            return String::new();
        }
        // Sans marque C, `fin` peut être très loin (toute la sortie de la commande) : on ne
        // lit pas plus de quelques lignes.
        let fin = if fin.ligne > debut.ligne + LIGNES_LUES_MAX {
            Pos {
                ligne: debut.ligne + LIGNES_LUES_MAX,
                col: 0,
            }
        } else {
            fin
        };
        let (texte, _) = self.terminal.text_range_format(
            vte4::Format::Text,
            debut.ligne,
            debut.col,
            fin.ligne,
            fin.col,
        );
        let texte = texte.map(|t| t.to_string()).unwrap_or_default();
        // Fin de la ligne logique : le texte n'a de saut de ligne qu'aux vrais retours à la
        // ligne, pas aux lignes repliées par VTE.
        let ligne = texte.lines().next().unwrap_or_default();
        ligne.split_whitespace().collect::<Vec<_>>().join(" ")
    }
}

/// Ligne d'une réponse « CSI ligne ; colonne R ».
fn ligne_de_reponse(texte: &str) -> Option<i64> {
    let corps = texte.strip_prefix("\x1b[")?.strip_suffix('R')?;
    let (ligne, colonne) = corps.split_once(';')?;
    colonne.parse::<i64>().ok()?;
    ligne.parse::<i64>().ok().filter(|&l| l >= 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use MarqueCommande::{Execution, Fin, Invite, Saisie};

    fn p(ligne: i64, col: i64) -> Pos {
        Pos { ligne, col }
    }

    fn fin(code: i32, duree: u64) -> MarqueCommande {
        Fin {
            code: Some(code),
            duree_ms: Some(duree),
        }
    }

    /// Une commande complète commençant à la ligne `l`.
    fn commande(m: &mut MarquesOnglet, l: i64) {
        m.appliquer(Invite, p(l, 0));
        m.appliquer(Saisie, p(l, 2));
        m.appliquer(Execution, p(l + 1, 0));
        m.appliquer(fin(0, 10), p(l + 2, 0));
    }

    #[test]
    fn texte_annonce_cas() {
        let attendu = |c, d, t: &str| assert_eq!(texte_annonce(c, d), t, "{c:?} {d:?}");
        attendu(Some(0), Some(3000), "Commande terminée en 3 s.");
        attendu(Some(0), Some(125_000), "Commande terminée en 2 min 5 s.");
        attendu(Some(0), None, "Commande terminée.");
        attendu(Some(2), Some(3000), "Commande en échec, code 2, en 3 s.");
        attendu(
            Some(2),
            Some(125_000),
            "Commande en échec, code 2, en 2 min 5 s.",
        );
        attendu(Some(2), None, "Commande en échec, code 2.");
        attendu(None, Some(3000), "Commande terminée en 3 s.");
        attendu(None, Some(125_000), "Commande terminée en 2 min 5 s.");
        attendu(None, None, "Commande terminée.");
        attendu(Some(0), Some(12_000), "Commande terminée en 12 s.");
        attendu(Some(0), Some(120_000), "Commande terminée en 2 min.");
        attendu(Some(0), Some(59_999), "Commande terminée en 59 s.");
        attendu(
            Some(0),
            Some(400),
            "Commande terminée en moins d'une seconde.",
        );
    }

    #[test]
    fn regles_d_annonce() {
        use AnnonceFin::{Aucune, Longues, Toutes};
        assert!(doit_annoncer(Longues, Some(5000)));
        assert!(!doit_annoncer(Longues, Some(4999)));
        assert!(!doit_annoncer(Longues, None));
        assert!(doit_annoncer(Toutes, None));
        assert!(doit_annoncer(Toutes, Some(1)));
        assert!(!doit_annoncer(Aucune, Some(60_000)));
    }

    #[test]
    fn fin_rend_code_et_duree() {
        let mut m = MarquesOnglet::new(100);
        m.appliquer(Invite, p(0, 0));
        m.appliquer(Saisie, p(0, 2));
        m.appliquer(Execution, p(1, 0));
        assert_eq!(
            m.appliquer(fin(2, 7000), p(3, 0)),
            Some((Some(2), Some(7000)))
        );
        // Une seconde fin n'a plus de commande ouverte : rien (pas de durée).
        let sans = Fin {
            code: Some(0),
            duree_ms: None,
        };
        assert_eq!(m.appliquer(sans, p(4, 0)), None);
        // Fin sans exécution connue mais avec une durée : annoncée quand même.
        assert_eq!(
            m.appliquer(fin(0, 9000), p(5, 0)),
            Some((Some(0), Some(9000)))
        );
    }

    #[test]
    fn invite_repetee_ne_cree_pas_de_commande() {
        let mut m = MarquesOnglet::new(100);
        for i in 0..5 {
            m.appliquer(Invite, p(10 + i, 0));
            m.appliquer(Saisie, p(10 + i, 2));
        }
        assert_eq!(m.len(), 1);
        m.appliquer(Execution, p(16, 0));
        assert_eq!(m.len(), 1);
        let c = m.deplacer(Sens::Precedente).unwrap();
        assert_eq!(c.ligne(), Some(14), "la dernière invite fait foi");
        assert_eq!(c.saisie, Some(p(14, 2)));
    }

    #[test]
    fn marques_manquantes_toleres() {
        let mut m = MarquesOnglet::new(100);
        // B sans A (shell imbriqué), C sans A ni B, D orphelin.
        m.appliquer(Saisie, p(1, 2));
        m.appliquer(Execution, p(2, 0));
        m.appliquer(fin(0, 10), p(3, 0));
        m.appliquer(Execution, p(5, 0));
        m.appliquer(
            Fin {
                code: None,
                duree_ms: None,
            },
            p(6, 0),
        );
        m.appliquer(
            Fin {
                code: None,
                duree_ms: None,
            },
            p(7, 0),
        );
        assert_eq!(m.len(), 2);
        assert_eq!(m.deplacer(Sens::Precedente).unwrap().ligne(), Some(5));
        assert_eq!(m.deplacer(Sens::Precedente).unwrap().ligne(), Some(1));
        assert!(m.deplacer(Sens::Precedente).is_none());
    }

    #[test]
    fn navigation_dans_les_deux_sens() {
        let mut m = MarquesOnglet::new(100);
        for l in [0, 10, 20] {
            commande(&mut m, l);
        }
        // L'invite du bas n'est pas une cible.
        m.appliquer(Invite, p(30, 0));
        m.appliquer(Saisie, p(30, 2));
        assert!(m.deplacer(Sens::Suivante).is_none());
        let lignes: Vec<_> = (0..4)
            .map(|_| m.deplacer(Sens::Precedente).and_then(|c| c.ligne()))
            .collect();
        assert_eq!(lignes, [Some(20), Some(10), Some(0), None]);
        assert_eq!(m.deplacer(Sens::Suivante).unwrap().ligne(), Some(10));
        assert_eq!(m.deplacer(Sens::Suivante).unwrap().ligne(), Some(20));
        assert!(m.deplacer(Sens::Suivante).is_none());
        // Une nouvelle commande remet la navigation en bas.
        m.appliquer(Execution, p(31, 0));
        assert_eq!(m.deplacer(Sens::Precedente).unwrap().ligne(), Some(30));
    }

    #[test]
    fn historique_et_capacite_bornent() {
        let mut m = MarquesOnglet::new(3);
        for l in 0..10 {
            commande(&mut m, l * 10);
        }
        assert_eq!(m.len(), 3, "capacité");
        m.oublier_avant(75);
        assert_eq!(m.len(), 2);
        m.oublier_avant(1000);
        assert!(m.is_empty());
        assert!(m.deplacer(Sens::Precedente).is_none());
    }

    #[test]
    fn plage_de_texte() {
        let mut m = MarquesOnglet::new(100);
        commande(&mut m, 4);
        let n = m.deplacer(Sens::Precedente).unwrap().numero;
        assert_eq!(m.plage_texte(n), Some((p(4, 2), Some(p(5, 0)))));
        // Sans B : toute la ligne de l'invite.
        let mut m = MarquesOnglet::new(100);
        m.appliquer(Invite, p(8, 0));
        m.appliquer(Execution, p(9, 0));
        let n = m.deplacer(Sens::Precedente).unwrap().numero;
        assert_eq!(m.plage_texte(n), Some((p(8, 0), Some(p(9, 0)))));
    }

    #[test]
    fn fin_sans_c_ferme_la_commande() {
        // A B (C perdue) sortie D A : deux commandes, la première close.
        let mut m = MarquesOnglet::new(100);
        m.appliquer(Invite, p(0, 0));
        m.appliquer(Saisie, p(0, 2));
        m.appliquer(
            Fin {
                code: Some(0),
                duree_ms: None,
            },
            p(40, 0),
        );
        m.appliquer(Invite, p(41, 0));
        assert_eq!(m.len(), 2);
        let n = m.deplacer(Sens::Precedente).unwrap();
        // Sans C, le texte s'arrête au début de la commande suivante (puis à sa ligne).
        assert_eq!(m.plage_texte(n.numero), Some((p(0, 2), Some(p(41, 0)))));
    }

    #[test]
    fn lignes_logiques_traversent_un_reagencement() {
        // Avant : 80 colonnes. Lignes 0-1 : une ligne logique repliée (100 caractères) ;
        // 2 : une ligne ; 3-4 : une ligne repliée ; curseur ligne 5.
        let dur_avant = |r: i64| matches!(r, 2 | 3 | 5);
        let debuts = debuts_logiques(5, 0, usize::MAX, &dur_avant);
        assert_eq!(debuts, [5, 3, 2, 0]);
        let marque = p(1, 20); // 100e cellule de la première ligne logique : 80 + 20
        let l = vers_logique(marque, 5, &debuts, 80).unwrap();
        assert_eq!(
            l,
            PosLogique {
                g: 3,
                decalage: 100
            }
        );
        let l2 = vers_logique(p(4, 10), 5, &debuts, 80).unwrap();
        assert_eq!(l2, PosLogique { g: 1, decalage: 90 });
        // Après : 120 colonnes, plus de repli : la première ligne logique tient sur une ligne.
        // Lignes 0 : première ; 1 : deuxième ; 2 : troisième (3-4 réunies) ; curseur 3.
        let dur_apres = |r: i64| (1..=3).contains(&r);
        let debuts2 = debuts_logiques(3, 0, 3, &dur_apres);
        assert_eq!(debuts2, [3, 2, 1, 0]);
        assert_eq!(
            depuis_logique(l, &debuts2, 120),
            Some(Pos { ligne: 0, col: 100 })
        );
        assert_eq!(
            depuis_logique(l2, &debuts2, 120),
            Some(Pos { ligne: 2, col: 90 })
        );
        // Ligne logique plus ancienne que ce qui est connu : perdue.
        assert_eq!(
            depuis_logique(PosLogique { g: 9, decalage: 0 }, &debuts2, 120),
            None
        );
        // Position après le curseur : pas de ligne logique.
        assert_eq!(vers_logique(p(9, 0), 5, &debuts, 80), None);
    }

    #[test]
    fn remplacement_des_positions() {
        let mut m = MarquesOnglet::new(100);
        commande(&mut m, 0);
        commande(&mut m, 10);
        assert_eq!(m.positions().len(), 6);
        let mut n = 0;
        m.remplacer_positions(|q| {
            n += 1;
            // La première commande est perdue (trois positions), la seconde décalée.
            (n > 3).then_some(Pos {
                ligne: q.ligne + 1,
                col: q.col,
            })
        });
        assert_eq!(m.len(), 1);
        assert_eq!(m.deplacer(Sens::Precedente).unwrap().ligne(), Some(11));
    }

    #[test]
    fn phrases_de_navigation() {
        let c = |t: &str, e| Navigation::Commande {
            texte: t.into(),
            echec: e,
        };
        assert_eq!(c("git fetch", None).texte(), "Commande : git fetch");
        assert_eq!(
            c("git fetch", Some(1)).texte(),
            "Commande : git fetch, en échec, code 1"
        );
        assert_eq!(c("", None).texte(), "Commande vide.");
        assert_eq!(c("", Some(2)).texte(), "Commande vide, en échec, code 2.");
        assert_eq!(
            Navigation::Bout(Sens::Precedente).texte(),
            "Aucune commande avant."
        );
        assert_eq!(
            Navigation::Bout(Sens::Suivante).texte(),
            "Aucune commande après."
        );
    }

    #[test]
    fn reponse_de_position() {
        assert_eq!(ligne_de_reponse("\x1b[12;1R"), Some(12));
        assert_eq!(ligne_de_reponse("\x1b[1;80R"), Some(1));
        assert_eq!(ligne_de_reponse("\x1b[0;1R"), None);
        assert_eq!(ligne_de_reponse("\x1b[?12;1R"), None);
        assert_eq!(ligne_de_reponse("abc"), None);
        assert_eq!(ligne_de_reponse("\x1b[1;R"), None);
    }
}
