// SPDX-License-Identifier: GPL-3.0-or-later
//! Recherche d'un agent SSH joignable, ou lancement d'un agent propre à Pupitre.

use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Délai accordé à un agent pour répondre lors d'une sonde.
pub const DELAI_SONDE: Duration = Duration::from_millis(300);

/// Taille maximale acceptée pour une réponse d'agent (256 Kio).
const TAILLE_MAX: u32 = 256 * 1024;
/// `SSH_AGENTC_REQUEST_IDENTITIES`, avec son préfixe de longueur.
const REQUETE_IDENTITES: [u8; 5] = [0, 0, 0, 1, 11];
/// `SSH_AGENT_IDENTITIES_ANSWER`.
const REPONSE_IDENTITES: u8 = 12;

/// Vrai si un agent SSH répond à `socket` : on lui demande ses identités
/// et on vérifie que la réponse est du bon type. Connexion, écriture et
/// lecture sont bornées par `delai` ; le corps de la réponse n'est pas lu.
pub fn agent_joignable(socket: &Path, delai: Duration) -> bool {
    sonder(socket, delai).unwrap_or(false)
}

fn sonder(socket: &Path, delai: Duration) -> io::Result<bool> {
    let mut flux = UnixStream::connect(socket)?;
    flux.set_read_timeout(Some(delai))?;
    flux.set_write_timeout(Some(delai))?;
    flux.write_all(&REQUETE_IDENTITES)?;
    let mut longueur = [0u8; 4];
    flux.read_exact(&mut longueur)?;
    let longueur = u32::from_be_bytes(longueur);
    if longueur == 0 || longueur > TAILLE_MAX {
        return Ok(false);
    }
    let mut genre = [0u8; 1];
    flux.read_exact(&mut genre)?;
    Ok(genre[0] == REPONSE_IDENTITES)
}

/// Vrai si `chemin` est un socket (liens suivis) appartenant à l'utilisateur
/// courant.
fn socket_a_moi(chemin: &Path) -> bool {
    match fs::metadata(chemin) {
        Ok(m) => m.file_type().is_socket() && m.uid() == nix::unistd::getuid().as_raw(),
        Err(_) => false,
    }
}

/// Sockets d'agent plausibles, par ordre de préférence.
pub fn candidats_agent(runtime: &Path, tmp: &Path) -> Vec<PathBuf> {
    let mut liste: Vec<PathBuf> = [
        "openssh_agent",
        "ssh-agent.socket",
        "gcr/ssh",
        "keyring/ssh",
        "gnupg/S.gpg-agent.ssh",
    ]
    .iter()
    .map(|c| runtime.join(c))
    .collect();
    let motif = tmp.join("ssh-*").join("agent.*");
    if let Some(motif) = motif.to_str() {
        if let Ok(chemins) = glob::glob(motif) {
            let mut trouves: Vec<PathBuf> = chemins.flatten().collect();
            trouves.sort();
            liste.extend(trouves);
        }
    }
    liste.retain(|c| socket_a_moi(c));
    liste
}

/// Agent de l'environnement s'il répond, sinon premier candidat joignable.
pub fn chercher_agent(
    env_sock: Option<&Path>,
    runtime: &Path,
    tmp: &Path,
    delai: Duration,
) -> Option<PathBuf> {
    if let Some(s) = env_sock {
        if agent_joignable(s, delai) {
            return Some(s.to_path_buf());
        }
    }
    candidats_agent(runtime, tmp)
        .into_iter()
        .find(|c| agent_joignable(c, delai))
}

/// Agent `ssh-agent` lancé par Pupitre ; tué et nettoyé à la destruction.
pub struct AgentPupitre {
    socket: PathBuf,
    enfant: Child,
}

impl AgentPupitre {
    /// Lance `ssh-agent -D -a <socket>` avec les variables `env` (l'agent ne
    /// lit `SSH_ASKPASS` qu'au démarrage) et attend au plus 2 s qu'il réponde.
    pub fn lancer(socket: &Path, env: &[(String, String)]) -> io::Result<AgentPupitre> {
        // Supprime un socket périmé, mais jamais autre chose qu'un socket.
        if let Ok(m) = fs::symlink_metadata(socket) {
            if m.file_type().is_socket() {
                fs::remove_file(socket)?;
            }
        }
        let mut cmd = Command::new("ssh-agent");
        cmd.arg("-D")
            .arg("-a")
            .arg(socket)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        for (k, v) in env {
            cmd.env(k, v);
        }
        let mut enfant = cmd.spawn()?;
        let limite = Instant::now() + Duration::from_secs(2);
        loop {
            if agent_joignable(socket, DELAI_SONDE) {
                return Ok(AgentPupitre {
                    socket: socket.to_path_buf(),
                    enfant,
                });
            }
            let fini = matches!(enfant.try_wait(), Ok(Some(_)) | Err(_));
            if fini || Instant::now() >= limite {
                let _ = enfant.kill();
                let _ = enfant.wait();
                let _ = fs::remove_file(socket);
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "ssh-agent n'est pas devenu joignable",
                ));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Chemin du socket de l'agent.
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// Vrai tant que le processus `ssh-agent` tourne.
    pub fn vivant(&mut self) -> bool {
        matches!(self.enfant.try_wait(), Ok(None))
    }
}

impl Drop for AgentPupitre {
    fn drop(&mut self) {
        let _ = self.enfant.kill();
        let _ = self.enfant.wait();
        let _ = fs::remove_file(&self.socket);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::os::unix::net::UnixListener;
    use std::path::Path;
    use std::time::Instant;

    const REPONSE: [u8; 9] = [0, 0, 0, 5, 12, 0, 0, 0, 0];

    /// Faux agent : répond à chaque connexion par une liste d'identités vide.
    fn faux_agent(chemin: &Path) {
        std::fs::create_dir_all(chemin.parent().unwrap()).unwrap();
        let l = UnixListener::bind(chemin).unwrap();
        std::thread::spawn(move || {
            for mut s in l.incoming().flatten() {
                let mut req = [0u8; 5];
                if s.read_exact(&mut req).is_ok() {
                    let _ = s.write_all(&REPONSE);
                }
            }
        });
    }

    const D: Duration = Duration::from_millis(500);

    #[test]
    fn faux_agent_reconnu() {
        let t = tempfile::tempdir().unwrap();
        let s = t.path().join("a");
        faux_agent(&s);
        assert!(agent_joignable(&s, D));
    }

    #[test]
    fn socket_muet_refuse() {
        let t = tempfile::tempdir().unwrap();
        let s = t.path().join("a");
        let l = UnixListener::bind(&s).unwrap();
        std::thread::spawn(move || {
            let mut gardes = Vec::new();
            for c in l.incoming().flatten() {
                gardes.push(c);
            }
        });
        let debut = Instant::now();
        assert!(!agent_joignable(&s, DELAI_SONDE));
        assert!(debut.elapsed() < 2 * DELAI_SONDE);
    }

    #[test]
    fn socket_mort_refuse() {
        let t = tempfile::tempdir().unwrap();
        let s = t.path().join("a");
        drop(UnixListener::bind(&s).unwrap());
        assert!(!agent_joignable(&s, D));
    }

    #[test]
    fn fichier_ordinaire_ignore() {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("openssh_agent"), b"x").unwrap();
        let tmp = t.path().join("tmp");
        std::fs::create_dir(&tmp).unwrap();
        assert!(candidats_agent(t.path(), &tmp).is_empty());
    }

    #[test]
    fn ordre_des_candidats() {
        let t = tempfile::tempdir().unwrap();
        let rt = t.path().join("rt");
        let tmp = t.path().join("tmp");
        let a = rt.join("gcr/ssh");
        let b = tmp.join("ssh-x/agent.1");
        faux_agent(&a);
        faux_agent(&b);
        assert_eq!(candidats_agent(&rt, &tmp), vec![a, b]);
    }

    #[test]
    fn env_prioritaire() {
        let t = tempfile::tempdir().unwrap();
        let rt = t.path().join("rt");
        let env = t.path().join("env");
        faux_agent(&env);
        faux_agent(&rt.join("gcr/ssh"));
        assert_eq!(
            chercher_agent(Some(&env), &rt, t.path(), D),
            Some(env.clone())
        );
    }

    #[test]
    fn env_mort_puis_candidat() {
        let t = tempfile::tempdir().unwrap();
        let rt = t.path().join("rt");
        let env = t.path().join("env");
        drop(UnixListener::bind(&env).unwrap());
        let c = rt.join("gcr/ssh");
        faux_agent(&c);
        assert_eq!(chercher_agent(Some(&env), &rt, t.path(), D), Some(c));
        assert!(chercher_agent(None, &rt, t.path(), D).is_some());
    }

    #[test]
    fn vrai_ssh_agent() {
        let present = std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).any(|d| d.join("ssh-agent").is_file()))
            .unwrap_or(false);
        if !present {
            eprintln!("ssh-agent absent du PATH : test sauté");
            return;
        }
        let t = tempfile::tempdir().unwrap();
        let s = t.path().join("agent.sock");
        let mut a = AgentPupitre::lancer(&s, &[]).unwrap();
        assert!(agent_joignable(&s, Duration::from_secs(1)));
        assert!(a.vivant());
        assert_eq!(a.socket(), s);
        drop(a);
        assert!(!agent_joignable(&s, D));
    }
}
