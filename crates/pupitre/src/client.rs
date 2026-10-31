// SPDX-License-Identifier: GPL-3.0-or-later
//! Connexion de l'interface au processus de fond : démarrage automatique, envoi sans
//! bloquer GTK, réception sur un canal `async_channel` lu par la boucle GTK.

use pupitre_fond::serveur::{preparer_dossier, socket_path};
use pupitre_proto::{read_frame, write_frame, ClientMsg, FrameError, ServerMsg, VERSION};
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

/// Temps laissé au processus de fond pour démarrer et répondre.
const DELAI_DEMARRAGE: Duration = Duration::from_secs(2);
const PAS: Duration = Duration::from_millis(25);
/// Message placé sur le canal quand la connexion est perdue.
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
            ClientError::Lancement(e) => write!(f, "impossible de lancer pupitre-fond : {e}"),
            ClientError::Injoignable(e) => write!(f, "pupitre-fond ne répond pas : {e}"),
            ClientError::Refus(e) => write!(f, "pupitre-fond refuse la connexion : {e}"),
            ClientError::Io(e) => write!(f, "erreur de connexion à pupitre-fond : {e}"),
            ClientError::Deconnecte => f.write_str("connexion à pupitre-fond fermée"),
        }
    }
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
}

impl Client {
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
    demarrer(flux)
}

/// Lance `pupitre-fond` détaché : son propre groupe de processus (la fermeture du terminal
/// d'où Pupitre a été lancé ne le tue pas), entrées-sorties fermées. Un thread récupère sa
/// fin pour qu'il ne reste pas de processus zombie.
fn lancer_fond(socket: &Path, binaire: PathBuf) -> Result<(), ClientError> {
    let mut enfant = Command::new(&binaire)
        .arg("--socket")
        .arg(socket)
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

/// Poignée de main, puis threads d'écriture et de lecture.
fn demarrer(flux: UnixStream) -> Result<(Client, async_channel::Receiver<ServerMsg>), ClientError> {
    let mut ecriture = flux.try_clone()?;
    let mut lecture = flux.try_clone()?;
    write_frame(&mut ecriture, &ClientMsg::Hello { version: VERSION })?;
    lecture.set_read_timeout(Some(DELAI_DEMARRAGE))?;
    let accueil = match read_frame::<_, ServerMsg>(&mut lecture) {
        Ok(m @ ServerMsg::Welcome { .. }) => m,
        Ok(ServerMsg::Error(e)) => return Err(ClientError::Refus(e)),
        Ok(autre) => {
            return Err(ClientError::Refus(format!(
                "réponse inattendue : {autre:?}"
            )))
        }
        Err(FrameError::Io(e)) => return Err(ClientError::Injoignable(e.to_string())),
        Err(e) => return Err(ClientError::Refus(e.to_string())),
    };
    lecture.set_read_timeout(None)?;

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
        },
        recu_rx,
    ))
}
