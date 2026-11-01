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
const DELAI_ATTENTE: Duration = Duration::from_millis(250);
/// Octets en attente au-delà desquels les marques sont abandonnées pour ne pas retenir la
/// sortie (mémoire bornée).
const FILE_MAX_OCTETS: usize = 8 * 1024 * 1024;
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
/// ne devient jamais « active » : toute fenêtre est alors tenue pour active.
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
                    // Fin sans exécution connue (marque C perdue) : annoncée si le fond a
                    // mesuré une durée ; sinon c'est l'invite initiale d'un shell (D sans
                    // commande), rien à dire.
                    None => duree_ms.map(|d| (code, Some(d))),
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
}

/// File ordonnée des octets et des marques d'un onglet, avec les lignes des commandes.
pub struct Flux {
    terminal: vte4::Terminal,
    donner: Donneur,
    etat: RefCell<Etat>,
    marques: RefCell<MarquesOnglet>,
    sur_fin: RefCell<Option<RappelFin>>,
    colonnes: Cell<i64>,
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
            colonnes: Cell::new(0),
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

    /// La largeur de la grille change : VTE réagence les lignes et les lignes notées ne
    /// valent plus ; les commandes connues sont oubliées.
    pub fn grille_changee(&self, colonnes: i64) {
        if self.colonnes.replace(colonnes) != colonnes {
            self.marques.borrow_mut().vider();
        }
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
        // Terminal pas encore affiché : VTE ne traite pas ses octets, inutile d'attendre.
        if !self.terminal.is_mapped() {
            match genre {
                Genre::Marque(m, rejouee) => {
                    let (col, ligne) = self.terminal.cursor_position();
                    self.noter(m, Pos { ligne, col }, rejouee);
                }
                Genre::Calibrage(f) => f(self, None),
            }
            return false;
        }
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
        self.terminal.feed(&data);
        let faible = self.soi.borrow().clone();
        gtk::glib::timeout_add_local_once(DELAI_ATTENTE, move || {
            if let Some(f) = faible.upgrade() {
                f.repere_expire(repere);
            }
        });
        true
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
        if en_cours {
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
        let mut e = self.etat.borrow_mut();
        let Some(a) = e.attente.as_mut() else {
            return false;
        };
        if !matches!(a.genre, Genre::Calibrage(_)) || a.reponse.is_some() {
            return false;
        }
        match ligne_de_reponse(texte) {
            Some(l) => {
                a.reponse = Some(l);
                true
            }
            None => false,
        }
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
        let (texte, _) = self.terminal.text_range_format(
            vte4::Format::Text,
            debut.ligne,
            debut.col,
            fin.ligne,
            fin.col,
        );
        let texte = texte.map(|t| t.to_string()).unwrap_or_default();
        texte.split_whitespace().collect::<Vec<_>>().join(" ")
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
