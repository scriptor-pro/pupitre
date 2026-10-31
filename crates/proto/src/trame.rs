// SPDX-License-Identifier: GPL-3.0-or-later
//! Trames : longueur `u32` petit-boutiste suivie d'une charge postcard.

use serde::{de::DeserializeOwned, Serialize};
use std::io::{self, Read, Write};

/// Taille maximale d'une charge (16 Mio) ; au-delà, la trame est refusée sans être lue.
pub const MAX_FRAME: usize = 16 * 1024 * 1024;

/// Erreur de lecture d'une trame.
#[derive(Debug)]
pub enum FrameError {
    Io(io::Error),
    TooLarge(usize),
    Decode(String),
    /// Le flux s'est terminé (proprement ou au milieu d'une trame).
    Closed,
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameError::Io(e) => write!(f, "erreur d'entrée-sortie : {e}"),
            FrameError::TooLarge(n) => {
                write!(f, "trame trop grande : {n} octets (max. {MAX_FRAME})")
            }
            FrameError::Decode(e) => write!(f, "trame illisible : {e}"),
            FrameError::Closed => write!(f, "connexion fermée"),
        }
    }
}

impl std::error::Error for FrameError {}

/// Écrit `msg` dans une trame. Une charge de plus de [`MAX_FRAME`] octets est refusée
/// et rien n'est écrit. L'en-tête et la charge partent en un seul `write_all`.
pub fn write_frame<W: Write, T: Serialize>(w: &mut W, msg: &T) -> io::Result<()> {
    // Réserve l'en-tête, puis encode la charge à la suite.
    let mut buf = postcard::to_extend(msg, vec![0u8; 4])
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let longueur = buf.len() - 4;
    if longueur > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            FrameError::TooLarge(longueur).to_string(),
        ));
    }
    // longueur ≤ MAX_FRAME < u32::MAX : la conversion ne tronque pas.
    buf[..4].copy_from_slice(&(longueur as u32).to_le_bytes());
    w.write_all(&buf)?;
    w.flush()
}

/// Lit une trame et la décode. Ne panique jamais, quelles que soient les données reçues ;
/// une longueur annoncée trop grande est refusée avant toute allocation.
pub fn read_frame<R: Read, T: DeserializeOwned>(r: &mut R) -> Result<T, FrameError> {
    let mut entete = [0u8; 4];
    lire_exactement(r, &mut entete)?;
    let longueur = u32::from_le_bytes(entete) as usize;
    if longueur > MAX_FRAME {
        return Err(FrameError::TooLarge(longueur));
    }
    let mut charge = vec![0u8; longueur];
    lire_exactement(r, &mut charge)?;
    match postcard::take_from_bytes::<T>(&charge) {
        Ok((msg, [])) => Ok(msg),
        Ok((_, reste)) => Err(FrameError::Decode(format!(
            "{} octet(s) en trop après le message",
            reste.len()
        ))),
        Err(e) => Err(FrameError::Decode(e.to_string())),
    }
}

/// Comme `read_exact`, mais une fin de flux (même au milieu) donne `Closed`.
fn lire_exactement<R: Read>(r: &mut R, buf: &mut [u8]) -> Result<(), FrameError> {
    r.read_exact(buf).map_err(|e| match e.kind() {
        io::ErrorKind::UnexpectedEof => FrameError::Closed,
        _ => FrameError::Io(e),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ClientMsg, ServerMsg, SessionInfo};
    use std::io::Cursor;
    use std::path::PathBuf;

    fn aller_retour<T>(msg: T)
    where
        T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
    {
        let mut buf = Vec::new();
        write_frame(&mut buf, &msg).unwrap();
        let mut r = Cursor::new(buf);
        let relu: T = read_frame(&mut r).unwrap();
        assert_eq!(relu, msg);
        // Rien après la trame : la lecture suivante voit la fin du flux.
        assert!(matches!(
            read_frame::<_, T>(&mut r),
            Err(FrameError::Closed)
        ));
    }

    #[test]
    fn aller_retour_chaque_message() {
        aller_retour(ClientMsg::Hello {
            version: crate::VERSION,
        });
        aller_retour(ClientMsg::Open {
            argv: None,
            cwd: None,
            cols: 80,
            rows: 24,
        });
        aller_retour(ClientMsg::Open {
            argv: Some(vec!["/bin/bash".into(), "-l".into()]),
            cwd: Some(PathBuf::from("/home/é")),
            cols: u16::MAX,
            rows: 0,
        });
        aller_retour(ClientMsg::Input {
            id: 1,
            data: vec![0, 0xff, 0x1b, b'['],
        });
        aller_retour(ClientMsg::Resize {
            id: u64::MAX,
            cols: 120,
            rows: 40,
        });
        aller_retour(ClientMsg::Close { id: 7 });
        aller_retour(ClientMsg::List);
        aller_retour(ClientMsg::Attach { id: 3 });

        aller_retour(ServerMsg::Welcome {
            version: crate::VERSION,
        });
        aller_retour(ServerMsg::Opened { id: 1 });
        aller_retour(ServerMsg::OpenFailed {
            error: "shell introuvable".into(),
        });
        aller_retour(ServerMsg::Output {
            id: 1,
            data: vec![0xc3],
        });
        aller_retour(ServerMsg::Replay {
            id: 1,
            data: Vec::new(),
        });
        aller_retour(ServerMsg::Exited {
            id: 1,
            code: Some(-1),
        });
        aller_retour(ServerMsg::Exited { id: 1, code: None });
        aller_retour(ServerMsg::Sessions(vec![
            SessionInfo {
                id: 1,
                cols: 80,
                rows: 24,
                busy: false,
            },
            SessionInfo {
                id: 2,
                cols: 100,
                rows: 30,
                busy: true,
            },
        ]));
        aller_retour(ServerMsg::Sessions(Vec::new()));
        aller_retour(ServerMsg::Busy { id: 2, busy: true });
        aller_retour(ServerMsg::Error("protocole".into()));
    }

    #[test]
    fn plusieurs_trames_a_la_suite() {
        let mut buf = Vec::new();
        write_frame(&mut buf, &ClientMsg::List).unwrap();
        write_frame(&mut buf, &ClientMsg::Close { id: 9 }).unwrap();
        let mut r = Cursor::new(buf);
        assert_eq!(read_frame::<_, ClientMsg>(&mut r).unwrap(), ClientMsg::List);
        assert_eq!(
            read_frame::<_, ClientMsg>(&mut r).unwrap(),
            ClientMsg::Close { id: 9 }
        );
    }

    #[test]
    fn trame_trop_grande_refusee() {
        let entete = ((MAX_FRAME + 1) as u32).to_le_bytes();
        let r = read_frame::<_, ClientMsg>(&mut Cursor::new(entete.to_vec()));
        assert!(matches!(r, Err(FrameError::TooLarge(n)) if n == MAX_FRAME + 1));
    }

    #[test]
    fn ecriture_trop_grande_refusee() {
        let msg = ServerMsg::Output {
            id: 1,
            data: vec![0; MAX_FRAME],
        };
        let mut buf = Vec::new();
        assert!(write_frame(&mut buf, &msg).is_err());
        assert!(buf.is_empty(), "rien ne doit être écrit");
    }

    #[test]
    fn flux_coupe_donne_closed() {
        let vide: &[u8] = &[];
        assert!(matches!(
            read_frame::<_, ClientMsg>(&mut Cursor::new(vide)),
            Err(FrameError::Closed)
        ));
        let tronque: &[u8] = &[5, 0];
        assert!(matches!(
            read_frame::<_, ClientMsg>(&mut Cursor::new(tronque)),
            Err(FrameError::Closed)
        ));
        // En-tête complet, charge incomplète.
        let charge_coupee: &[u8] = &[10, 0, 0, 0, 1, 2];
        assert!(matches!(
            read_frame::<_, ClientMsg>(&mut Cursor::new(charge_coupee)),
            Err(FrameError::Closed)
        ));
    }

    #[test]
    fn charge_corrompue_donne_decode() {
        let corrompu: &[u8] = &[3, 0, 0, 0, 0xff, 0xff, 0xff];
        assert!(matches!(
            read_frame::<_, ClientMsg>(&mut Cursor::new(corrompu)),
            Err(FrameError::Decode(_))
        ));
    }

    #[test]
    fn octets_en_trop_dans_la_charge_donnent_decode() {
        // ClientMsg::List suivi d'un octet parasite dans la même trame.
        let mut charge = postcard::to_stdvec(&ClientMsg::List).unwrap();
        charge.push(0);
        let mut buf = (charge.len() as u32).to_le_bytes().to_vec();
        buf.extend(charge);
        assert!(matches!(
            read_frame::<_, ClientMsg>(&mut Cursor::new(buf)),
            Err(FrameError::Decode(_))
        ));
    }

    #[test]
    fn erreur_io_est_remontee() {
        struct Panne;
        impl Read for Panne {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::new(io::ErrorKind::PermissionDenied, "panne"))
            }
        }
        assert!(matches!(
            read_frame::<_, ClientMsg>(&mut Panne),
            Err(FrameError::Io(_))
        ));
    }
}
