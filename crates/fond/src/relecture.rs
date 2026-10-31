// SPDX-License-Identifier: GPL-3.0-or-later
//! Tampon de relecture : les derniers octets affichés par une session, gardés en mémoire
//! (jamais sur disque) pour être rejoués quand une interface se (re)connecte.

use std::collections::VecDeque;

/// Capacité par session : 8 Mio.
pub const REPLAY_CAPACITY: usize = 8 * 1024 * 1024;

/// Anneau d'octets borné : au-delà de la capacité, les plus anciens octets sont écartés.
#[derive(Debug)]
pub struct ReplayBuffer {
    octets: VecDeque<u8>,
    capacite: usize,
    /// Vrai dès que des octets ont été écartés : le début du tampon peut alors être
    /// au milieu d'une ligne, d'un caractère UTF-8 ou d'une séquence d'échappement.
    coupe: bool,
}

impl ReplayBuffer {
    pub fn new(capacity: usize) -> Self {
        ReplayBuffer {
            octets: VecDeque::new(),
            capacite: capacity,
            coupe: false,
        }
    }

    /// Ajoute des octets ; seuls les `capacite` derniers sont gardés.
    pub fn push(&mut self, data: &[u8]) {
        if data.len() >= self.capacite {
            // Le nouveau morceau remplit tout le tampon à lui seul.
            self.coupe |= !self.octets.is_empty() || data.len() > self.capacite;
            self.octets.clear();
            self.octets.extend(&data[data.len() - self.capacite..]);
            return;
        }
        let exces = (self.octets.len() + data.len()).saturating_sub(self.capacite);
        if exces > 0 {
            self.octets.drain(..exces);
            self.coupe = true;
        }
        self.octets.extend(data);
    }

    /// Copie du contenu à rejouer. Si le tampon a été coupé, commence au premier début
    /// de ligne suivant la coupe (pas de séquence tronquée rejouée) ; sans fin de ligne,
    /// tout le contenu.
    pub fn snapshot(&self) -> Vec<u8> {
        let debut = if self.coupe {
            self.octets
                .iter()
                .position(|&c| c == b'\n')
                .map_or(0, |i| i + 1)
        } else {
            0
        };
        self.octets.range(debut..).copied().collect()
    }

    pub fn len(&self) -> usize {
        self.octets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.octets.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conserve_tout_sous_la_capacite() {
        let mut t = ReplayBuffer::new(16);
        t.push(b"abc\n");
        assert_eq!(t.snapshot(), b"abc\n");
        t.push(b"def");
        assert_eq!(t.snapshot(), b"abc\ndef");
        assert_eq!(t.len(), 7);
    }

    #[test]
    fn tampon_reste_borne() {
        let mut t = ReplayBuffer::new(1024);
        t.push(&vec![b'x'; 10 * 1024 * 1024]);
        assert!(t.len() <= 1024);
        // Par petits morceaux aussi.
        for i in 0..100_000u32 {
            t.push(&i.to_le_bytes());
        }
        assert_eq!(t.len(), 1024);
    }

    #[test]
    fn garde_les_octets_les_plus_recents() {
        let mut t = ReplayBuffer::new(4);
        t.push(b"\n12");
        t.push(b"345");
        t.push(b"6");
        assert_eq!(t.len(), 4);
        // Coupé et sans fin de ligne : tout le contenu.
        assert_eq!(t.snapshot(), b"3456");
    }

    #[test]
    fn reprend_au_debut_de_ligne() {
        let mut t = ReplayBuffer::new(8);
        t.push(b"aaaa\nbbbb\ncc");
        // Gardé : "\nbbbb\ncc" ; on rejoue à partir du début de ligne suivant la coupe.
        assert_eq!(t.snapshot(), b"bbbb\ncc");
    }

    #[test]
    fn coupe_dans_une_sequence_d_echappement() {
        let mut t = ReplayBuffer::new(10);
        t.push(b"\x1b[38;2;1;2;3mX\nOK");
        let s = t.snapshot();
        assert_eq!(s, b"OK", "la séquence tronquée n'est pas rejouée");
    }

    #[test]
    fn tampon_coupe_sans_paniquer() {
        for capacite in [0, 1, 2, 3, 7, 64] {
            let mut t = ReplayBuffer::new(capacite);
            for _ in 0..100 {
                t.push("é".as_bytes());
            }
            assert!(t.len() <= capacite);
            let _ = t.snapshot();
        }
    }

    #[test]
    fn capacite_nulle_ne_garde_rien() {
        let mut t = ReplayBuffer::new(0);
        t.push(b"abc\n");
        assert!(t.is_empty());
        assert!(t.snapshot().is_empty());
    }
}
