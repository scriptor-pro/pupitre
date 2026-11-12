// SPDX-License-Identifier: GPL-3.0-or-later
//! Messages échangés entre l'interface (`pupitre`) et le processus de fond (`pupitre-fond`).
//!
//! Chaque message voyage dans une trame : longueur `u32` petit-boutiste, puis la charge
//! encodée en postcard (voir [`trame`]).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use zeroize::Zeroizing;

pub mod ssh;
pub mod trame;
pub use ssh::{EtatTrousseau, Genre};
pub use trame::{read_frame, write_frame, FrameError, MAX_FRAME};

/// Version courante du protocole ; échangée dans `Hello`/`Welcome`.
/// La version 2 ajoute les demandes de l'askpass SSH ; la version 3 l'ouverture d'un onglet
/// avec des variables d'environnement, les marques de commandes et l'état « trousseau
/// lent » ; la version 4 la copie dans le presse-papiers (OSC 52) et les notifications
/// demandées par un programme (OSC 9, OSC 777). Chacune reste compatible avec les précédentes.
pub const VERSION: u32 = 4;
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
        reponse: Option<Zeroizing<String>>,
        memoriser: bool,
    },
    /// Charge une clé SSH dans l'agent.
    ChargerCle {
        cle: PathBuf,
    },
    // Variantes de la version 3 : à la fin, et jamais envoyées à un fond en version 2.
    /// Ouvre une session avec des variables d'environnement en plus de celles du fond ;
    /// `argv` vide = shell de connexion.
    Ouvrir {
        argv: Vec<String>,
        cwd: Option<PathBuf>,
        cols: u16,
        rows: u16,
        env: Vec<(String, String)>,
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
        reponse: Option<Zeroizing<String>>,
        memoriser: bool,
    },
    /// Texte à montrer dans le bandeau de toutes les fenêtres.
    Message(String),
    // Variante de la version 3 : jamais envoyée à un client en version 1 ou 2.
    /// Une marque de commande a été vue dans la sortie de la session ; `rejouee` vrai si
    /// elle vient du tampon de relecture et non du flux en direct.
    Marque {
        id: SessionId,
        marque: MarqueCommande,
        rejouee: bool,
    },
    /// Rejeu d'un client en version 3 (réponse à `Attach`) : comme `Replay`, avec les marques
    /// vues dans `data`, chacune à son décalage en octets (croissant, au plus `data.len()` ;
    /// la marque vient juste avant l'octet de ce rang). Variante ajoutée en fin d'énumération.
    ReplayMarque {
        id: SessionId,
        data: Vec<u8>,
        marques: Vec<(u32, MarqueCommande)>,
    },
    /// Version 4 : un programme a demandé de copier `texte` dans le presse-papiers (OSC 52,
    /// écriture seulement), vu en direct ; jamais rejoué à la reprise. Variante ajoutée en fin
    /// d'énumération.
    PressePapiers {
        id: SessionId,
        texte: String,
    },
    /// Version 4 : un programme demande une notification (OSC 9, OSC 777), vue en direct ;
    /// jamais rejouée à la reprise. Variante ajoutée en fin d'énumération.
    Notification {
        id: SessionId,
        titre: Option<String>,
        corps: String,
    },
}

/// Étape d'une commande, repérée dans la sortie du shell (séquences OSC 133 A, B, C, D).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarqueCommande {
    /// A : l'invite commence.
    Invite,
    /// B : l'invite est finie, la saisie commence.
    Saisie,
    /// C : la commande saisie s'exécute.
    Execution,
    /// D : la commande est finie, avec son code de sortie et sa durée si le shell les donne.
    Fin {
        code: Option<i32>,
        duree_ms: Option<u64>,
    },
}

/// Réponse d'un utilisateur pour `Debug` : jamais son contenu (phrase de passe).
struct Masque<'a>(&'a Option<Zeroizing<String>>);

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
            // Les variables d'environnement peuvent porter des secrets : seuls les noms
            // sont montrés.
            ClientMsg::Ouvrir {
                argv,
                cwd,
                cols,
                rows,
                env,
            } => f
                .debug_struct("Ouvrir")
                .field("argv", argv)
                .field("cwd", cwd)
                .field("cols", cols)
                .field("rows", rows)
                .field(
                    "env",
                    &env.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
                )
                .finish(),
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
            ServerMsg::ReplayMarque { id, data, marques } => f
                .debug_struct("ReplayMarque")
                .field("id", id)
                .field("data", data)
                .field("marques", marques)
                .finish(),
            ServerMsg::Marque {
                id,
                marque,
                rejouee,
            } => f
                .debug_struct("Marque")
                .field("id", id)
                .field("marque", marque)
                .field("rejouee", rejouee)
                .finish(),
            // Le texte copié peut être un secret : jamais montré par `Debug`.
            ServerMsg::PressePapiers { id, texte } => f
                .debug_struct("PressePapiers")
                .field("id", id)
                .field("caracteres", &texte.chars().count())
                .finish(),
            ServerMsg::Notification { id, titre, corps } => f
                .debug_struct("Notification")
                .field("id", id)
                .field("titre", titre)
                .field("corps", corps)
                .finish(),
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
            ServerMsg::Marque { .. } => "Marque",
            ServerMsg::ReplayMarque { .. } => "ReplayMarque",
            ServerMsg::PressePapiers { .. } => "PressePapiers",
            ServerMsg::Notification { .. } => "Notification",
        }
    }

    /// Vrai pour les variantes introduites par la version 4, qu'un client en version 1 à 3
    /// ne sait pas décoder.
    pub fn est_v4(&self) -> bool {
        matches!(
            self,
            ServerMsg::PressePapiers { .. } | ServerMsg::Notification { .. }
        )
    }

    /// Vrai pour les variantes introduites par la version 3, qu'un client en version 1 ou 2
    /// ne sait pas décoder.
    pub fn est_v3(&self) -> bool {
        matches!(
            self,
            ServerMsg::Marque { .. } | ServerMsg::ReplayMarque { .. }
        )
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

/// Découpe un rejeu `ReplayMarque` en segments : chaque élément est la marque (sauf pour le
/// premier segment, avant toute marque) suivie des octets qui la suivent, jusqu'à la marque
/// suivante. Sûr face à des décalages invalides : un décalage hors de `data` est ramené à
/// sa fin, un décalage qui recule est ramené au précédent ; jamais de panique.
pub fn segments_de_rejeu<'a>(
    data: &'a [u8],
    marques: &[(u32, MarqueCommande)],
) -> Vec<(Option<MarqueCommande>, &'a [u8])> {
    let mut coupes = Vec::with_capacity(marques.len() + 1);
    let mut precedent = 0usize;
    for &(d, _) in marques {
        let d = usize::try_from(d).unwrap_or(usize::MAX).min(data.len());
        precedent = precedent.max(d);
        coupes.push(precedent);
    }
    coupes.push(data.len());
    let mut segments = Vec::with_capacity(coupes.len());
    let mut debut = 0usize;
    for (i, &fin) in coupes.iter().enumerate() {
        let marque = i
            .checked_sub(1)
            .and_then(|k| marques.get(k))
            .map(|&(_, m)| m);
        segments.push((marque, data.get(debut..fin).unwrap_or_default()));
        debut = fin;
    }
    segments
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
            reponse: Some("yes".to_string().into()),
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
            reponse: Some("x".to_string().into()),
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

    /// Octets de la version 2, figés avant l'ajout de la version 3.
    #[test]
    fn encodage_v2_inchange() {
        let c = |m: &ClientMsg| postcard::to_stdvec(m).unwrap();
        let s = |m: &ServerMsg| postcard::to_stdvec(m).unwrap();
        assert_eq!(c(&ClientMsg::ChargerCle { cle: "a".into() }), [9, 1, 97]);
        assert_eq!(
            c(&ClientMsg::Demander {
                session: Some(2),
                invite: "x".into(),
                genre: Genre::Texte { masque: true },
                trousseau: EtatTrousseau::Absent,
            }),
            [7, 1, 2, 1, 120, 4, 1, 2]
        );
        assert_eq!(
            c(&ClientMsg::Repondre {
                demande: 1,
                reponse: Some("y".to_string().into()),
                memoriser: true,
            }),
            [8, 1, 1, 1, 121, 1]
        );
        assert_eq!(s(&ServerMsg::DemandeClose { demande: 5 }), [10, 5]);
        assert_eq!(s(&ServerMsg::Message("x".into())), [12, 1, 120]);
        assert_eq!(
            s(&ServerMsg::Demande {
                demande: 1,
                session: None,
                invite: String::new(),
                genre: Genre::HoteInconnu,
                trousseau: EtatTrousseau::Verrouille,
            }),
            [9, 1, 0, 0, 1, 1]
        );
    }

    #[test]
    fn nouvelles_variantes_v3_aller_retour() {
        fn ar<T: Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug>(m: T) {
            let o = postcard::to_stdvec(&m).unwrap();
            assert_eq!(postcard::from_bytes::<T>(&o).unwrap(), m);
        }
        ar(ClientMsg::Ouvrir {
            argv: vec!["sh".into()],
            cwd: Some(PathBuf::from("/tmp")),
            cols: 80,
            rows: 24,
            env: vec![("A".into(), "1".into()), ("B".into(), String::new())],
        });
        ar(ClientMsg::Ouvrir {
            argv: vec![],
            cwd: None,
            cols: 1,
            rows: 1,
            env: vec![],
        });
        for marque in [
            MarqueCommande::Invite,
            MarqueCommande::Saisie,
            MarqueCommande::Execution,
            MarqueCommande::Fin {
                code: Some(-1),
                duree_ms: Some(1500),
            },
            MarqueCommande::Fin {
                code: None,
                duree_ms: None,
            },
        ] {
            ar(ServerMsg::Marque {
                id: 7,
                marque,
                rejouee: true,
            });
        }
        ar(ServerMsg::ReplayMarque {
            id: 3,
            data: b"abc".to_vec(),
            marques: vec![(0, MarqueCommande::Invite), (3, MarqueCommande::Saisie)],
        });
        ar(ClientMsg::Demander {
            session: None,
            invite: String::new(),
            genre: Genre::HoteInconnu,
            trousseau: EtatTrousseau::Lent,
        });
        // Indices : les nouvelles variantes suivent les anciennes.
        let ouvrir = ClientMsg::Ouvrir {
            argv: vec![],
            cwd: None,
            cols: 0,
            rows: 0,
            env: vec![],
        };
        assert_eq!(postcard::to_stdvec(&ouvrir).unwrap()[0], 10);
        let marque = ServerMsg::Marque {
            id: 0,
            marque: MarqueCommande::Invite,
            rejouee: false,
        };
        assert_eq!(postcard::to_stdvec(&marque).unwrap()[0], 13);
        assert!(marque.est_v3() && !marque.est_v2());
        assert_eq!(marque.nom(), "Marque");
        let rejeu = ServerMsg::ReplayMarque {
            id: 0,
            data: vec![],
            marques: vec![],
        };
        assert_eq!(postcard::to_stdvec(&rejeu).unwrap()[0], 14);
        assert!(rejeu.est_v3() && !rejeu.est_v2());
        assert_eq!(rejeu.nom(), "ReplayMarque");
        let copie = ServerMsg::PressePapiers {
            id: 1,
            texte: "secret".into(),
        };
        assert_eq!(postcard::to_stdvec(&copie).unwrap()[0], 15);
        assert!(copie.est_v4() && !copie.est_v3());
        assert!(!format!("{copie:?}").contains("secret"));
        let notif = ServerMsg::Notification {
            id: 1,
            titre: Some("t".into()),
            corps: "c".into(),
        };
        assert_eq!(postcard::to_stdvec(&notif).unwrap()[0], 16);
        assert!(notif.est_v4() && !notif.est_v3());
        ar(copie);
        ar(notif);
        assert_eq!(postcard::to_stdvec(&EtatTrousseau::Lent).unwrap(), [3]);
        // Les valeurs v2 de l'état ne bougent pas.
        assert_eq!(postcard::to_stdvec(&EtatTrousseau::Absent).unwrap(), [2]);
    }

    #[test]
    fn segments_de_rejeu_surs() {
        use MarqueCommande::{Invite, Saisie};
        let s = segments_de_rejeu(b"abcdef", &[(2, Invite), (4, Saisie)]);
        assert_eq!(
            s,
            vec![
                (None, &b"ab"[..]),
                (Some(Invite), &b"cd"[..]),
                (Some(Saisie), &b"ef"[..])
            ]
        );
        // Décalages hors bornes ou qui reculent : bornés, sans panique.
        let s = segments_de_rejeu(b"abc", &[(9, Invite), (1, Saisie), (u32::MAX, Invite)]);
        assert_eq!(s.len(), 4);
        assert_eq!(s.iter().map(|(_, o)| o.len()).sum::<usize>(), 3);
        assert_eq!(segments_de_rejeu(b"", &[]), vec![(None, &b""[..])]);
    }

    #[test]
    fn debug_ouvrir_ne_montre_pas_les_valeurs() {
        let m = ClientMsg::Ouvrir {
            argv: vec![],
            cwd: None,
            cols: 1,
            rows: 1,
            env: vec![("JETON".into(), "VALEUR-SECRETE".into())],
        };
        let t = format!("{m:?}");
        assert!(t.contains("JETON") && !t.contains("VALEUR-SECRETE"), "{t}");
    }

    #[test]
    fn debug_ne_montre_pas_la_phrase() {
        let r = ClientMsg::Repondre {
            demande: 1,
            reponse: Some("PHRASE-SECRETE".to_string().into()),
            memoriser: false,
        };
        let a = ServerMsg::Reponse {
            reponse: Some("PHRASE-SECRETE".to_string().into()),
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
