// SPDX-License-Identifier: GPL-3.0-or-later
//! Lecture, dans le flux d'une session, des séquences OSC qui intéressent Pupitre : marques
//! de commandes (133), copie dans le presse-papiers (52, écriture seulement) et
//! notifications d'un programme (9 et 777).

use pupitre_proto::MarqueCommande;
use std::time::{Duration, Instant};

/// Taille maximale d'une séquence OSC suivie ; au-delà, elle est abandonnée.
pub const OSC_MAX: usize = 4096;

/// Marques transmises au plus par morceau lu du PTY. Chaque marque coupe le flux en deux
/// messages : sans limite, un seul gros morceau plein de fausses marques remplirait la file
/// (bornée) d'un client et le ferait déconnecter.
pub const MARQUES_PAR_LECTURE: u32 = 16;
/// Marques transmises au plus par seconde et par session (fenêtre d'une seconde). Avec la
/// réserve des `D` (voir [`LimiteMarques`]), cela reste très loin de la file d'un client.
pub const MARQUES_PAR_SECONDE: u32 = 100;
const FENETRE: Duration = Duration::from_secs(1);

/// Taille maximale, en octets décodés, d'un texte copié par OSC 52 ; au-delà, la séquence est
/// suivie sans être copiée, puis ignorée.
pub const PRESSE_PAPIERS_MAX: usize = 1 << 20;
/// Longueur maximale, en caractères, du titre et du corps d'une notification ; au-delà, le
/// texte est tronqué et terminé par « … ».
pub const NOTIFICATION_MAX: usize = 1024;
/// Longueur du base64 (avec bourrage) du texte le plus long accepté.
const BASE64_MAX: usize = PRESSE_PAPIERS_MAX.div_ceil(3) * 4;
/// Longueur maximale du contenu d'une OSC 52 : `52;`, les cibles (au plus 16), `;`, la charge.
const CORPS_52_MAX: usize = 3 + 16 + 1 + BASE64_MAX;

/// Un morceau du flux, dans l'ordre : des octets à relayer, ou un événement vu juste après.
/// La copie et la notification ne valent qu'en direct : à la reprise, on ne les rejoue pas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Morceau {
    Octets(Vec<u8>),
    Marque(MarqueCommande),
    /// Un programme demande de copier ce texte dans le presse-papiers (OSC 52).
    PressePapiers(String),
    /// Un programme demande une notification (OSC 9 ou OSC 777), texte sans contrôles.
    Notification {
        titre: Option<String>,
        corps: String,
    },
}

/// Limite du débit des marques transmises (les octets, eux, passent toujours). Une marque
/// refusée est simplement ignorée : seule la navigation entre commandes est dégradée.
/// Une marque `D` qui clôt une commande dont le `C` a été transmis passe toujours (elle porte
/// l'annonce de fin) ; comme les `C` sont limités, ces `D` le sont aussi.
#[derive(Debug, Default)]
pub struct LimiteMarques {
    dans_la_lecture: u32,
    debut_fenetre: Option<Instant>,
    dans_la_fenetre: u32,
    /// Un `C` a été transmis et son `D` n'est pas encore passé.
    execution_transmise: bool,
}

impl LimiteMarques {
    pub fn new() -> Self {
        Self::default()
    }

    /// À appeler au début de chaque morceau lu du PTY.
    pub fn nouvelle_lecture(&mut self) {
        self.dans_la_lecture = 0;
    }

    /// Vrai si `marque` peut être transmise (et comptée).
    pub fn admettre(&mut self, marque: &MarqueCommande, maintenant: Instant) -> bool {
        if matches!(marque, MarqueCommande::Fin { .. }) && self.execution_transmise {
            self.execution_transmise = false;
            return true;
        }
        if !self.compter(maintenant) {
            return false;
        }
        match marque {
            MarqueCommande::Execution => self.execution_transmise = true,
            MarqueCommande::Fin { .. } => self.execution_transmise = false,
            _ => {}
        }
        true
    }

    /// Vrai si une copie ou une notification peut être transmise : elle compte dans la même
    /// limite que les marques.
    pub fn admettre_evenement(&mut self, maintenant: Instant) -> bool {
        self.compter(maintenant)
    }

    /// Compte un événement dans la lecture et la fenêtre ; faux si l'une des limites est
    /// atteinte.
    fn compter(&mut self, maintenant: Instant) -> bool {
        match self.debut_fenetre {
            Some(d) if maintenant.saturating_duration_since(d) < FENETRE => {}
            _ => {
                self.debut_fenetre = Some(maintenant);
                self.dans_la_fenetre = 0;
            }
        }
        if self.dans_la_lecture >= MARQUES_PAR_LECTURE
            || self.dans_la_fenetre >= MARQUES_PAR_SECONDE
        {
            return false;
        }
        self.dans_la_lecture += 1;
        self.dans_la_fenetre += 1;
        true
    }
}

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;
/// Débuts des séquences qui nous intéressent, après `ESC ]`.
const PREFIXE_133: &[u8] = b"133;";
const PREFIXE_52: &[u8] = b"52;";
const PREFIXE_9: &[u8] = b"9;";
const PREFIXE_777: &[u8] = b"777;notify;";
const PREFIXES: [&[u8]; 4] = [PREFIXE_133, PREFIXE_52, PREFIXE_9, PREFIXE_777];
/// Longueur du plus long préfixe : au-delà, le genre de la séquence est décidé.
const PREFIXE_MAX: usize = PREFIXE_777.len();

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Etat {
    /// Hors de toute séquence.
    #[default]
    Normal,
    /// `ESC` vu.
    Esc,
    /// Dans `ESC ] …`, contenu accumulé dans `corps`.
    Osc,
    /// `ESC` vu dans une OSC : `\` la termine.
    OscEsc,
}

/// Lecteur des séquences OSC 133 (`ESC ] 133 ; A|B|C|D [; …]`), 52, 9 et 777, terminées par
/// `BEL` ou `ESC \`. Il ne modifie ni ne retarde le flux : les octets sont rendus tels
/// quels, l'événement est rendu juste après l'octet qui termine la séquence. Seul le début de
/// la séquence en cours est copié, et jamais plus de [`OSC_MAX`] octets, sauf la charge d'une
/// OSC 52, gardée jusqu'à sa propre borne.
#[derive(Debug, Default)]
pub struct LecteurMarques {
    etat: Etat,
    /// Contenu de la séquence en cours, après `ESC ]`.
    corps: Vec<u8>,
    /// Instant de la dernière marque `C`, tant que la commande n'est pas finie.
    execution: Option<Instant>,
}

impl LecteurMarques {
    pub fn new() -> Self {
        Self::default()
    }

    /// Octets réservés par le lecteur pour la séquence en cours (pour borner la mémoire).
    pub fn memoire(&self) -> usize {
        self.corps.capacity()
    }

    /// Lit un morceau du flux. La concaténation des [`Morceau::Octets`] rendus est
    /// exactement `octets`.
    pub fn lire(&mut self, octets: &[u8], maintenant: Instant) -> Vec<Morceau> {
        let mut rendu = Vec::new();
        let mut debut = 0usize;
        let mut i = 0usize;
        while i < octets.len() {
            if self.etat == Etat::Normal {
                // Voie rapide : on saute jusqu'au prochain ESC.
                match octets
                    .get(i..)
                    .and_then(|reste| reste.iter().position(|&o| o == ESC))
                {
                    Some(p) => i += p,
                    None => break,
                }
            }
            let Some(&o) = octets.get(i) else { break };
            i += 1;
            if let Some(evenement) = self.avancer(o, maintenant) {
                if let Some(avant) = octets.get(debut..i) {
                    rendu.push(Morceau::Octets(avant.to_vec()));
                }
                rendu.push(evenement);
                debut = i;
            }
        }
        if let Some(reste) = octets.get(debut..) {
            if !reste.is_empty() {
                rendu.push(Morceau::Octets(reste.to_vec()));
            }
        }
        rendu
    }

    /// Fait avancer l'automate d'un octet ; rend l'événement qu'il termine, s'il y en a un.
    fn avancer(&mut self, o: u8, maintenant: Instant) -> Option<Morceau> {
        match self.etat {
            Etat::Normal => {
                if o == ESC {
                    self.etat = Etat::Esc;
                }
                None
            }
            Etat::Esc => {
                match o {
                    b']' => {
                        self.etat = Etat::Osc;
                        self.corps.clear();
                    }
                    // `ESC ESC` : le second recommence une séquence.
                    ESC => {}
                    _ => self.etat = Etat::Normal,
                }
                None
            }
            Etat::Osc => match o {
                BEL => self.terminer(maintenant),
                ESC => {
                    self.etat = Etat::OscEsc;
                    None
                }
                // CAN et SUB annulent une séquence.
                0x18 | 0x1a => self.abandonner(),
                _ => {
                    self.corps.push(o);
                    let n = self.corps.len();
                    // Autre OSC que celles qui nous intéressent : on ne la suit plus.
                    if n <= PREFIXE_MAX && !self.prefixe_possible() {
                        return self.abandonner();
                    }
                    if n >= OSC_MAX {
                        // Trop longue, sauf la charge d'une OSC 52, gardée jusqu'à sa borne.
                        let copie = self.corps.starts_with(PREFIXE_52);
                        if !copie || n > CORPS_52_MAX {
                            return self.abandonner();
                        }
                        if n == OSC_MAX {
                            self.corps.reserve_exact(CORPS_52_MAX - n);
                        }
                    }
                    None
                }
            },
            Etat::OscEsc => {
                if o == b'\\' {
                    return self.terminer(maintenant);
                }
                // Séquence interrompue : l'octet est relu comme suite d'un `ESC`.
                self.abandonner();
                self.etat = Etat::Esc;
                self.avancer(o, maintenant)
            }
        }
    }

    /// Le contenu déjà lu est-il le début d'un des préfixes connus ?
    fn prefixe_possible(&self) -> bool {
        PREFIXES.iter().any(|p| {
            let n = self.corps.len().min(p.len());
            self.corps.get(..n) == p.get(..n)
        })
    }

    fn abandonner(&mut self) -> Option<Morceau> {
        self.etat = Etat::Normal;
        self.corps.clear();
        // Une charge d'OSC 52 a pu agrandir le tampon : on ne le garde pas.
        self.corps.shrink_to(OSC_MAX);
        None
    }

    /// La séquence est complète : interprète son contenu.
    fn terminer(&mut self, maintenant: Instant) -> Option<Morceau> {
        self.etat = Etat::Normal;
        // Le tampon est rendu à chaque séquence : une grande charge ne reste pas allouée.
        let corps = std::mem::take(&mut self.corps);
        if let Some(reste) = corps.strip_prefix(PREFIXE_133) {
            return self.marque(reste, maintenant).map(Morceau::Marque);
        }
        if let Some(reste) = corps.strip_prefix(PREFIXE_52) {
            return presse_papiers(reste).map(Morceau::PressePapiers);
        }
        if let Some(reste) = corps.strip_prefix(PREFIXE_777) {
            return notification_777(reste);
        }
        if let Some(reste) = corps.strip_prefix(PREFIXE_9) {
            return notification_9(reste);
        }
        None
    }

    /// Interprète une marque OSC 133 (`reste` suit `133;`).
    fn marque(&mut self, reste: &[u8], maintenant: Instant) -> Option<MarqueCommande> {
        let (&lettre, params) = reste.split_first()?;
        // La lettre est suivie de rien ou de `;` : `133;AB` n'est pas une marque.
        let params = match params.split_first() {
            None => &[][..],
            Some((&b';', p)) => p,
            Some(_) => return None,
        };
        // Entre `C` et `D`, seules les marques `D` comptent.
        if self.execution.is_some() && lettre != b'D' {
            return None;
        }
        match lettre {
            b'A' => Some(MarqueCommande::Invite),
            b'B' => Some(MarqueCommande::Saisie),
            b'C' => {
                self.execution = Some(maintenant);
                Some(MarqueCommande::Execution)
            }
            b'D' => {
                let code = params
                    .split(|&c| c == b';')
                    .next()
                    .and_then(|c| std::str::from_utf8(c).ok())
                    .and_then(|c| c.parse::<i32>().ok());
                let duree_ms = self.execution.take().map(|debut| {
                    let ms = maintenant.saturating_duration_since(debut).as_millis();
                    u64::try_from(ms).unwrap_or(u64::MAX)
                });
                Some(MarqueCommande::Fin { code, duree_ms })
            }
            _ => None,
        }
    }
}

/// Retire les caractères de contrôle, borne la longueur et enlève les blancs de rive.
fn texte_notification(octets: &[u8]) -> String {
    let texte: String = String::from_utf8_lossy(octets)
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    let texte = texte.trim();
    if texte.chars().count() > NOTIFICATION_MAX {
        let mut coupe: String = texte.chars().take(NOTIFICATION_MAX).collect();
        coupe.push('…');
        coupe
    } else {
        texte.to_owned()
    }
}

/// `9;<texte>` (iTerm2) ; `9;4;…` est une barre de progression (ConEmu), ignorée.
fn notification_9(reste: &[u8]) -> Option<Morceau> {
    if reste == b"4" || reste.starts_with(b"4;") {
        return None;
    }
    let corps = texte_notification(reste);
    (!corps.is_empty()).then_some(Morceau::Notification { titre: None, corps })
}

/// `777;notify;<titre>;<corps>` (le corps peut contenir des `;`) ; sans corps, le seul
/// champ présent sert de corps.
fn notification_777(reste: &[u8]) -> Option<Morceau> {
    let (titre, corps) = match reste.iter().position(|&o| o == b';') {
        Some(p) => (reste.get(..p)?, reste.get(p + 1..)?),
        None => (&[][..], reste),
    };
    let titre = texte_notification(titre);
    let corps = texte_notification(corps);
    match (titre.is_empty(), corps.is_empty()) {
        (true, true) => None,
        (false, true) => Some(Morceau::Notification {
            titre: None,
            corps: titre,
        }),
        (true, false) => Some(Morceau::Notification { titre: None, corps }),
        (false, false) => Some(Morceau::Notification {
            titre: Some(titre),
            corps,
        }),
    }
}

/// `52;<cibles>;<base64>` (`reste` suit `52;`) : écriture seulement. La lecture (`?`) n'est
/// jamais servie ; une cible autre que le presse-papiers (`c`, ou vide) est ignorée.
fn presse_papiers(reste: &[u8]) -> Option<String> {
    let p = reste.iter().position(|&o| o == b';')?;
    let (cibles, charge) = (reste.get(..p)?, reste.get(p + 1..)?);
    if !cibles.is_empty() && !cibles.contains(&b'c') {
        return None;
    }
    if charge.is_empty() || charge == b"?" {
        return None;
    }
    let octets = decoder_base64(charge)?;
    if octets.len() > PRESSE_PAPIERS_MAX {
        return None;
    }
    String::from_utf8(octets).ok()
}

/// Valeur d'un caractère de l'alphabet base64 standard.
fn valeur_base64(o: u8) -> Option<u32> {
    match o {
        b'A'..=b'Z' => Some(u32::from(o - b'A')),
        b'a'..=b'z' => Some(u32::from(o - b'a') + 26),
        b'0'..=b'9' => Some(u32::from(o - b'0') + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Décode du base64 standard, bourrage `=` facultatif ; `None` pour tout caractère étranger
/// (blancs compris), bourrage mal placé ou longueur impossible.
fn decoder_base64(entree: &[u8]) -> Option<Vec<u8>> {
    let sans = entree
        .strip_suffix(b"==")
        .or_else(|| entree.strip_suffix(b"="));
    let corps = sans.unwrap_or(entree);
    let bourrage = entree.len() - corps.len();
    if (bourrage > 0 && entree.len() % 4 != 0) || corps.len() % 4 == 1 {
        return None;
    }
    let mut sortie = Vec::with_capacity(corps.len() / 4 * 3 + 2);
    let (mut acc, mut bits) = (0u32, 0u32);
    for &o in corps {
        acc = (acc << 6) | valeur_base64(o)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            sortie.push(u8::try_from((acc >> bits) & 0xff).ok()?);
        }
    }
    Some(sortie)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn octets_de(m: &[Morceau]) -> Vec<u8> {
        m.iter()
            .flat_map(|x| match x {
                Morceau::Octets(o) => o.clone(),
                _ => Vec::new(),
            })
            .collect()
    }

    fn marques_de(m: &[Morceau]) -> Vec<MarqueCommande> {
        m.iter()
            .filter_map(|x| match x {
                Morceau::Marque(k) => Some(*k),
                _ => None,
            })
            .collect()
    }

    /// Les morceaux qui ne sont ni des octets ni des marques OSC 133, dans l'ordre.
    fn evenements_de(m: &[Morceau]) -> Vec<Morceau> {
        m.iter()
            .filter(|x| !matches!(x, Morceau::Octets(_) | Morceau::Marque(_)))
            .cloned()
            .collect()
    }

    fn notif(titre: Option<&str>, corps: &str) -> Morceau {
        Morceau::Notification {
            titre: titre.map(String::from),
            corps: corps.into(),
        }
    }

    /// Lit `entree` en une fois, au temps `t`.
    fn lire(l: &mut LecteurMarques, entree: &[u8], t: Instant) -> Vec<Morceau> {
        l.lire(entree, t)
    }

    #[test]
    fn limite_par_lecture_et_par_seconde() {
        let t0 = Instant::now();
        let mut l = LimiteMarques::new();
        l.nouvelle_lecture();
        let admises = (0..1000)
            .filter(|_| l.admettre(&MarqueCommande::Invite, t0))
            .count();
        assert_eq!(admises, MARQUES_PAR_LECTURE as usize);
        // Lectures successives dans la même seconde : plafond par seconde.
        let mut total = admises;
        for _ in 0..1000 {
            l.nouvelle_lecture();
            total += (0..1000)
                .filter(|_| l.admettre(&MarqueCommande::Saisie, t0))
                .count();
        }
        assert_eq!(total, MARQUES_PAR_SECONDE as usize);
        // Nouvelle seconde : ça repart.
        l.nouvelle_lecture();
        assert!(l.admettre(&MarqueCommande::Invite, t0 + Duration::from_millis(1100)));
    }

    #[test]
    fn fin_d_une_commande_transmise_toujours_admise() {
        let t0 = Instant::now();
        let fin = MarqueCommande::Fin {
            code: Some(0),
            duree_ms: None,
        };
        let mut l = LimiteMarques::new();
        l.nouvelle_lecture();
        assert!(l.admettre(&MarqueCommande::Execution, t0));
        for _ in 0..1000 {
            l.admettre(&MarqueCommande::Invite, t0);
        }
        assert!(!l.admettre(&MarqueCommande::Invite, t0));
        assert!(l.admettre(&fin, t0), "le D d'un C transmis passe");
        // Un D sans C transmis compte dans la limite, donc est refusé.
        assert!(!l.admettre(&fin, t0));
    }

    #[test]
    fn commande_complete() {
        let t0 = Instant::now();
        let mut l = LecteurMarques::new();
        let a = lire(&mut l, b"\x1b]133;A\x07$ \x1b]133;B\x07", t0);
        assert_eq!(
            a,
            vec![
                Morceau::Octets(b"\x1b]133;A\x07".to_vec()),
                Morceau::Marque(MarqueCommande::Invite),
                Morceau::Octets(b"$ \x1b]133;B\x07".to_vec()),
                Morceau::Marque(MarqueCommande::Saisie),
            ]
        );
        let c = lire(&mut l, b"\x1b]133;C\x07", t0);
        assert_eq!(marques_de(&c), vec![MarqueCommande::Execution]);
        let d = lire(
            &mut l,
            b"sortie\r\n\x1b]133;D;3\x07",
            t0 + Duration::from_millis(250),
        );
        assert_eq!(
            marques_de(&d),
            vec![MarqueCommande::Fin {
                code: Some(3),
                duree_ms: Some(250)
            }]
        );
        assert_eq!(octets_de(&d), b"sortie\r\n\x1b]133;D;3\x07");
    }

    #[test]
    fn marque_coupee_en_deux() {
        let t0 = Instant::now();
        let entree: &[u8] =
            b"x\x1b]133;A;cl=m\x07y\x1b]133;B\x1b\\z\x1b]133;C\x07w\x1b]133;D;0;extra\x07v";
        let mut ref_l = LecteurMarques::new();
        let ref_m = ref_l.lire(entree, t0);
        let attendu = marques_de(&ref_m);
        assert_eq!(attendu.len(), 4);
        assert_eq!(octets_de(&ref_m), entree);
        for i in 0..=entree.len() {
            let mut l = LecteurMarques::new();
            let mut m = l.lire(&entree[..i], t0);
            m.extend(l.lire(&entree[i..], t0));
            assert_eq!(octets_de(&m), entree, "coupure en {i}");
            assert_eq!(marques_de(&m), attendu, "coupure en {i}");
        }
        // Un octet à la fois.
        let mut l = LecteurMarques::new();
        let mut m = Vec::new();
        for o in entree {
            m.extend(l.lire(std::slice::from_ref(o), t0));
        }
        assert_eq!(octets_de(&m), entree);
        assert_eq!(marques_de(&m), attendu);
    }

    #[test]
    fn terminateur_esc_backslash() {
        let mut l = LecteurMarques::new();
        let m = l.lire(b"\x1b]133;A\x1b\\", Instant::now());
        assert_eq!(marques_de(&m), vec![MarqueCommande::Invite]);
        assert_eq!(octets_de(&m), b"\x1b]133;A\x1b\\");
    }

    #[test]
    fn osc_demesure_abandonne() {
        let mut entree = b"\x1b]133;A;".to_vec();
        entree.extend(std::iter::repeat_n(b'x', OSC_MAX + 10));
        entree.extend(b"\x07fin");
        let mut l = LecteurMarques::new();
        let m = l.lire(&entree, Instant::now());
        assert!(marques_de(&m).is_empty());
        assert_eq!(octets_de(&m), entree);
        // Le lecteur est reparti à zéro.
        let m = l.lire(b"\x1b]133;B\x07", Instant::now());
        assert_eq!(marques_de(&m), vec![MarqueCommande::Saisie]);
    }

    #[test]
    fn marques_ignorees_pendant_une_commande() {
        let t0 = Instant::now();
        let mut l = LecteurMarques::new();
        let m = l.lire(
            b"\x1b]133;C\x07\x1b]133;A\x07\x1b]133;B\x07\x1b]133;C\x07",
            t0,
        );
        assert_eq!(marques_de(&m), vec![MarqueCommande::Execution]);
        let m = l.lire(b"\x1b]133;D;0\x07", t0 + Duration::from_millis(5));
        assert_eq!(
            marques_de(&m),
            vec![MarqueCommande::Fin {
                code: Some(0),
                duree_ms: Some(5)
            }]
        );
        // Après D, l'invite compte de nouveau.
        let m = l.lire(b"\x1b]133;A\x07", t0);
        assert_eq!(marques_de(&m), vec![MarqueCommande::Invite]);
    }

    #[test]
    fn invite_redessinee_avant_la_commande() {
        let t0 = Instant::now();
        let mut l = LecteurMarques::new();
        let m = l.lire(
            b"\x1b]133;A\x07\x1b]133;B\x07\x1b]133;A\x07\x1b]133;B\x07\x1b]133;C\x07",
            t0,
        );
        assert_eq!(marques_de(&m).len(), 5);
        let m = l.lire(b"\x1b]133;D;0\x07", t0 + Duration::from_millis(40));
        assert_eq!(
            marques_de(&m),
            vec![MarqueCommande::Fin {
                code: Some(0),
                duree_ms: Some(40)
            }]
        );
    }

    #[test]
    fn code_illisible_none() {
        let t0 = Instant::now();
        for brut in [
            &b"\x1b]133;D\x07"[..],
            b"\x1b]133;D;\x07",
            b"\x1b]133;D;abc\x07",
            b"\x1b]133;D;99999999999\x07",
            b"\x1b]133;D;;x\x07",
        ] {
            let mut l = LecteurMarques::new();
            let m = l.lire(brut, t0);
            assert_eq!(
                marques_de(&m),
                vec![MarqueCommande::Fin {
                    code: None,
                    duree_ms: None
                }],
                "{brut:?}"
            );
        }
    }

    #[test]
    fn lettres_inconnues_ignorees() {
        let mut l = LecteurMarques::new();
        let entree = b"\x1b]133;P;k=i\x07\x1b]133;Z\x07\x1b]133;AB\x07\x1b]133;\x07\x1b]1337;A\x07";
        let m = l.lire(entree, Instant::now());
        assert!(marques_de(&m).is_empty());
        assert_eq!(octets_de(&m), entree);
    }

    #[test]
    fn autres_osc_intacts() {
        let mut l = LecteurMarques::new();
        let entree = b"\x1b]0;titre\x07\x1b]7;file:///tmp\x1b\\\x1b]8;;http://x\x07lien\x1b]8;;\x07\x1b]99;i=1;x\x07";
        let m = l.lire(entree, Instant::now());
        assert!(marques_de(&m).is_empty());
        assert_eq!(octets_de(&m), entree);
    }

    #[test]
    fn sequence_interrompue_puis_nouvelle_marque() {
        let mut l = LecteurMarques::new();
        let entree = b"\x1b]133;A\x1b[0m\x1b]133;B\x07\x1b]133;\x1b]133;A\x07";
        let m = l.lire(entree, Instant::now());
        assert_eq!(
            marques_de(&m),
            vec![MarqueCommande::Saisie, MarqueCommande::Invite]
        );
        assert_eq!(octets_de(&m), entree);
    }

    #[test]
    fn osc52_copie() {
        let mut l = LecteurMarques::new();
        let entree = b"a\x1b]52;c;QUJD\x07b";
        let m = l.lire(entree, Instant::now());
        assert_eq!(octets_de(&m), entree);
        // L'événement vient juste après l'octet qui termine la séquence.
        assert_eq!(
            m,
            vec![
                Morceau::Octets(b"a\x1b]52;c;QUJD\x07".to_vec()),
                Morceau::PressePapiers("ABC".into()),
                Morceau::Octets(b"b".to_vec()),
            ]
        );
        // Cibles vides, terminateur ST, plusieurs cibles dont `c`, UTF-8, sans bourrage.
        for (brut, attendu) in [
            (&b"\x1b]52;;QUJD\x1b\\"[..], "ABC"),
            (b"\x1b]52;pc;QUJD\x07", "ABC"),
            (b"\x1b]52;c;w6k=\x07", "é"),
            (b"\x1b]52;c;QUI\x07", "AB"),
        ] {
            let m = LecteurMarques::new().lire(brut, Instant::now());
            assert_eq!(
                evenements_de(&m),
                vec![Morceau::PressePapiers(attendu.into())],
                "{brut:?}"
            );
        }
    }

    #[test]
    fn osc52_lecture_ignoree() {
        let entree = b"\x1b]52;c;?\x07\x1b]52;;?\x1b\\";
        let m = LecteurMarques::new().lire(entree, Instant::now());
        assert!(evenements_de(&m).is_empty());
        assert_eq!(octets_de(&m), entree);
        // Une cible qui n'est pas le presse-papiers : ignorée aussi.
        let m = LecteurMarques::new().lire(b"\x1b]52;p;QUJD\x07", Instant::now());
        assert!(evenements_de(&m).is_empty());
    }

    #[test]
    fn osc52_coupe_en_deux() {
        let t0 = Instant::now();
        let entree: &[u8] = b"x\x1b]52;c;QUJD\x07y\x1b]9;hop\x1b\\z\x1b]777;notify;T;C\x07w";
        let ref_m = LecteurMarques::new().lire(entree, t0);
        let attendu = evenements_de(&ref_m);
        assert_eq!(attendu.len(), 3);
        for i in 0..=entree.len() {
            let mut l = LecteurMarques::new();
            let mut m = l.lire(&entree[..i], t0);
            m.extend(l.lire(&entree[i..], t0));
            assert_eq!(octets_de(&m), entree, "coupure en {i}");
            assert_eq!(evenements_de(&m), attendu, "coupure en {i}");
        }
    }

    #[test]
    fn osc52_demesure_ignore() {
        let mut entree = b"\x1b]52;c;".to_vec();
        entree.extend(std::iter::repeat_n(b'A', 2 << 20));
        entree.extend(b"\x07fin");
        let mut l = LecteurMarques::new();
        let mut m = Vec::new();
        for morceau in entree.chunks(65536) {
            m.extend(l.lire(morceau, Instant::now()));
        }
        assert!(evenements_de(&m).is_empty());
        assert_eq!(octets_de(&m), entree);
        assert!(l.memoire() < 2 * OSC_MAX, "mémoire : {}", l.memoire());
        // Le lecteur est reparti à zéro.
        let m = l.lire(b"\x1b]52;c;QUJD\x07", Instant::now());
        assert_eq!(
            evenements_de(&m),
            vec![Morceau::PressePapiers("ABC".into())]
        );
    }

    #[test]
    fn osc52_a_la_borne() {
        // Exactement `PRESSE_PAPIERS_MAX` octets décodés : accepté ; un de plus : refusé.
        let pile = "A".repeat(PRESSE_PAPIERS_MAX);
        let trop = "A".repeat(PRESSE_PAPIERS_MAX + 1);
        for (texte, copie) in [(pile, true), (trop, false)] {
            let mut entree = b"\x1b]52;c;".to_vec();
            entree.extend(base64_pour_test(texte.as_bytes()));
            entree.push(BEL);
            let m = LecteurMarques::new().lire(&entree, Instant::now());
            assert_eq!(octets_de(&m), entree);
            assert_eq!(evenements_de(&m).len(), usize::from(copie));
        }
    }

    fn base64_pour_test(o: &[u8]) -> Vec<u8> {
        const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut s = Vec::new();
        for c in o.chunks(3) {
            let n = c.iter().fold(0u32, |a, &b| a << 8 | u32::from(b)) << (8 * (3 - c.len()));
            for k in 0..=c.len() {
                s.push(T[(n >> (18 - 6 * k) & 63) as usize]);
            }
            s.extend(std::iter::repeat_n(b'=', 3 - c.len()));
        }
        s
    }

    #[test]
    fn osc52_base64_invalide_ignore() {
        for brut in [
            &b"\x1b]52;c;QU*D\x07"[..],
            b"\x1b]52;c;Q\x07",
            b"\x1b]52;c;QUJD=\x07",
            b"\x1b]52;c;QU=D\x07",
            // Octets décodés qui ne sont pas de l'UTF-8.
            b"\x1b]52;c;/w==\x07",
            // Charge vide.
            b"\x1b]52;c;\x07",
            b"\x1b]52;c\x07",
        ] {
            let m = LecteurMarques::new().lire(brut, Instant::now());
            assert!(evenements_de(&m).is_empty(), "{brut:?}");
            assert_eq!(octets_de(&m), brut);
        }
    }

    #[test]
    fn base64_decode() {
        assert_eq!(decoder_base64(b""), Some(vec![]));
        assert_eq!(decoder_base64(b"Zg=="), Some(b"f".to_vec()));
        assert_eq!(decoder_base64(b"Zm8="), Some(b"fo".to_vec()));
        assert_eq!(decoder_base64(b"Zm9v"), Some(b"foo".to_vec()));
        assert_eq!(decoder_base64(b"Zm9vYg"), Some(b"foob".to_vec()));
        assert_eq!(decoder_base64(b"+/+/"), Some(vec![0xfb, 0xff, 0xbf]));
        assert_eq!(decoder_base64(b"Zm9v\n"), None);
        assert_eq!(decoder_base64(b"-_-_"), None);
        assert_eq!(decoder_base64(b"Z"), None);
        assert_eq!(decoder_base64(b"Zg=x"), None);
        let tout: Vec<u8> = (0..=255).collect();
        assert_eq!(decoder_base64(&base64_pour_test(&tout)), Some(tout));
    }

    #[test]
    fn osc9_notification() {
        let entree = b"\x1b]9;Claude: travail fini\x07\x1b]9;fin\x1b\\";
        let m = LecteurMarques::new().lire(entree, Instant::now());
        assert_eq!(octets_de(&m), entree);
        assert_eq!(
            evenements_de(&m),
            vec![notif(None, "Claude: travail fini"), notif(None, "fin")]
        );
    }

    #[test]
    fn osc9_progression_ignoree() {
        let entree = b"\x1b]9;4;3\x07\x1b]9;4\x07\x1b]9;4;1;50\x1b\\\x1b]9;\x07";
        let m = LecteurMarques::new().lire(entree, Instant::now());
        assert!(evenements_de(&m).is_empty());
        assert_eq!(octets_de(&m), entree);
        // `9;42` n'est pas une progression.
        let m = LecteurMarques::new().lire(b"\x1b]9;42 fichiers\x07", Instant::now());
        assert_eq!(evenements_de(&m), vec![notif(None, "42 fichiers")]);
    }

    #[test]
    fn osc777_notification() {
        let entree = b"\x1b]777;notify;Titre;Le corps; avec point-virgule\x07\x1b]777;notify;;seul\x07\x1b]777;notify;Titre seul\x07";
        let m = LecteurMarques::new().lire(entree, Instant::now());
        assert_eq!(octets_de(&m), entree);
        assert_eq!(
            evenements_de(&m),
            vec![
                notif(Some("Titre"), "Le corps; avec point-virgule"),
                notif(None, "seul"),
                notif(None, "Titre seul"),
            ]
        );
        // Autres sous-commandes de 777 : ignorées.
        let m = LecteurMarques::new().lire(
            b"\x1b]777;precmd\x07\x1b]777;notifyx;a;b\x07",
            Instant::now(),
        );
        assert!(evenements_de(&m).is_empty());
    }

    #[test]
    fn controles_retires() {
        let m = LecteurMarques::new().lire(
            "\x1b]777;notify;Ti\u{1}tre\u{7f};co\r\nrps\u{9b}\x07".as_bytes(),
            Instant::now(),
        );
        assert_eq!(evenements_de(&m), vec![notif(Some("Titre"), "corps")]);
        let m = LecteurMarques::new().lire(b"\x1b]9;a\x08b\x07\x1b]9;\x01\x02\x07", Instant::now());
        assert_eq!(evenements_de(&m), vec![notif(None, "ab")]);
    }

    #[test]
    fn notification_longue_tronquee() {
        let mut entree = b"\x1b]9;".to_vec();
        entree.extend("é".repeat(OSC_MAX / 2 - 10).as_bytes());
        entree.push(BEL);
        let m = LecteurMarques::new().lire(&entree, Instant::now());
        let evts = evenements_de(&m);
        let [Morceau::Notification { corps, .. }] = &evts[..] else {
            panic!("{evts:?}");
        };
        assert_eq!(corps.chars().count(), NOTIFICATION_MAX + 1);
        assert!(corps.ends_with('…'));
        assert!(corps.starts_with("éé"));
    }

    #[test]
    fn evenements_limites_comme_les_marques() {
        let t0 = Instant::now();
        let mut l = LimiteMarques::new();
        l.nouvelle_lecture();
        let admises = (0..1000).filter(|_| l.admettre_evenement(t0)).count();
        assert_eq!(admises, MARQUES_PAR_LECTURE as usize);
    }

    #[test]
    fn osc_inconnus_et_voisins_ignores() {
        let entree = b"\x1b]5;x\x07\x1b]53;c;QUJD\x07\x1b]99;i=1:d=0;x\x07\x1b]7777;a\x07\x1b]7501;x\x07\x1b]21337;x\x07";
        let m = LecteurMarques::new().lire(entree, Instant::now());
        assert!(evenements_de(&m).is_empty());
        assert!(marques_de(&m).is_empty());
        assert_eq!(octets_de(&m), entree);
    }

    #[test]
    fn binaire_quelconque_sans_paniquer() {
        let mut l = LecteurMarques::new();
        let mut x: u32 = 12345;
        let mut entree = Vec::new();
        for _ in 0..200_000 {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
            entree.push((x >> 16) as u8);
        }
        let m = l.lire(&entree, Instant::now());
        assert_eq!(octets_de(&m), entree);
    }
}
