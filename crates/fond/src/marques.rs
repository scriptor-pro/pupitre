// SPDX-License-Identifier: GPL-3.0-or-later
//! Lecture des marques de commandes (OSC 133) dans le flux d'une session.

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

/// Un morceau du flux, dans l'ordre : des octets à relayer, ou une marque vue juste après.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Morceau {
    Octets(Vec<u8>),
    Marque(MarqueCommande),
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
        match marque {
            MarqueCommande::Execution => self.execution_transmise = true,
            MarqueCommande::Fin { .. } => self.execution_transmise = false,
            _ => {}
        }
        true
    }
}

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;
/// Début d'une séquence qui nous intéresse, après `ESC ]`.
const PREFIXE: &[u8] = b"133;";

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

/// Lecteur des marques OSC 133 (`ESC ] 133 ; A|B|C|D [; …]`, terminée par `BEL` ou
/// `ESC \`). Il ne modifie ni ne retarde le flux : les octets sont rendus tels quels, la
/// marque est rendue juste après l'octet qui la termine. Seul le début de la séquence en
/// cours est copié, et jamais plus de [`OSC_MAX`] octets.
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
            if let Some(marque) = self.avancer(o, maintenant) {
                if let Some(avant) = octets.get(debut..i) {
                    rendu.push(Morceau::Octets(avant.to_vec()));
                }
                rendu.push(Morceau::Marque(marque));
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

    /// Fait avancer l'automate d'un octet ; rend la marque qu'il termine, s'il y en a une.
    fn avancer(&mut self, o: u8, maintenant: Instant) -> Option<MarqueCommande> {
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
                    // Autre OSC que 133, ou trop longue : on ne la suit plus.
                    let n = self.corps.len().min(PREFIXE.len());
                    if self.corps.len() > OSC_MAX || self.corps.get(..n) != PREFIXE.get(..n) {
                        return self.abandonner();
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

    fn abandonner(&mut self) -> Option<MarqueCommande> {
        self.etat = Etat::Normal;
        self.corps.clear();
        None
    }

    /// La séquence est complète : interprète son contenu.
    fn terminer(&mut self, maintenant: Instant) -> Option<MarqueCommande> {
        self.etat = Etat::Normal;
        let corps = std::mem::take(&mut self.corps);
        let reste = corps.strip_prefix(PREFIXE)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn octets_de(m: &[Morceau]) -> Vec<u8> {
        m.iter()
            .flat_map(|x| match x {
                Morceau::Octets(o) => o.clone(),
                Morceau::Marque(_) => Vec::new(),
            })
            .collect()
    }

    fn marques_de(m: &[Morceau]) -> Vec<MarqueCommande> {
        m.iter()
            .filter_map(|x| match x {
                Morceau::Marque(k) => Some(*k),
                Morceau::Octets(_) => None,
            })
            .collect()
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
        let entree = b"\x1b]0;titre\x07\x1b]7;file:///tmp\x1b\\\x1b]8;;http://x\x07lien\x1b]8;;\x07\x1b]52;c;QUJD\x07";
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
