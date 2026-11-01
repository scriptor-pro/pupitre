// SPDX-License-Identifier: GPL-3.0-or-later
//! Connexion de l'interface au processus de fond : démarrage automatique, envoi sans
//! bloquer GTK, réception sur un canal `async_channel` lu par la boucle GTK.

use pupitre_fond::serveur::{preparer_dossier, socket_path};
use pupitre_i18n::{langue_active, t, ta, ArgI18n};
use pupitre_proto::{
    read_frame, write_frame, ClientMsg, FrameError, ServerMsg, VERSION, VERSION_MIN,
};
use std::fmt;
use std::io;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// Temps laissé au processus de fond pour démarrer (création du socket).
const DELAI_DEMARRAGE: Duration = Duration::from_secs(2);
/// Temps laissé au processus de fond pour répondre au `Hello`. Il écoute avant d'avoir fini
/// de démarrer : sa recherche d'agent SSH (jusqu'à 1 s + 0,3 s, puis 2 s pour lancer le
/// sien) passe avant l'accueil, et plus encore sur une machine chargée.
const DELAI_ACCUEIL: Duration = Duration::from_secs(5);
const PAS: Duration = Duration::from_millis(25);
/// Message placé sur le canal quand la connexion est perdue (repère interne : l'interface le
/// reconnaît et affiche son propre texte traduit, jamais celui-ci).
pub const CONNEXION_PERDUE: &str = "connexion perdue";

#[derive(Debug)]
pub enum ClientError {
    /// Le processus de fond n'a pas pu être lancé.
    Lancement(String),
    /// Le processus de fond ne répond pas dans le délai.
    Injoignable(String),
    /// Le processus de fond a refusé la connexion (version différente…).
    Refus(String),
    Io(io::Error),
    /// La connexion est fermée ; il faut se reconnecter.
    Deconnecte,
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClientError::Lancement(e) => f.write_str(&erreur("client-lancement", e)),
            ClientError::Injoignable(e) => f.write_str(&erreur("client-injoignable", e)),
            ClientError::Refus(e) => f.write_str(&erreur("client-refus", e)),
            ClientError::Io(e) => f.write_str(&erreur("client-io", &e.to_string())),
            ClientError::Deconnecte => f.write_str(&t("client-deconnecte")),
        }
    }
}

fn erreur(id: &str, e: &str) -> String {
    ta(id, &[("erreur", ArgI18n::from(e))])
}

impl std::error::Error for ClientError {}

impl From<io::Error> for ClientError {
    fn from(e: io::Error) -> Self {
        ClientError::Io(e)
    }
}

/// Connexion établie. Les messages reçus arrivent sur le canal rendu par `connect_or_spawn`
/// (le premier est `Welcome`) ; la perte de connexion y est signalée par
/// `ServerMsg::Error("connexion perdue")`.
pub struct Client {
    envoi: mpsc::Sender<ClientMsg>,
    flux: UnixStream,
    /// Version du protocole négociée avec le processus de fond.
    version: u32,
}

impl Client {
    /// Version du protocole négociée (1 si le processus de fond est ancien).
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Met le message en file ; ne bloque jamais (un thread se charge de l'écriture).
    pub fn send(&self, msg: ClientMsg) -> Result<(), ClientError> {
        self.envoi.send(msg).map_err(|_| ClientError::Deconnecte)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.flux.shutdown(Shutdown::Both);
    }
}

/// Se connecte au processus de fond de l'utilisateur, en le lançant s'il ne tourne pas.
pub fn connect_or_spawn() -> Result<(Client, async_channel::Receiver<ServerMsg>), ClientError> {
    connect_or_spawn_at(&socket_path())
}

/// Comme [`connect_or_spawn`], sur un socket donné.
pub fn connect_or_spawn_at(
    socket: &Path,
) -> Result<(Client, async_channel::Receiver<ServerMsg>), ClientError> {
    connect_or_spawn_with(socket, None)
}

/// Comme [`connect_or_spawn_at`], avec un chemin explicite du binaire `pupitre-fond`
/// (`None` : variable `PUPITRE_FOND`, puis dossier de l'exécutable courant, puis `PATH`).
pub fn connect_or_spawn_with(
    socket: &Path,
    binaire: Option<PathBuf>,
) -> Result<(Client, async_channel::Receiver<ServerMsg>), ClientError> {
    let flux = match UnixStream::connect(socket) {
        Ok(f) => f,
        Err(_) => {
            preparer_dossier(socket)?;
            lancer_fond(socket, binaire.unwrap_or_else(binaire_fond))?;
            attendre_socket(socket)?
        }
    };
    match demarrer(flux, VERSION) {
        // Processus de fond plus ancien (qui tourne encore avec des onglets) : il ferme la
        // connexion refusée, on en ouvre une autre en proposant la version 1. Le texte du refus
        // n'est pas examiné : il dépend de la langue du processus de fond, qui peut différer
        // de la nôtre (ou être celui d'une version sans traduction).
        Err(ClientError::Refus(_)) if VERSION > VERSION_MIN => {
            demarrer(UnixStream::connect(socket)?, VERSION_MIN)
        }
        autre => autre,
    }
}

/// Lance `pupitre-fond` détaché : son propre groupe de processus (la fermeture du terminal
/// d'où Pupitre a été lancé ne le tue pas), entrées-sorties fermées. Un thread récupère sa
/// fin pour qu'il ne reste pas de processus zombie.
fn lancer_fond(socket: &Path, binaire: PathBuf) -> Result<(), ClientError> {
    let mut enfant = Command::new(&binaire)
        .arg("--socket")
        .arg(socket)
        .arg("--langue")
        .arg(langue_active().language.as_str())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|e| ClientError::Lancement(format!("{} : {e}", binaire.display())))?;
    let _ = thread::Builder::new()
        .name("pupitre-fond-attente".into())
        .spawn(move || enfant.wait());
    Ok(())
}

fn binaire_fond() -> PathBuf {
    if let Some(p) = std::env::var_os("PUPITRE_FOND").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join("pupitre-fond")))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from("pupitre-fond"))
}

fn attendre_socket(socket: &Path) -> Result<UnixStream, ClientError> {
    let limite = Instant::now() + DELAI_DEMARRAGE;
    loop {
        match UnixStream::connect(socket) {
            Ok(f) => return Ok(f),
            Err(e) if Instant::now() >= limite => {
                return Err(ClientError::Injoignable(format!(
                    "{} : {e}",
                    socket.display()
                )))
            }
            Err(_) => thread::sleep(PAS),
        }
    }
}

/// Poignée de main en proposant `version`, puis threads d'écriture et de lecture.
fn demarrer(
    flux: UnixStream,
    version: u32,
) -> Result<(Client, async_channel::Receiver<ServerMsg>), ClientError> {
    let mut ecriture = flux.try_clone()?;
    let mut lecture = flux.try_clone()?;
    write_frame(&mut ecriture, &ClientMsg::Hello { version })?;
    lecture.set_read_timeout(Some(DELAI_ACCUEIL))?;
    let accueil = match read_frame::<_, ServerMsg>(&mut lecture) {
        Ok(m @ ServerMsg::Welcome { .. }) => m,
        Ok(ServerMsg::Error(e)) => return Err(ClientError::Refus(e)),
        Ok(autre) => {
            return Err(ClientError::Refus(ta(
                "client-reponse-inattendue",
                &[("nom", ArgI18n::from(autre.nom()))],
            )))
        }
        Err(FrameError::Io(e)) => return Err(ClientError::Injoignable(e.to_string())),
        Err(e) => return Err(ClientError::Refus(e.to_string())),
    };
    lecture.set_read_timeout(None)?;
    let negociee = match accueil {
        ServerMsg::Welcome { version: v } => v.min(version),
        _ => version,
    };

    let (recu_tx, recu_rx) = async_channel::unbounded();
    let _ = recu_tx.send_blocking(accueil);
    let (envoi_tx, envoi_rx) = mpsc::channel::<ClientMsg>();

    thread::Builder::new()
        .name("pupitre-client-envoi".into())
        .spawn(move || {
            for m in envoi_rx {
                if write_frame(&mut ecriture, &m).is_err() {
                    break;
                }
            }
            // Client abandonné ou connexion perdue : la lecture se termine aussi.
            let _ = ecriture.shutdown(Shutdown::Both);
        })?;
    thread::Builder::new()
        .name("pupitre-client-reception".into())
        .spawn(move || {
            while let Ok(m) = read_frame::<_, ServerMsg>(&mut lecture) {
                if recu_tx.send_blocking(m).is_err() {
                    break; // Plus personne n'écoute.
                }
            }
            let _ = lecture.shutdown(Shutdown::Both);
            let _ = recu_tx.send_blocking(ServerMsg::Error(CONNEXION_PERDUE.into()));
        })?;
    Ok((
        Client {
            envoi: envoi_tx,
            flux,
            version: negociee,
        },
        recu_rx,
    ))
}
