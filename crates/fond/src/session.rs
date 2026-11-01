// SPDX-License-Identifier: GPL-3.0-or-later
//! Session : un shell dans un pseudo-terminal, possédé par le processus de fond.
//! Le tampon de relecture est tenu par le serveur, qui diffuse la sortie (voir `serveur`).

use nix::sys::signal::{killpg, Signal};
use nix::unistd::{Pid, User};
use portable_pty::{
    native_pty_system, ChildKiller, CommandBuilder, ExitStatus, MasterPty, PtySize,
};
use pupitre_i18n::{ta, ArgI18n};
use std::ffi::OsString;
use std::fmt;
use std::io::{self, Read, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

/// Après la fin du shell, temps laissé à la lecture pour recevoir les derniers octets
/// (si un processus d'arrière-plan garde le terminal ouvert, on n'attend pas plus).
const ATTENTE_FIN_LECTURE: Duration = Duration::from_millis(500);
/// Taille de la file d'événements d'une session (morceaux de 64 Kio au plus).
const FILE_EVENEMENTS: usize = 64;
/// Délai entre SIGHUP et SIGKILL quand une session est fermée.
const DELAI_AVANT_SIGKILL: Duration = Duration::from_secs(2);
/// Variables héritées qui décriraient mal le terminal de Pupitre, ou (mode de
/// `pupitre-askpass` réservé au chargement des clés) changeraient le comportement de
/// l'askpass des shells : réponse depuis le trousseau, sans fenêtre.
const VARIABLES_RETIREES: [&str; 6] = [
    "COLUMNS",
    "LINES",
    "VTE_VERSION",
    "TERM_PROGRAM_VERSION",
    "TERMINAL_EMULATOR",
    "PUPITRE_ASKPASS_MODE",
];

#[derive(Debug, Clone, PartialEq)]
pub enum SessionEvent {
    Output(Vec<u8>),
    /// Fin du shell ; `None` si tué par un signal ou si la session a été fermée sur incident.
    Exited(Option<i32>),
}

#[derive(Debug)]
pub enum OpenError {
    /// Pseudo-terminal impossible à créer.
    Pty(String),
    /// Shell impossible à lancer (introuvable, non exécutable…).
    Spawn(String),
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenError::Pty(e) => f.write_str(&ta(
                "session-terminal-impossible",
                &[("erreur", ArgI18n::from(e.as_str()))],
            )),
            OpenError::Spawn(e) => f.write_str(&ta(
                "session-shell-impossible",
                &[("erreur", ArgI18n::from(e.as_str()))],
            )),
        }
    }
}

impl std::error::Error for OpenError {}

/// Un shell dans un PTY. Deux threads par session : lecture du PTY, attente de la fin
/// du shell. Une panique dans l'un d'eux ferme cette session seulement.
/// Fermer (`kill` ou abandon de la `Session`) termine le shell et ses processus.
pub struct Session {
    maitre: Mutex<Box<dyn MasterPty + Send>>,
    ecrivain: Mutex<Box<dyn Write + Send>>,
    tueur: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    /// Le shell est chef de sa session et de son groupe (setsid) : pid = pgid.
    pid: Option<Pid>,
    termine: Arc<AtomicBool>,
}

/// Un verrou empoisonné (panique d'un autre thread) reste utilisable : les données
/// protégées ici ne peuvent pas être laissées dans un état incohérent.
fn verrou<T: ?Sized>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Session {
    /// Lance `argv` (vide = shell de connexion) dans un nouveau PTY.
    /// `cwd` absent ou inexistant = dossier personnel.
    pub fn open(
        argv: &[String],
        cwd: Option<&Path>,
        cols: u16,
        rows: u16,
        env: &[(String, String)],
    ) -> Result<(Session, Receiver<SessionEvent>), OpenError> {
        let argv = if argv.is_empty() {
            login_shell()
        } else {
            argv.to_vec()
        };
        let taille = PtySize {
            rows: rows.max(1),
            cols: cols.max(1),
            pixel_width: 0,
            pixel_height: 0,
        };
        let paire = native_pty_system()
            .openpty(taille)
            .map_err(|e| OpenError::Pty(e.to_string()))?;

        let mut cmd = CommandBuilder::from_argv(argv.iter().map(OsString::from).collect());
        if let Some(d) = cwd
            .filter(|d| d.is_dir())
            .map(Path::to_path_buf)
            .or_else(std::env::home_dir)
        {
            cmd.cwd(d);
        }
        for v in VARIABLES_RETIREES {
            cmd.env_remove(v);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("TERM_PROGRAM", "Pupitre");
        for (k, v) in env {
            cmd.env(k, v);
        }
        let nom = argv.first().cloned().unwrap_or_default();
        let mut enfant = paire
            .slave
            .spawn_command(cmd)
            .map_err(|e| OpenError::Spawn(format!("{nom} : {e}")))?;
        drop(paire.slave);
        let mut tueur = enfant.clone_killer();
        let abandon = |e: String, tueur: &mut Box<dyn ChildKiller + Send + Sync>| {
            let _ = tueur.kill();
            OpenError::Pty(e)
        };
        let lecteur = paire
            .master
            .try_clone_reader()
            .map_err(|e| abandon(e.to_string(), &mut tueur))?;
        let ecrivain = paire
            .master
            .take_writer()
            .map_err(|e| abandon(e.to_string(), &mut tueur))?;

        let pid = enfant
            .process_id()
            .and_then(|p| i32::try_from(p).ok())
            .map(Pid::from_raw);
        let termine = Arc::new(AtomicBool::new(false));
        let fin_envoyee = Arc::new(AtomicBool::new(false));
        // File bornée : si personne ne suit, la lecture ralentit le programme (contre-pression
        // du PTY) au lieu de remplir la mémoire.
        let (tx, rx) = mpsc::sync_channel(FILE_EVENEMENTS);
        let (fin_lecture_tx, fin_lecture_rx) = mpsc::channel::<()>();

        // Thread de lecture : PTY → événements.
        {
            let (tx, fin_envoyee) = (tx.clone(), fin_envoyee.clone());
            let mut tueur_lecture = enfant.clone_killer();
            thread::Builder::new()
                .name("pupitre-lecture".into())
                .spawn(move || {
                    let ok =
                        catch_unwind(AssertUnwindSafe(|| boucle_lecture(lecteur, &tx))).is_ok();
                    if !ok {
                        let _ = tueur_lecture.kill();
                        envoyer_fin(&tx, &fin_envoyee, None);
                    }
                    let _ = fin_lecture_tx.send(());
                })
                .map_err(|e| abandon(e.to_string(), &mut tueur))?;
        }
        // Thread d'attente : fin du shell → Exited, après les derniers octets.
        {
            let (termine, fin_envoyee) = (termine.clone(), fin_envoyee.clone());
            let attente = move || {
                let code = catch_unwind(AssertUnwindSafe(|| {
                    let statut = enfant.wait();
                    termine.store(true, Ordering::SeqCst);
                    let _ = fin_lecture_rx.recv_timeout(ATTENTE_FIN_LECTURE);
                    statut.ok().and_then(|s| code_de_sortie(&s))
                }));
                termine.store(true, Ordering::SeqCst);
                envoyer_fin(&tx, &fin_envoyee, code.unwrap_or(None));
            };
            thread::Builder::new()
                .name("pupitre-attente".into())
                .spawn(attente)
                .map_err(|e| abandon(e.to_string(), &mut tueur))?;
        }

        let session = Session {
            maitre: Mutex::new(paire.master),
            ecrivain: Mutex::new(ecrivain),
            tueur: Mutex::new(tueur),
            pid,
            termine,
        };
        Ok((session, rx))
    }

    /// Envoie des octets au shell (frappes, collage, réponses du terminal).
    pub fn write_input(&self, data: &[u8]) -> io::Result<()> {
        let mut w = verrou(&self.ecrivain);
        w.write_all(data)?;
        w.flush()
    }

    pub fn resize(&self, cols: u16, rows: u16) -> io::Result<()> {
        let taille = PtySize {
            rows: rows.max(1),
            cols: cols.max(1),
            pixel_width: 0,
            pixel_height: 0,
        };
        verrou(&self.maitre)
            .resize(taille)
            .map_err(|e| io::Error::other(e.to_string()))
    }

    /// Vrai si un programme autre que le shell est au premier plan du terminal.
    pub fn is_busy(&self) -> bool {
        if self.termine.load(Ordering::SeqCst) {
            return false;
        }
        match (verrou(&self.maitre).process_group_leader(), self.pid) {
            (Some(groupe), Some(pid)) => groupe > 0 && groupe != pid.as_raw(),
            _ => false,
        }
    }

    /// Termine le shell et les processus de son terminal : SIGHUP (comme à la fermeture d'un
    /// terminal), puis SIGKILL après 2 s pour ceux qui l'ignorent. Ne bloque pas.
    pub fn kill(&self) {
        let premier_plan = verrou(&self.maitre)
            .process_group_leader()
            .filter(|&g| g > 0)
            .map(Pid::from_raw);
        let groupes: Vec<Pid> = self.pid.into_iter().chain(premier_plan).collect();
        if !self.termine.load(Ordering::SeqCst) {
            let _ = verrou(&self.tueur).kill();
        }
        for &g in &groupes {
            // Le pgid reste réservé tant que le groupe existe : pas de risque de viser un
            // processus étranger ; s'il n'existe plus, ESRCH est ignoré.
            let _ = killpg(g, Signal::SIGHUP);
        }
        let _ = thread::Builder::new()
            .name("pupitre-sigkill".into())
            .spawn(move || {
                thread::sleep(DELAI_AVANT_SIGKILL);
                for g in groupes {
                    let _ = killpg(g, Signal::SIGKILL);
                }
            });
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.kill();
    }
}

fn boucle_lecture(mut lecteur: Box<dyn Read + Send>, tx: &SyncSender<SessionEvent>) {
    let mut tampon = vec![0u8; 64 * 1024];
    loop {
        match lecteur.read(&mut tampon) {
            Ok(0) => break,
            Ok(n) => {
                let donnees = tampon[..n].to_vec();
                if tx.send(SessionEvent::Output(donnees)).is_err() {
                    break; // Plus personne n'écoute cette session.
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => break, // EIO : plus aucun processus n'a le terminal ouvert.
        }
    }
}

/// Envoie `Exited` une seule fois, quel que soit le thread qui constate la fin.
fn envoyer_fin(tx: &SyncSender<SessionEvent>, deja: &AtomicBool, code: Option<i32>) {
    if !deja.swap(true, Ordering::SeqCst) {
        let _ = tx.send(SessionEvent::Exited(code));
    }
}

fn code_de_sortie(s: &ExitStatus) -> Option<i32> {
    match s.signal() {
        Some(_) => None,
        None => i32::try_from(s.exit_code()).ok(),
    }
}

/// Shell de connexion de l'utilisateur (base des comptes), sinon `/bin/sh`.
pub fn login_shell() -> Vec<String> {
    let shell = User::from_uid(nix::unistd::getuid())
        .ok()
        .flatten()
        .map(|u| u.shell)
        .filter(|s| s.is_absolute() && s.is_file())
        .unwrap_or_else(|| PathBuf::from("/bin/sh"));
    vec![shell.to_string_lossy().into_owned()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn argv(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    fn ouvrir(a: &[&str]) -> (Session, Receiver<SessionEvent>) {
        Session::open(&argv(a), None, 80, 24, &[]).unwrap()
    }

    /// Lit les événements jusqu'à ce que la sortie cumulée contienne `motif`.
    /// Renvoie la sortie et le code de sortie s'il est arrivé entre-temps.
    fn attendre(rx: &Receiver<SessionEvent>, motif: &str) -> String {
        let limite = Instant::now() + Duration::from_secs(10);
        let mut sortie = Vec::new();
        while Instant::now() < limite {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(SessionEvent::Output(d)) => sortie.extend(d),
                Ok(SessionEvent::Exited(_)) => break,
                Err(_) => {}
            }
            if String::from_utf8_lossy(&sortie).contains(motif) {
                break;
            }
        }
        let s = String::from_utf8_lossy(&sortie).into_owned();
        assert!(s.contains(motif), "{motif:?} absent de {s:?}");
        s
    }

    fn attendre_fin(rx: &Receiver<SessionEvent>) -> Option<i32> {
        let limite = Instant::now() + Duration::from_secs(10);
        while Instant::now() < limite {
            if let Ok(SessionEvent::Exited(c)) = rx.recv_timeout(Duration::from_millis(100)) {
                return c;
            }
        }
        panic!("la session ne s'est pas terminée");
    }

    #[test]
    fn sortie_du_shell_recue() {
        let (_s, rx) = ouvrir(&["/bin/sh", "-c", "printf bonjour"]);
        attendre(&rx, "bonjour");
        assert_eq!(attendre_fin(&rx), Some(0));
    }

    #[test]
    fn code_de_sortie_transmis() {
        let (_s, rx) = ouvrir(&["/bin/sh", "-c", "exit 3"]);
        assert_eq!(attendre_fin(&rx), Some(3));
    }

    #[test]
    fn saisie_transmise() {
        let (s, rx) = ouvrir(&["/bin/sh"]);
        s.write_input(b"echo $((6*7))\n").unwrap();
        attendre(&rx, "42");
    }

    #[test]
    fn redimensionnement_visible() {
        let (s, rx) = ouvrir(&["/bin/sh"]);
        s.resize(100, 30).unwrap();
        s.write_input(b"stty size\n").unwrap();
        attendre(&rx, "30 100");
    }

    #[test]
    fn variables_d_environnement() {
        let (s, rx) = Session::open(
            &argv(&["/bin/sh"]),
            None,
            80,
            24,
            &[("PUPITRE_ESSAI".into(), "oui".into())],
        )
        .unwrap();
        s.write_input(b"echo $TERM $COLORTERM $TERM_PROGRAM $PUPITRE_ESSAI\n")
            .unwrap();
        attendre(&rx, "xterm-256color truecolor Pupitre oui");
    }

    #[test]
    fn repertoire_de_depart() {
        let d = std::env::temp_dir();
        let (s, rx) = Session::open(&argv(&["/bin/sh"]), Some(&d), 80, 24, &[]).unwrap();
        s.write_input(b"pwd\n").unwrap();
        attendre(&rx, &d.canonicalize().unwrap().display().to_string());
    }

    #[test]
    fn repertoire_absent_ne_bloque_pas() {
        let (_s, rx) = Session::open(
            &argv(&["/bin/sh", "-c", "printf ok"]),
            Some(Path::new("/nexiste/pas")),
            80,
            24,
            &[],
        )
        .unwrap();
        attendre(&rx, "ok");
    }

    #[test]
    fn occupe_pendant_une_commande() {
        let (s, rx) = ouvrir(&["/bin/sh"]);
        // fi''ni : l'écho de la ligne tapée ne contient pas « fini », seule la sortie le contient.
        s.write_input(b"sleep 2; echo fi''ni\n").unwrap();
        std::thread::sleep(Duration::from_millis(300));
        assert!(s.is_busy());
        attendre(&rx, "fini");
        std::thread::sleep(Duration::from_millis(300));
        assert!(!s.is_busy());
    }

    #[test]
    fn ouvrir_shell_inexistant_renvoie_erreur() {
        let debut = Instant::now();
        match Session::open(&argv(&["/nexiste/pas"]), None, 80, 24, &[]) {
            Err(OpenError::Spawn(_)) => {}
            Ok((_s, rx)) => assert!(matches!(attendre_fin(&rx), Some(c) if c != 0)),
            Err(e) => panic!("erreur inattendue : {e:?}"),
        }
        assert!(debut.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn argv_vide_lance_le_shell_de_connexion() {
        let (s, rx) = Session::open(&[], None, 80, 24, &[]).unwrap();
        s.write_input(b"echo $((2+3))z\n").unwrap();
        attendre(&rx, "5z");
    }

    #[test]
    fn kill_termine_la_session() {
        let (s, rx) = ouvrir(&["/bin/sh"]);
        s.kill();
        attendre_fin(&rx);
        // Écrire dans une session terminée renvoie une erreur ou rien, jamais une panique.
        let _ = s.write_input(b"x");
        let _ = s.resize(10, 10);
        assert!(!s.is_busy());
    }

    #[test]
    fn fin_meme_si_un_processus_garde_le_terminal() {
        // Le shell se termine mais laisse une tâche de fond attachée au PTY.
        let (_s, rx) = ouvrir(&["/bin/sh", "-c", "sleep 30 & exit 0"]);
        let debut = Instant::now();
        assert_eq!(attendre_fin(&rx), Some(0));
        assert!(debut.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn shell_de_connexion_existe() {
        let sh = login_shell();
        assert_eq!(sh.len(), 1);
        assert!(Path::new(&sh[0]).is_absolute());
    }
}
