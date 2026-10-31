// SPDX-License-Identifier: GPL-3.0-or-later
//! Messages échangés entre l'interface (`pupitre`) et le processus de fond (`pupitre-fond`).
//!
//! Chaque message voyage dans une trame : longueur `u32` petit-boutiste, puis la charge
//! encodée en postcard (voir [`trame`]).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub mod trame;
pub use trame::{read_frame, write_frame, FrameError, MAX_FRAME};

/// Version du protocole ; échangée dans `Hello`/`Welcome`.
pub const VERSION: u32 = 1;

/// Identifiant d'une session (un shell dans un PTY), unique dans un processus de fond.
pub type SessionId = u64;

/// Messages envoyés par l'interface au processus de fond.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
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
}

/// Messages envoyés par le processus de fond à l'interface.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum ServerMsg {
    Welcome { version: u32 },
    Opened { id: SessionId },
    OpenFailed { error: String },
    Output { id: SessionId, data: Vec<u8> },
    Replay { id: SessionId, data: Vec<u8> },
    Exited { id: SessionId, code: Option<i32> },
    Sessions(Vec<SessionInfo>),
    Busy { id: SessionId, busy: bool },
    Error(String),
}

/// Description d'une session, renvoyée par `List`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SessionInfo {
    pub id: SessionId,
    pub cols: u16,
    pub rows: u16,
    pub busy: bool,
}
