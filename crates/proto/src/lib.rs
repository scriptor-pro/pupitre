// SPDX-License-Identifier: GPL-3.0-or-later
//! Messages échangés entre l'interface (`pupitre`) et le processus de fond (`pupitre-fond`).
//!
//! Chaque message voyage dans une trame : longueur `u32` petit-boutiste, puis la charge
//! encodée en postcard (voir [`trame`]).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub mod ssh;
pub mod trame;
pub use ssh::{EtatTrousseau, Genre};
pub use trame::{read_frame, write_frame, FrameError, MAX_FRAME};

/// Version courante du protocole ; échangée dans `Hello`/`Welcome`.
/// La version 2 ajoute les demandes de l'askpass SSH ; elle reste compatible avec la 1.
pub const VERSION: u32 = 2;
/// Plus ancienne version encore acceptée par le processus de fond.
pub const VERSION_MIN: u32 = 1;

/// Identifiant d'une demande de saisie (phrase de passe, confirmation…), unique dans un
/// processus de fond.
pub type DemandeId = u64;

/// Identifiant d'une session (un shell dans un PTY), unique dans un processus de fond.
pub type SessionId = u64;

/// Messages envoyés par l'interface au processus de fond.
#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub enum ClientMsg {
    Hello {
        version: u32,
    },
    /// Ouvre une session ; `argv` absent = shell de connexion.
    Open {
        argv: Option<Vec<String>>,
        cwd: Option<PathBuf>,
        cols: u16,
        rows: u16,
    },
    Input {
        id: SessionId,
        data: Vec<u8>,
    },
    Resize {
        id: SessionId,
        cols: u16,
        rows: u16,
    },
    Close {
        id: SessionId,
    },
    List,
    /// Demande le tampon de relecture puis le flux d'une session existante.
    Attach {
        id: SessionId,
    },
    // Variantes de la version 2 : toujours à la fin, pour ne pas changer l'encodage des
    // précédentes.
    /// Envoyé par `pupitre-askpass` : une invite SSH attend une réponse.
    Demander {
        session: Option<SessionId>,
        invite: String,
        genre: Genre,
        trousseau: EtatTrousseau,
    },
    /// Réponse de l'interface à une demande ; `None` = refus ou annulation. Pour
    /// `Confirmation` et `HoteInconnu` : `Some("yes")` ou `Some("no")`.
    Repondre {
        demande: DemandeId,
        reponse: Option<String>,
        memoriser: bool,
    },
    /// Charge une clé SSH dans l'agent.
    ChargerCle {
        cle: PathBuf,
    },
}

/// Messages envoyés par le processus de fond à l'interface.
#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub enum ServerMsg {
    Welcome {
        version: u32,
    },
    Opened {
        id: SessionId,
    },
    OpenFailed {
        error: String,
    },
    Output {
        id: SessionId,
        data: Vec<u8>,
    },
    Replay {
        id: SessionId,
        data: Vec<u8>,
    },
    Exited {
        id: SessionId,
        code: Option<i32>,
    },
    Sessions(Vec<SessionInfo>),
    Busy {
        id: SessionId,
        busy: bool,
    },
    Error(String),
    // Variantes de la version 2 : jamais envoyées à un client en version 1.
    /// Une invite SSH attend une réponse de l'utilisateur.
    Demande {
        demande: DemandeId,
        session: Option<SessionId>,
        invite: String,
        genre: Genre,
        trousseau: EtatTrousseau,
    },
    /// La demande est réglée (répondue ailleurs, annulée ou expirée) : fermer son dialogue.
    DemandeClose {
        demande: DemandeId,
    },
    /// Réponse transmise à `pupitre-askpass`.
    Reponse {
        reponse: Option<String>,
        memoriser: bool,
    },
    /// Texte à montrer dans le bandeau de toutes les fenêtres.
    Message(String),
}

/// Réponse d'un utilisateur pour `Debug` : jamais son contenu (phrase de passe).
struct Masque<'a>(&'a Option<String>);

impl std::fmt::Debug for Masque<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            Some(_) => f.write_str("Some(«masqué»)"),
            None => f.write_str("None"),
        }
    }
}

// `Debug` écrit à la main : une réponse (phrase de passe) ne doit jamais apparaître dans
// un message d'erreur ou un journal.
impl std::fmt::Debug for ClientMsg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientMsg::Hello { version } => {
                f.debug_struct("Hello").field("version", version).finish()
            }
            ClientMsg::Open {
                argv,
                cwd,
                cols,
                rows,
            } => f
                .debug_struct("Open")
                .field("argv", argv)
                .field("cwd", cwd)
                .field("cols", cols)
                .field("rows", rows)
                .finish(),
            ClientMsg::Input { id, data } => f
                .debug_struct("Input")
                .field("id", id)
                .field("data", data)
                .finish(),
            ClientMsg::Resize { id, cols, rows } => f
                .debug_struct("Resize")
                .field("id", id)
                .field("cols", cols)
                .field("rows", rows)
                .finish(),
            ClientMsg::Close { id } => f.debug_struct("Close").field("id", id).finish(),
            ClientMsg::List => f.write_str("List"),
            ClientMsg::Attach { id } => f.debug_struct("Attach").field("id", id).finish(),
            ClientMsg::Demander {
                session,
                invite,
                genre,
                trousseau,
            } => f
                .debug_struct("Demander")
                .field("session", session)
                .field("invite", invite)
                .field("genre", genre)
                .field("trousseau", trousseau)
                .finish(),
            ClientMsg::Repondre {
                demande,
                reponse,
                memoriser,
            } => f
                .debug_struct("Repondre")
                .field("demande", demande)
                .field("reponse", &Masque(reponse))
                .field("memoriser", memoriser)
                .finish(),
            ClientMsg::ChargerCle { cle } => {
                f.debug_struct("ChargerCle").field("cle", cle).finish()
            }
        }
    }
}

impl std::fmt::Debug for ServerMsg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServerMsg::Welcome { version } => {
                f.debug_struct("Welcome").field("version", version).finish()
            }
            ServerMsg::Opened { id } => f.debug_struct("Opened").field("id", id).finish(),
            ServerMsg::OpenFailed { error } => {
                f.debug_struct("OpenFailed").field("error", error).finish()
            }
            ServerMsg::Output { id, data } => f
                .debug_struct("Output")
                .field("id", id)
                .field("data", data)
                .finish(),
            ServerMsg::Replay { id, data } => f
                .debug_struct("Replay")
                .field("id", id)
                .field("data", data)
                .finish(),
            ServerMsg::Exited { id, code } => f
                .debug_struct("Exited")
                .field("id", id)
                .field("code", code)
                .finish(),
            ServerMsg::Sessions(v) => f.debug_tuple("Sessions").field(v).finish(),
            ServerMsg::Busy { id, busy } => f
                .debug_struct("Busy")
                .field("id", id)
                .field("busy", busy)
                .finish(),
            ServerMsg::Error(e) => f.debug_tuple("Error").field(e).finish(),
            ServerMsg::Demande {
                demande,
                session,
                invite,
                genre,
                trousseau,
            } => f
                .debug_struct("Demande")
                .field("demande", demande)
                .field("session", session)
                .field("invite", invite)
                .field("genre", genre)
                .field("trousseau", trousseau)
                .finish(),
            ServerMsg::DemandeClose { demande } => f
                .debug_struct("DemandeClose")
                .field("demande", demande)
                .finish(),
            ServerMsg::Reponse { reponse, memoriser } => f
                .debug_struct("Reponse")
                .field("reponse", &Masque(reponse))
                .field("memoriser", memoriser)
                .finish(),
            ServerMsg::Message(t) => f.debug_tuple("Message").field(t).finish(),
        }
    }
}

impl ServerMsg {
    /// Nom de la variante, sans aucun contenu : pour les messages d'erreur.
    pub fn nom(&self) -> &'static str {
        match self {
            ServerMsg::Welcome { .. } => "Welcome",
            ServerMsg::Opened { .. } => "Opened",
            ServerMsg::OpenFailed { .. } => "OpenFailed",
            ServerMsg::Output { .. } => "Output",
            ServerMsg::Replay { .. } => "Replay",
            ServerMsg::Exited { .. } => "Exited",
            ServerMsg::Sessions(_) => "Sessions",
            ServerMsg::Busy { .. } => "Busy",
            ServerMsg::Error(_) => "Error",
            ServerMsg::Demande { .. } => "Demande",
            ServerMsg::DemandeClose { .. } => "DemandeClose",
            ServerMsg::Reponse { .. } => "Reponse",
            ServerMsg::Message(_) => "Message",
        }
    }

    /// Vrai pour les variantes introduites par la version 2, qu'un client en version 1
    /// ne sait pas décoder.
    pub fn est_v2(&self) -> bool {
        matches!(
            self,
            ServerMsg::Demande { .. }
                | ServerMsg::DemandeClose { .. }
                | ServerMsg::Reponse { .. }
                | ServerMsg::Message(_)
        )
    }
}

/// Description d'une session, renvoyée par `List`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SessionInfo {
    pub id: SessionId,
    pub cols: u16,
    pub rows: u16,
    pub busy: bool,
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// Octets postcard relevés sur la version 1 du protocole, avant l'ajout de la version 2 :
    /// ils ne doivent jamais changer (les variantes nouvelles vont à la fin des énumérations).
    #[test]
    fn encodage_v1_inchange() {
        let c = |m: &ClientMsg| postcard::to_stdvec(m).unwrap();
        let s = |m: &ServerMsg| postcard::to_stdvec(m).unwrap();
        assert_eq!(c(&ClientMsg::Hello { version: 1 }), [0, 1]);
        assert_eq!(
            c(&ClientMsg::Open {
                argv: Some(vec!["sh".into(), "-c".into()]),
                cwd: Some(PathBuf::from("/tmp")),
                cols: 80,
                rows: 24,
            }),
            [1, 1, 2, 2, 115, 104, 2, 45, 99, 1, 4, 47, 116, 109, 112, 80, 24]
        );
        assert_eq!(c(&ClientMsg::Attach { id: 3 }), [6, 3]);
        assert_eq!(s(&ServerMsg::Welcome { version: 1 }), [0, 1]);
        assert_eq!(
            s(&ServerMsg::Output {
                id: 1,
                data: vec![0xc3, b'a']
            }),
            [3, 1, 2, 195, 97]
        );
        assert_eq!(s(&ServerMsg::Error("ab".into())), [8, 2, 97, 98]);
    }

    #[test]
    fn nouvelles_variantes_aller_retour() {
        fn ar<T: Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug>(m: T) {
            let o = postcard::to_stdvec(&m).unwrap();
            assert_eq!(postcard::from_bytes::<T>(&o).unwrap(), m);
        }
        let genre = Genre::PhraseCle {
            cle: PathBuf::from("/home/u/.ssh/id_ed25519"),
            nouvel_essai: true,
        };
        ar(ClientMsg::Demander {
            session: Some(4),
            invite: "Enter passphrase".into(),
            genre: genre.clone(),
            trousseau: EtatTrousseau::Verrouille,
        });
        ar(ClientMsg::Demander {
            session: None,
            invite: String::new(),
            genre: Genre::Texte { masque: true },
            trousseau: EtatTrousseau::Absent,
        });
        ar(ClientMsg::Repondre {
            demande: 9,
            reponse: Some("yes".into()),
            memoriser: true,
        });
        ar(ClientMsg::Repondre {
            demande: 9,
            reponse: None,
            memoriser: false,
        });
        ar(ClientMsg::ChargerCle {
            cle: PathBuf::from("/k"),
        });
        ar(ServerMsg::Demande {
            demande: 1,
            session: Some(2),
            invite: "?".into(),
            genre,
            trousseau: EtatTrousseau::Disponible,
        });
        ar(ServerMsg::DemandeClose { demande: 1 });
        ar(ServerMsg::Reponse {
            reponse: Some("x".into()),
            memoriser: false,
        });
        ar(ServerMsg::Message("texte".into()));
        // Les nouvelles variantes suivent les anciennes : indices 9 (client) et 10 (serveur) pour les dernières.
        assert_eq!(
            postcard::to_stdvec(&ClientMsg::ChargerCle { cle: "a".into() }).unwrap()[0],
            9
        );
        assert_eq!(
            postcard::to_stdvec(&ServerMsg::DemandeClose { demande: 0 }).unwrap()[0],
            10
        );
    }

    #[test]
    fn debug_ne_montre_pas_la_phrase() {
        let r = ClientMsg::Repondre {
            demande: 1,
            reponse: Some("PHRASE-SECRETE".into()),
            memoriser: false,
        };
        let a = ServerMsg::Reponse {
            reponse: Some("PHRASE-SECRETE".into()),
            memoriser: true,
        };
        for t in [format!("{r:?}"), format!("{a:?}"), format!("{r:#?}")] {
            assert!(!t.contains("PHRASE-SECRETE"), "{t}");
            assert!(t.contains("Some(«masqué»)"), "{t}");
        }
        assert_eq!(a.nom(), "Reponse");
        // L'encodage n'est pas touché.
        let o = postcard::to_stdvec(&a).unwrap();
        assert!(o.windows(14).any(|w| w == b"PHRASE-SECRETE"));
    }
}
