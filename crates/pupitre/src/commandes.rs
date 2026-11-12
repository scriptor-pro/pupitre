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
use std::time::{Duration, Instant};
use vte4::prelude::*;

/// Durée à partir de laquelle une commande est « longue » (réglage `longues`).
pub const SEUIL_LONGUE_MS: u64 = 5_000;
/// Délai après lequel une attente de VTE est abandonnée (VTE ne répond jamais : on ne
/// bloque pas pour autant les octets qui suivent).
const DELAI_ATTENTE: Duration = Duration::from_millis(1000);
/// Silence qui met fin à un geste de redimensionnement (les positions sont alors
/// reconverties), et durée maximale d'un geste.
const CALME: Duration = Duration::from_millis(200);
const GESTE_MAX: Duration = Duration::from_secs(2);
/// Silence après lequel on vérifie que VTE a tout lu (pour pouvoir suivre un réagencement).
const REPOS: Duration = Duration::from_millis(80);
/// Une demande de position sans réponse est oubliée après ce délai.
const OUBLI_DEMANDE: Duration = Duration::from_secs(5);
/// Octets en attente au-delà desquels les marques sont abandonnées pour ne pas retenir la
/// sortie (mémoire bornée).
const FILE_MAX_OCTETS: usize = 8 * 1024 * 1024;
/// Longueur (octets) au-delà de laquelle une séquence CSI ou une chaîne OSC/DCS/APC… jamais
/// terminée est tenue pour finie par le suivi des frontières.
const CHAINE_MAX: usize = 1024 * 1024;
/// Une navigation demandée hors frontière propre attend celle-ci au plus ce délai, puis
/// échoue (annoncée).
const DELAI_FRONTIERE: Duration = Duration::from_millis(1000);
/// Navigations en attente d'une frontière au plus (les suivantes échouent, annoncées).
const NAVIGATIONS_EN_ATTENTE_MAX: usize = 8;
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
    static INACTIVES_FORCEES: Cell<bool> = const { Cell::new(false) };
}

/// Pour les tests : aucune fenêtre n'est tenue pour active (même si le compositeur en dit
/// autrement), pour essayer ce que Pupitre fait quand l'utilisateur est ailleurs. Prime sur
/// `forcer_fenetre_active`. Réservé aux tests.
#[doc(hidden)]
pub fn forcer_fenetres_inactives(oui: bool) {
    INACTIVES_FORCEES.with(|a| a.set(oui));
}

/// Pour les tests sous un compositeur sans clavier (Weston sans écran), où aucune fenêtre
/// ne devient jamais « active » : toute fenêtre est alors tenue pour active. Réservé aux tests.
#[doc(hidden)]
pub fn forcer_fenetre_active(oui: bool) {
    ACTIVE_FORCEE.with(|a| a.set(oui));
}

/// Vrai si la fenêtre `active` l'est vraiment (ou si les tests l'ont forcé).
pub fn fenetre_active(active: bool) -> bool {
    !INACTIVES_FORCEES.with(Cell::get) && (active || ACTIVE_FORCEE.with(Cell::get))
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
                    // Fin sans exécution : Entrée vide ou Ctrl+C à l'invite (bash et zsh
                    // envoient D sans C), ou marque C perdue. Aucune entrée n'est créée ;
                    // annoncée seulement si le fond a mesuré une durée.
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
    /// Un programme plein écran est affiché : les lignes lues seraient les siennes.
    Indisponible,
    /// La position n'a pas pu être relevée (flux arrêté au milieu d'une séquence, VTE muet,
    /// file débordée…) : annoncé, jamais passé sous silence.
    Impossible,
}

impl Navigation {
    /// Phrase à annoncer.
    pub fn texte(&self) -> String {
        match self {
            Navigation::Bout(Sens::Precedente) => tr("commande-aucune-avant"),
            Navigation::Bout(Sens::Suivante) => tr("commande-aucune-apres"),
            Navigation::Indisponible => tr("commande-navigation-indisponible"),
            Navigation::Impossible => tr("commande-navigation-impossible"),
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

/// Rangée (`g`) de la ligne logique d'une position (0 : celle du curseur, 1, 2… : les
/// précédentes, -1, -2… : les suivantes) et décalage en cellules dans cette ligne logique :
/// ne dépend pas de la largeur.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PosLogique {
    g: i64,
    decalage: i64,
}

/// Positions notées avant un réagencement de VTE (changement de largeur), gardées sous
/// forme logique le temps d'un geste de redimensionnement.
struct Instantane {
    /// Pris pour l'allocation en cours.
    neuf: bool,
    /// Colonnes de la grille au début du geste.
    colonnes: i64,
    positions: Vec<Option<PosLogique>>,
    g_max: usize,
    k_max: usize,
    /// Début du geste.
    debut: Instant,
    /// Dernière allocation de largeur vue.
    derniere: Instant,
}

/// Première ligne de chaque ligne logique autour du curseur.
#[derive(Debug, PartialEq, Eq)]
struct Carte {
    /// `haut[g]` : première ligne de la ligne logique `g` (0 : celle du curseur).
    haut: Vec<i64>,
    /// `bas[k - 1]` : première ligne de la ligne logique `-k` (sous celle du curseur).
    bas: Vec<i64>,
}

/// Construit la carte autour de `curseur`. `dur(r)` dit si un vrai retour à la ligne sépare
/// les lignes `r - 1` et `r`. Vers le haut, on s'arrête à `borne_haut` ou quand la ligne
/// logique `g_max` est complète ; vers le bas, à `borne_bas` ou après `k_max` lignes.
fn construire_carte(
    curseur: i64,
    borne_haut: i64,
    g_max: usize,
    borne_bas: i64,
    k_max: usize,
    dur: &dyn Fn(i64) -> bool,
) -> Carte {
    let mut haut = vec![curseur];
    let mut r = curseur;
    while r > borne_haut.max(0) {
        let p = r - 1;
        if dur(r) {
            if haut.len() > g_max {
                break;
            }
            haut.push(p);
        } else if let Some(d) = haut.last_mut() {
            *d = p;
        }
        r = p;
    }
    let mut bas = Vec::new();
    let mut r = curseur;
    while r < borne_bas && bas.len() < k_max {
        r += 1;
        if dur(r) {
            bas.push(r);
        }
    }
    Carte { haut, bas }
}

impl Carte {
    fn debut(&self, g: i64) -> Option<i64> {
        if g >= 0 {
            self.haut.get(usize::try_from(g).ok()?).copied()
        } else {
            self.bas.get(usize::try_from(-g - 1).ok()?).copied()
        }
    }
}

/// Position logique de `pos`, ou `None` si elle est plus haut que ce qui est connu.
fn vers_logique(pos: Pos, curseur: i64, carte: &Carte, colonnes: i64) -> Option<PosLogique> {
    let g = if pos.ligne <= curseur {
        // La plus petite rangée dont la première ligne ne dépasse pas la position.
        i64::try_from(carte.haut.iter().position(|&d| d <= pos.ligne)?).ok()?
    } else {
        // Sous le curseur : autant de lignes logiques que de débuts jusqu'à la position.
        -i64::try_from(carte.bas.iter().take_while(|&&d| d <= pos.ligne).count()).ok()?
    };
    let debut = carte.debut(g)?;
    Some(PosLogique {
        g,
        decalage: (pos.ligne - debut) * colonnes + pos.col,
    })
}

fn depuis_logique(l: PosLogique, carte: &Carte, colonnes: i64) -> Option<Pos> {
    let debut = carte.debut(l.g)?;
    let colonnes = colonnes.max(1);
    Some(Pos {
        ligne: debut + l.decalage / colonnes,
        col: l.decalage % colonnes,
    })
}

/// État d'analyse de ce qui a été donné à VTE : sert à ne glisser un repère qu'à une
/// frontière propre (ni au milieu d'une séquence d'échappement, ni au milieu d'un caractère
/// UTF-8).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum EtatFlux {
    #[default]
    Texte,
    Echap,
    Csi,
    /// OSC, DCS, SOS, PM ou APC : jusqu'à BEL ou ST.
    Chaine,
    ChaineEchap,
}

#[derive(Debug, Default, Clone)]
struct Frontiere {
    etat: EtatFlux,
    /// Octets de continuation UTF-8 encore attendus.
    utf8: u8,
    /// L'octet précédent était 0xC2 : le suivant peut former un contrôle C1 (U+0080 à
    /// U+009F), que VTE interprète comme son équivalent ESC (CSI, OSC, DCS, ST…).
    c2: bool,
    /// Octets lus dans la séquence CSI ou la chaîne en cours (borné par `CHAINE_MAX`).
    longueur: usize,
}

impl Frontiere {
    fn suivre(&mut self, data: &[u8]) {
        for &c in data {
            self.octet(c);
        }
    }

    /// Nombre d'octets de `data` à donner pour arriver à la première frontière propre
    /// (0 si on y est déjà), `None` s'il n'y en a pas dans `data`. Ne change rien.
    fn prochaine_propre(&self, data: &[u8]) -> Option<usize> {
        if self.propre() {
            return Some(0);
        }
        let mut f = self.clone();
        for (i, &c) in data.iter().enumerate() {
            f.octet(c);
            if f.propre() {
                return Some(i + 1);
            }
        }
        None
    }

    fn octet(&mut self, c: u8) {
        use EtatFlux::{Chaine, ChaineEchap, Csi, Echap, Texte};
        // CAN et SUB annulent toute séquence.
        if matches!(c, 0x18 | 0x1a) {
            self.etat = Texte;
            self.utf8 = 0;
            self.c2 = false;
            self.longueur = 0;
            return;
        }
        let avant = self.etat;
        let apres_c2 = std::mem::replace(&mut self.c2, c == 0xc2);
        // Contrôle C1 codé en UTF-8 (C2 80 à C2 9F).
        let c1 = if apres_c2 && (0x80..=0x9f).contains(&c) {
            match (avant, c) {
                (Texte, 0x9b) => Some(Csi),
                (Texte, 0x90 | 0x98 | 0x9d | 0x9e | 0x9f) => Some(Chaine),
                (Texte, _) => Some(Texte),
                // ST (U+009C) termine la chaîne.
                (Chaine, 0x9c) => Some(Texte),
                _ => None,
            }
        } else {
            None
        };
        let mut apres = if let Some(e) = c1 {
            self.utf8 = 0;
            e
        } else {
            match (avant, c) {
                (Texte | Csi | Echap | Chaine | ChaineEchap, 0x1b) => {
                    self.utf8 = 0;
                    if avant == Chaine {
                        ChaineEchap
                    } else {
                        Echap
                    }
                }
                (Texte, _) => {
                    match c {
                        0xc0..=0xdf => self.utf8 = 1,
                        0xe0..=0xef => self.utf8 = 2,
                        0xf0..=0xf7 => self.utf8 = 3,
                        0x80..=0xbf => self.utf8 = self.utf8.saturating_sub(1),
                        _ => self.utf8 = 0,
                    }
                    Texte
                }
                (Echap, b'[') => Csi,
                (Echap, b']' | b'P' | b'X' | b'^' | b'_') => Chaine,
                (Echap, 0x20..=0x2f) => Echap,
                (Echap, _) => Texte,
                (Csi, 0x40..=0x7e) => Texte,
                (Csi, _) => Csi,
                (Chaine, 0x07) => Texte,
                (Chaine, _) => Chaine,
                (ChaineEchap, b'\\') => Texte,
                (ChaineEchap, b'[') => Csi,
                (ChaineEchap, b']' | b'P' | b'X' | b'^' | b'_') => Chaine,
                (ChaineEchap, _) => Texte,
            }
        };
        // Sortie bornée : une séquence ou une chaîne jamais terminée ne bloque pas
        // l'automate indéfiniment (au pire un repère arrive au milieu d'une chaîne trop
        // longue, que VTE abandonne alors).
        self.longueur = match (avant, apres) {
            (Csi, Csi) | (Chaine, Chaine | ChaineEchap) => self.longueur + 1,
            _ => 0,
        };
        if self.longueur >= CHAINE_MAX {
            apres = Texte;
            self.longueur = 0;
            self.utf8 = 0;
        }
        self.etat = apres;
    }

    /// Vrai si le prochain octet peut commencer une séquence ou un caractère.
    fn propre(&self) -> bool {
        self.etat == EtatFlux::Texte && self.utf8 == 0
    }
}

/// Suit les séquences `CSI ? 47/1047/1049 h/l` du flux : vrai quand l'écran alternatif (vim,
/// less, htop…) est affiché. Les séquences coupées entre deux morceaux sont recollées.
#[derive(Debug, Default)]
struct SuiviEcran {
    alterne: bool,
    reste: Vec<u8>,
}

impl SuiviEcran {
    fn suivre(&mut self, data: &[u8]) {
        let mut tout = std::mem::take(&mut self.reste);
        tout.extend_from_slice(data);
        let mut i = 0;
        while i < tout.len() {
            if tout[i] != 0x1b {
                i += 1;
                continue;
            }
            // ESC c (RIS, envoyé par `reset`) : retour à l'écran normal.
            if tout.get(i + 1) == Some(&b'c') {
                self.alterne = false;
                i += 2;
                continue;
            }
            // ESC [ ? params final
            let mut j = i + 1;
            if tout.get(j).is_some_and(|&c| c != b'[') {
                i += 1;
                continue;
            }
            j += 1;
            if tout.get(j).is_some_and(|&c| c != b'?') {
                i += 1;
                continue;
            }
            j += 1;
            let debut = j;
            while tout
                .get(j)
                .is_some_and(|&c| c.is_ascii_digit() || c == b';')
            {
                j += 1;
            }
            match tout.get(j) {
                // Séquence coupée : on la reprend avec le morceau suivant.
                None => {
                    if tout.len() - i <= 32 {
                        self.reste = tout[i..].to_vec();
                    }
                    return;
                }
                Some(&f @ (b'h' | b'l')) => {
                    let modes = &tout[debut..j];
                    let concerne = modes
                        .split(|&c| c == b';')
                        .any(|m| matches!(m, b"47" | b"1047" | b"1049"));
                    if concerne {
                        self.alterne = f == b'h';
                    }
                    i = j + 1;
                }
                Some(_) => i = j.max(i + 1),
            }
        }
    }
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
    /// Aucune marque : seulement confirmer que VTE a lu tout ce qu'on lui a donné.
    Repos,
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
    /// dans l'ordre) : la réponse est gardée pour soi même si l'attente a expiré. Une
    /// demande perdue est oubliée au bout de `OUBLI_DEMANDE`.
    reponses_attendues: VecDeque<(u32, Instant)>,
    reponses_gardees: u32,
    /// Une confirmation de repos est programmée.
    confirmation_programmee: bool,
    /// Instant des derniers octets donnés à VTE.
    dernier_octet: Option<Instant>,
    /// Un minuteur attend la fin du geste de redimensionnement.
    calme_programme: bool,
    /// Temps passé à convertir les positions (réagencements), et nombre de conversions.
    temps_conversions: Duration,
    conversions: u32,
    /// Réagencements pour lesquels les positions n'ont pas pu être suivies.
    reagencements_imprecis: u32,
    /// Navigations demandées alors que le flux donné à VTE s'arrêtait hors frontière
    /// propre : elles attendent la prochaine (au plus `DELAI_FRONTIERE`).
    navigations_en_attente: Vec<RappelCalibrage>,
    /// Numéro de la dernière mise en attente (le délai d'une attente finie est ignoré).
    generation_attente: u32,
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
    suivi_ecran: RefCell<SuiviEcran>,
    frontiere: RefCell<Frontiere>,
    file_max: Cell<usize>,
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
            suivi_ecran: RefCell::new(SuiviEcran::default()),
            frontiere: RefCell::new(Frontiere::default()),
            file_max: Cell::new(FILE_MAX_OCTETS),
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

    /// Nombre de retours à la ligne durs entre le début de la ligne `de` et celui de `a`.
    fn retours_durs(&self, de: i64, a: i64) -> i64 {
        let (t, _) = self
            .terminal
            .text_range_format(vte4::Format::Text, de, 0, a, 0);
        t.map_or(-1, |t| t.matches('\n').count() as i64)
    }

    /// Carte des lignes logiques autour du curseur. Quand aucune ligne n'est repliée (cas
    /// courant : une seule lecture du texte suffit à le savoir), chaque ligne est sa propre
    /// ligne logique et rien d'autre n'est lu ; sinon on lit ligne par ligne, au plus
    /// jusqu'à `borne_haut` et `borne_bas`.
    fn carte(
        &self,
        curseur: i64,
        borne_haut: i64,
        g_max: usize,
        borne_bas: i64,
        k_max: usize,
    ) -> Carte {
        let haut = borne_haut.clamp(0, curseur);
        let bas = borne_bas.max(curseur);
        if self.retours_durs(haut, curseur) == curseur - haut
            && self.retours_durs(curseur, bas) == bas - curseur
        {
            let g_utiles = usize::try_from(curseur - haut).unwrap_or(0).min(g_max);
            let k_utiles = usize::try_from(bas - curseur).unwrap_or(0).min(k_max);
            return Carte {
                haut: (0..=g_utiles).map(|g| curseur - g as i64).collect(),
                bas: (1..=k_utiles).map(|k| curseur + k as i64).collect(),
            };
        }
        construire_carte(curseur, borne_haut, g_max, borne_bas, k_max, &|r| {
            self.retour_dur(r)
        })
    }

    /// Première ligne absolue qui peut encore être dans l'historique de VTE : l'ajustement
    /// compte les lignes conservées, le curseur en fait partie.
    fn premiere_ligne(&self, curseur: i64) -> i64 {
        let conservees = self
            .terminal
            .vadjustment()
            .map_or(i64::MAX / 2, |a| a.upper() as i64);
        (curseur - (conservees - 1).max(0)).max(0)
    }

    /// Écran alternatif affiché (vim, less…) d'après le flux donné à VTE.
    pub fn ecran_alterne(&self) -> bool {
        self.suivi_ecran.borrow().alterne
    }

    /// À appeler juste avant que VTE réagence ses lignes (largeur de la grille changée).
    ///
    /// Au début d'un geste, les positions sont notées en lignes logiques (une fois) et la
    /// sortie des programmes est retenue ; les changements de largeur suivants ne coûtent
    /// rien. Le retour aux lignes absolues se fait au calme (`CALME` sans changement), à la
    /// prochaine navigation, ou au bout de `GESTE_MAX`, et reste exact : VTE ne reçoit rien
    /// entre-temps, donc le curseur est resté sur la même ligne logique.
    ///
    /// Si ce n'est pas possible (écran alternatif affiché, ou octets donnés à VTE et pas
    /// encore confirmés lus), les positions sont gardées telles quelles, jamais effacées ;
    /// elles ne sont alors imprécises que pour les lignes repliées.
    pub fn avant_reagencement(&self) {
        if let Some(i) = self.instantane.borrow_mut().as_mut() {
            i.derniere = Instant::now();
            return;
        }
        if self.marques.borrow().is_empty() {
            return;
        }
        let repos = {
            let e = self.etat.borrow();
            e.attente.is_none() && !e.octets_depuis_pos
        };
        if self.ecran_alterne() || !repos || !self.terminal.is_mapped() {
            self.etat.borrow_mut().reagencements_imprecis += 1;
            return;
        }
        let debut = Instant::now();
        let colonnes = self.terminal.column_count();
        let (_, curseur) = self.terminal.cursor_position();
        // D'abord oublier ce qui est sorti de l'historique : rien à convertir pour cela.
        let premiere = self.premiere_ligne(curseur);
        self.marques
            .borrow_mut()
            .remplacer_positions(|p| (p.ligne >= premiere).then_some(p));
        let positions = self.marques.borrow().positions();
        if positions.is_empty() || colonnes <= 0 {
            return;
        }
        let haut = positions.iter().map(|p| p.ligne).min().unwrap_or(curseur);
        let bas = positions.iter().map(|p| p.ligne).max().unwrap_or(curseur);
        let carte = self.carte(curseur, haut, usize::MAX, bas, usize::MAX);
        let logiques: Vec<Option<PosLogique>> = positions
            .iter()
            .map(|&p| vers_logique(p, curseur, &carte, colonnes))
            .collect();
        let g_max = logiques
            .iter()
            .flatten()
            .filter_map(|l| usize::try_from(l.g).ok())
            .max()
            .unwrap_or(0);
        let k_max = logiques
            .iter()
            .flatten()
            .filter_map(|l| usize::try_from(-l.g).ok())
            .max()
            .unwrap_or(0);
        *self.instantane.borrow_mut() = Some(Instantane {
            neuf: true,
            colonnes,
            positions: logiques,
            g_max,
            k_max,
            debut: Instant::now(),
            derniere: Instant::now(),
        });
        let mut e = self.etat.borrow_mut();
        e.derniere_pos = None;
        e.temps_conversions += debut.elapsed();
        e.conversions += 1;
    }

    /// À appeler juste après l'allocation : attend le calme pour reconvertir les positions.
    pub fn apres_reagencement(&self) {
        // Instantané pris pour cette allocation, mais même nombre de colonnes qu'avant :
        // les lignes sont celles d'origine, rien à reconvertir.
        let inchange = self.instantane.borrow_mut().as_mut().is_some_and(|i| {
            std::mem::take(&mut i.neuf) && i.colonnes == self.terminal.column_count()
        });
        if inchange {
            *self.instantane.borrow_mut() = None;
            self.avancer();
            return;
        }
        if self.instantane.borrow().is_none() || self.etat.borrow().calme_programme {
            return;
        }
        self.etat.borrow_mut().calme_programme = true;
        self.armer_calme(CALME);
    }

    fn armer_calme(&self, delai: Duration) {
        let faible = self.soi.borrow().clone();
        gtk::glib::timeout_add_local_once(delai, move || {
            if let Some(f) = faible.upgrade() {
                f.calme_ecoule();
            }
        });
    }

    fn calme_ecoule(&self) {
        let reste = {
            let i = self.instantane.borrow();
            match i.as_ref() {
                None => {
                    self.etat.borrow_mut().calme_programme = false;
                    return;
                }
                Some(i) => {
                    let calme = i.derniere.elapsed();
                    let geste = i.debut.elapsed();
                    if calme >= CALME || geste >= GESTE_MAX {
                        None
                    } else {
                        Some(CALME - calme)
                    }
                }
            }
        };
        match reste {
            Some(d) => self.armer_calme(d),
            None => {
                self.etat.borrow_mut().calme_programme = false;
                self.restaurer();
            }
        }
    }

    /// Reconvertit les positions en lignes absolues (fin du geste) puis reprend la file.
    fn restaurer(&self) {
        let Some(inst) = self.instantane.borrow_mut().take() else {
            return;
        };
        let debut = Instant::now();
        let colonnes = self.terminal.column_count();
        let (_, curseur) = self.terminal.cursor_position();
        let carte = self.carte(
            curseur,
            self.premiere_ligne(curseur),
            inst.g_max,
            curseur + self.terminal.row_count(),
            inst.k_max,
        );
        let mut suivantes = inst.positions.iter();
        self.marques.borrow_mut().remplacer_positions(|_| {
            suivantes
                .next()
                .copied()
                .flatten()
                .and_then(|l| depuis_logique(l, &carte, colonnes))
        });
        {
            let mut e = self.etat.borrow_mut();
            e.temps_conversions += debut.elapsed();
            e.derniere_pos = None;
        }
        self.avancer();
    }

    /// Change la borne de la file (tests).
    #[doc(hidden)]
    pub fn definir_file_max(&self, octets: usize) {
        self.file_max.set(octets);
    }

    /// Un réagencement a eu lieu sans avoir été annoncé : positions gardées telles quelles.
    pub fn reagencement_rate(&self) {
        self.etat.borrow_mut().reagencements_imprecis += 1;
    }

    /// Temps passé à convertir les positions et nombre de conversions (tests).
    #[doc(hidden)]
    pub fn conversions(&self) -> (Duration, u32, u32) {
        let e = self.etat.borrow();
        (e.temps_conversions, e.conversions, e.reagencements_imprecis)
    }

    /// Retient la sortie tant qu'un geste de redimensionnement n'est pas fini.
    fn en_geste(&self) -> bool {
        self.instantane.borrow().is_some()
    }

    /// Octets reçus du shell, dans l'ordre.
    pub fn octets(&self, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        let direct = {
            let e = self.etat.borrow();
            e.attente.is_none() && e.file.is_empty()
        } && !self.en_geste();
        if direct {
            self.donner_octets(data);
            // Une navigation qui attendait une frontière propre a pu être remise en file.
            self.avancer();
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

    /// Donne des octets à VTE. Si des navigations attendent une frontière propre, seuls les
    /// octets jusqu'à la première frontière sont donnés ; les navigations, puis le reste,
    /// sont remis en tête de la file (l'appelant doit ensuite faire `avancer`).
    fn donner_octets(&self, data: &[u8]) {
        let coupure = if self.etat.borrow().navigations_en_attente.is_empty() {
            None
        } else {
            self.frontiere.borrow().prochaine_propre(data)
        };
        let Some(k) = coupure else {
            self.donner_brut(data);
            return;
        };
        let (avant, reste) = data.split_at(k);
        if !avant.is_empty() {
            self.donner_brut(avant);
        }
        let mut e = self.etat.borrow_mut();
        if !reste.is_empty() {
            e.octets_en_file += reste.len();
            e.file.push_front(Travail::Octets(reste.to_vec()));
        }
        let navigations = std::mem::take(&mut e.navigations_en_attente);
        for f in navigations.into_iter().rev() {
            e.file.push_front(Travail::Calibrage(f));
        }
    }

    /// Met une navigation en attente de la prochaine frontière propre du flux ; au-delà de
    /// `DELAI_FRONTIERE`, elle échoue (annoncée).
    fn attendre_frontiere(&self, f: RappelCalibrage) {
        let (refusee, generation) = {
            let mut e = self.etat.borrow_mut();
            if e.navigations_en_attente.len() >= NAVIGATIONS_EN_ATTENTE_MAX {
                (Some(f), None)
            } else {
                let premiere = e.navigations_en_attente.is_empty();
                e.navigations_en_attente.push(f);
                if premiere {
                    e.generation_attente = e.generation_attente.wrapping_add(1);
                    (None, Some(e.generation_attente))
                } else {
                    (None, None)
                }
            }
        };
        if let Some(f) = refusee {
            f(self, None);
        }
        if let Some(g) = generation {
            let faible = self.soi.borrow().clone();
            gtk::glib::timeout_add_local_once(DELAI_FRONTIERE, move || {
                if let Some(flux) = faible.upgrade() {
                    flux.frontiere_expiree(g);
                }
            });
        }
    }

    /// Aucune frontière propre n'est arrivée à temps : les navigations en attente échouent.
    fn frontiere_expiree(&self, generation: u32) {
        let navigations = {
            let mut e = self.etat.borrow_mut();
            if e.generation_attente != generation {
                return;
            }
            std::mem::take(&mut e.navigations_en_attente)
        };
        for f in navigations {
            f(self, None);
        }
    }

    fn donner_brut(&self, data: &[u8]) {
        self.suivi_ecran.borrow_mut().suivre(data);
        self.frontiere.borrow_mut().suivre(data);
        let programmer = {
            let mut e = self.etat.borrow_mut();
            e.octets_depuis_pos = true;
            e.dernier_octet = Some(Instant::now());
            !std::mem::replace(&mut e.confirmation_programmee, true)
        };
        (self.donner)(data);
        if programmer {
            self.armer_repos(REPOS);
        }
    }

    fn armer_repos(&self, delai: Duration) {
        let faible = self.soi.borrow().clone();
        gtk::glib::timeout_add_local_once(delai, move || {
            if let Some(f) = faible.upgrade() {
                f.confirmer_repos();
            }
        });
    }

    /// Après `REPOS` de vrai silence (le minuteur est repoussé à chaque arrivée d'octets) :
    /// un repère pour savoir que VTE a tout lu. Jamais sur l'écran alternatif ni ailleurs
    /// qu'à une frontière propre du flux (pas au milieu d'une séquence ou d'un caractère).
    fn confirmer_repos(&self) {
        let reste = self
            .etat
            .borrow()
            .dernier_octet
            .map(|t| REPOS.saturating_sub(t.elapsed()))
            .filter(|d| !d.is_zero());
        if let Some(d) = reste {
            self.armer_repos(d);
            return;
        }
        let utile = {
            let mut e = self.etat.borrow_mut();
            e.confirmation_programmee = false;
            e.octets_depuis_pos && e.attente.is_none() && e.file.is_empty()
        };
        if utile
            && !self.en_geste()
            && self.terminal.is_mapped()
            && !self.ecran_alterne()
            && self.frontiere.borrow().propre()
        {
            self.attendre(Genre::Repos, b"");
        }
    }

    /// Trop d'octets attendent : on cesse d'attendre VTE, les octets passent et les marques
    /// restantes sont perdues.
    fn borner_file(&self) {
        if self.etat.borrow().octets_en_file <= self.file_max.get() {
            return;
        }
        // Un geste de redimensionnement en cours est terminé d'abord : donner les octets à
        // VTE déplacerait le curseur, et les positions seraient reconverties de travers.
        self.restaurer();
        if self.etat.borrow().octets_en_file <= self.file_max.get() {
            return;
        }
        // Les navigations demandées ne sont pas perdues : elles sont refaites une fois tous
        // les octets donnés (leur position est relevée à ce moment-là). Y compris celle dont
        // la demande de position est déjà partie : sa réponse tardive reste gardée pour soi
        // (`reponses_attendues`) et son repère, devenu ancien, est ignoré.
        let (travaux, mut navigations): (Vec<Travail>, Vec<RappelCalibrage>) = {
            let mut e = self.etat.borrow_mut();
            let mut navigations = Vec::new();
            if let Some(Attente {
                genre: Genre::Calibrage(f),
                ..
            }) = e.attente.take()
            {
                navigations.push(f);
            }
            navigations.append(&mut e.navigations_en_attente);
            e.octets_en_file = 0;
            (e.file.drain(..).collect(), navigations)
        };
        for t in travaux {
            match t {
                Travail::Octets(d) => self.donner_brut(&d),
                Travail::Calibrage(f) => navigations.push(f),
                Travail::Marque(..) => {}
            }
        }
        {
            let mut e = self.etat.borrow_mut();
            for f in navigations {
                e.file.push_back(Travail::Calibrage(f));
            }
        }
        self.avancer();
    }

    /// Traite la file jusqu'à la prochaine attente.
    fn avancer(&self) {
        loop {
            let travail = {
                let mut e = self.etat.borrow_mut();
                if e.attente.is_some() || self.instantane.borrow().is_some() {
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
                    // Flux arrêté au milieu d'une séquence ou d'un caractère : la demande de
                    // position attend la prochaine frontière propre ; la suite de la file
                    // continue (c'est elle qui l'amènera).
                    if !self.frontiere.borrow().propre() {
                        self.attendre_frontiere(f);
                    } else if self.attendre(Genre::Calibrage(f), b"\x1b[6n") {
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
        // Pas de repère au milieu d'une séquence ou d'un caractère : rien n'est donné, et
        // l'attente est levée au délai court (position lue sur le curseur). Les demandes de
        // position (navigation) n'arrivent jamais ici hors frontière : voir `avancer`.
        if !self.frontiere.borrow().propre() {
            self.armer_delai_court(repere);
            return true;
        }
        let mut data = avant.to_vec();
        data.extend_from_slice(format!("\x1b]666;{REPERE}={repere}\x1b\\").as_bytes());
        if avant.starts_with(b"\x1b[6n") {
            self.etat
                .borrow_mut()
                .reponses_attendues
                .push_back((repere, Instant::now()));
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

    /// Délai court quand aucun repère n'a pu être donné.
    fn armer_delai_court(&self, repere: u32) {
        let faible = self.soi.borrow().clone();
        gtk::glib::timeout_add_local_once(Duration::from_millis(100), move || {
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
            Genre::Repos => {
                let (col, ligne) = self.terminal.cursor_position();
                let mut e = self.etat.borrow_mut();
                e.derniere_pos = Some(Pos { ligne, col });
                e.octets_depuis_pos = false;
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
        // Une demande restée sans réponse ne doit pas avaler une réponse du programme.
        while e
            .reponses_attendues
            .front()
            .is_some_and(|(_, t)| t.elapsed() > OUBLI_DEMANDE)
        {
            e.reponses_attendues.pop_front();
        }
        let Some((repere, _)) = e.reponses_attendues.pop_front() else {
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
    /// appelle `fini` avec ce qu'il faut annoncer (toujours quelque chose, jamais de
    /// silence : `Navigation::Impossible` si la position n'a pas pu être relevée).
    pub fn naviguer(&self, sens: Sens, fini: impl FnOnce(Navigation) + 'static) {
        // Écran alternatif : les lignes lues seraient celles du programme, pas les nôtres.
        if self.ecran_alterne() {
            fini(Navigation::Indisponible);
            return;
        }
        // Un geste de redimensionnement en cours est terminé d'abord.
        self.restaurer();
        let rappel: RappelCalibrage = Box::new(move |flux, calibrage| {
            // L'écran alternatif a pu s'afficher pendant l'attente.
            let n = if flux.ecran_alterne() {
                Navigation::Indisponible
            } else {
                match calibrage {
                    Some(c) => flux.aller(sens, c),
                    None => Navigation::Impossible,
                }
            };
            fini(n);
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
    fn entree_vide_et_ctrl_c_ne_creent_rien() {
        // bash et zsh : D sans C à chaque Entrée vide ou Ctrl+C, puis une nouvelle invite.
        let mut m = MarquesOnglet::new(100);
        let d = |code| Fin {
            code: Some(code),
            duree_ms: None,
        };
        for (l, code) in [(0, 0), (2, 130)] {
            m.appliquer(Invite, p(l, 0));
            m.appliquer(Saisie, p(l, 2));
            assert_eq!(m.appliquer(d(code), p(l + 1, 0)), None);
        }
        m.appliquer(Invite, p(4, 0));
        m.appliquer(Saisie, p(4, 2));
        assert_eq!(m.len(), 1, "une seule invite en saisie");
        assert!(m.deplacer(Sens::Precedente).is_none());
    }

    #[test]
    fn lignes_logiques_traversent_un_reagencement() {
        // Avant : 80 colonnes. Lignes 0-1 : une ligne logique repliée (100 caractères) ;
        // 2 : une ligne ; 3-4 : une ligne repliée ; curseur ligne 5.
        let dur_avant = |r: i64| matches!(r, 2 | 3 | 5);
        let carte = construire_carte(5, 0, usize::MAX, 5, usize::MAX, &dur_avant);
        assert_eq!(carte.haut, [5, 3, 2, 0]);
        let l = vers_logique(p(1, 20), 5, &carte, 80).unwrap();
        assert_eq!(
            l,
            PosLogique {
                g: 3,
                decalage: 100
            }
        );
        let l2 = vers_logique(p(4, 10), 5, &carte, 80).unwrap();
        assert_eq!(l2, PosLogique { g: 1, decalage: 90 });
        // Après : 120 colonnes, plus de repli. Curseur ligne 3.
        let dur_apres = |r: i64| (1..=3).contains(&r);
        let carte2 = construire_carte(3, 0, 3, 3, usize::MAX, &dur_apres);
        assert_eq!(carte2.haut, [3, 2, 1, 0]);
        assert_eq!(
            depuis_logique(l, &carte2, 120),
            Some(Pos { ligne: 0, col: 100 })
        );
        assert_eq!(
            depuis_logique(l2, &carte2, 120),
            Some(Pos { ligne: 2, col: 90 })
        );
        // Ligne logique plus ancienne que ce qui est connu : perdue.
        assert_eq!(
            depuis_logique(PosLogique { g: 9, decalage: 0 }, &carte2, 120),
            None
        );
    }

    #[test]
    fn positions_sous_le_curseur_gardees() {
        // Curseur ligne 2 ; dessous : lignes 3 (début d'une ligne logique), 4-5 repliées.
        let dur = |r: i64| matches!(r, 3 | 4);
        let carte = construire_carte(2, 0, usize::MAX, 6, usize::MAX, &dur);
        assert_eq!(carte.bas, [3, 4]);
        let l = vers_logique(p(5, 7), 2, &carte, 80).unwrap();
        assert_eq!(
            l,
            PosLogique {
                g: -2,
                decalage: 87
            }
        );
        // Même position après un réagencement : les deux lignes ne se replient plus.
        let dur2 = |r: i64| matches!(r, 3 | 4);
        let carte2 = construire_carte(2, 0, 0, 6, 2, &dur2);
        assert_eq!(
            depuis_logique(l, &carte2, 200),
            Some(Pos { ligne: 4, col: 87 })
        );
        // Sur la ligne logique du curseur mais sous lui (curseur au milieu d'un repli).
        let l0 = vers_logique(p(2, 5), 2, &carte, 80).unwrap();
        assert_eq!(l0.g, 0);
    }

    #[test]
    fn frontiere_propre() {
        let propre = |t: &[u8]| {
            let mut f = Frontiere::default();
            f.suivre(t);
            f.propre()
        };
        assert!(propre(b""));
        assert!(propre(b"abc\r\n"));
        assert!(propre("é€😀".as_bytes()));
        assert!(propre(b"\x1b[1;31mrouge\x1b[0m"));
        assert!(propre(b"\x1b]133;A\x07"));
        assert!(propre(b"\x1b]0;titre\x1b\\"));
        assert!(!propre(b"abc\x1b"));
        assert!(!propre(b"abc\x1b["));
        assert!(!propre(b"\x1b[1;3"));
        assert!(!propre(b"\x1b]0;titre"));
        assert!(!propre(b"\x1b]0;titre\x1b"));
        assert!(!propre(&"é".as_bytes()[..1]));
        assert!(!propre(&"😀".as_bytes()[..3]));
        // Coupures en deux morceaux n'importe où : propre seulement aux frontières.
        let flux = "a\x1b[1;31mé€😀\x1b[0m\x1b]133;B\x07b".as_bytes();
        let mut f = Frontiere::default();
        let mut propres = Vec::new();
        for (i, &c) in flux.iter().enumerate() {
            f.octet(c);
            if f.propre() {
                propres.push(i + 1);
            }
        }
        for &i in &propres {
            let (a, b) = flux.split_at(i);
            // Chaque frontière annoncée propre sépare bien des éléments entiers.
            assert!(std::str::from_utf8(a).is_ok(), "coupure en {i}");
            assert!(!a.ends_with(b"\x1b") && !b.starts_with(b"[") || i == 0);
        }
        assert!(propres.contains(&flux.len()));
    }

    #[test]
    fn frontiere_c1_et_chaine_bornee() {
        let propre = |t: &[u8]| {
            let mut f = Frontiere::default();
            f.suivre(t);
            f.propre()
        };
        // ST en C1 codé en UTF-8 (U+009C) : fin de chaîne, comme VTE.
        assert!(propre(b"\x1b]0;titre\xc2\x9c"));
        assert!(propre(b"\x1b]0;titre\xc2\x9cabc"));
        assert!(!propre(b"\x1b]0;titre\xc2"));
        // Un autre caractère en C2 ne termine pas la chaîne.
        assert!(!propre(b"\x1b]0;titre\xc2\xa9"));
        // Introducteurs C1 en UTF-8 (CSI U+009B, OSC U+009D, DCS U+0090).
        assert!(!propre(b"\xc2\x9b1;3"));
        assert!(propre(b"\xc2\x9b1;31m"));
        assert!(!propre(b"\xc2\x9d0;titre"));
        assert!(propre(b"\xc2\x9d0;titre\x07"));
        assert!(!propre(b"\xc2\x90q#0"));
        assert!(propre("é©".as_bytes()));
        // Chaîne jamais terminée : l'automate en sort au-delà de la longueur maximale.
        let mut f = Frontiere::default();
        f.suivre(b"\x1b]");
        f.suivre(&vec![b'x'; CHAINE_MAX - 1]);
        assert!(!f.propre());
        f.suivre(b"x");
        assert!(f.propre());
        // Première frontière propre dans un morceau à venir.
        let mut f = Frontiere::default();
        assert_eq!(f.prochaine_propre(b"x"), Some(0));
        f.suivre(b"\x1b[1;3");
        assert_eq!(f.prochaine_propre(b"1mvert \xc3"), Some(2));
        f.suivre(b"1mvert \xc3");
        assert_eq!(f.prochaine_propre(b"\xa9t"), Some(1));
        let mut f = Frontiere::default();
        f.suivre(b"\x1b]0;t");
        assert_eq!(f.prochaine_propre(b"abc"), None);
        assert!(!f.propre());
        // De même pour une séquence CSI interminable.
        let mut f = Frontiere::default();
        f.suivre(b"\x1b[");
        f.suivre(&vec![b'1'; CHAINE_MAX]);
        assert!(f.propre());
    }

    #[test]
    fn ris_revient_a_l_ecran_normal() {
        let mut s = SuiviEcran::default();
        s.suivre(b"\x1b[?1049h");
        assert!(s.alterne);
        s.suivre(b"\x1bc");
        assert!(!s.alterne);
        s.suivre(b"\x1b[?47h\x1b");
        assert!(s.alterne);
        s.suivre(b"c");
        assert!(!s.alterne);
    }

    #[test]
    fn suivi_de_l_ecran_alternatif() {
        let mut s = SuiviEcran::default();
        s.suivre(b"abc\x1b[?1049hvim");
        assert!(s.alterne);
        s.suivre(b"\x1b[?25h"); // curseur visible : sans rapport
        assert!(s.alterne);
        // Séquence coupée entre deux morceaux.
        s.suivre(b"fin\x1b[?10");
        assert!(s.alterne);
        s.suivre(b"49l\r\n");
        assert!(!s.alterne);
        s.suivre(b"\x1b[?1;47h");
        assert!(s.alterne);
        s.suivre(b"\x1b[?1047l");
        assert!(!s.alterne);
        s.suivre(b"\x1b[?2004h\x1b[?47");
        assert!(!s.alterne);
        s.suivre(b"h");
        assert!(s.alterne);
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
        assert_eq!(
            Navigation::Impossible.texte(),
            "Navigation impossible pour l'instant."
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
