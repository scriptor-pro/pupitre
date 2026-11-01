// SPDX-License-Identifier: GPL-3.0-or-later
//! Agent SSH garanti : le processus de fond trouve un agent joignable, ou lance le sien,
//! et fournit aux shells qu'il démarre les variables qui y mènent.

use pupitre_proto::SessionId;
use pupitre_ssh::agent::agent_joignable;
use pupitre_ssh::agent::{chercher_agent, AgentPupitre, DELAI_SONDE};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Intervalle minimal entre deux sondes d'agent.
const INTERVALLE_SONDE: Duration = Duration::from_secs(10);
/// Intervalle entre deux recherches complètes quand aucun agent n'est possible.
const INTERVALLE_SANS_AGENT: Duration = Duration::from_secs(60);
/// Dossier inexistant utilisé quand `XDG_RUNTIME_DIR` est absent : aucun candidat n'y existe.
const SANS_RUNTIME: &str = "/nonexistent-pupitre-runtime";

/// Texte envoyé une fois au client quand aucun agent n'est possible.
pub fn message_sans_agent() -> String {
    pupitre_i18n::t("ssh-sans-agent")
}

/// Ce dont dépend la recherche d'agent ; construit depuis l'environnement en production,
/// injectable dans les tests (les variables d'environnement sont communes au processus).
#[derive(Debug, Clone)]
pub struct EntreesSsh {
    /// `SSH_AUTH_SOCK` hérité par le processus de fond.
    pub env_sock: Option<PathBuf>,
    /// `XDG_RUNTIME_DIR`.
    pub runtime: Option<PathBuf>,
    /// Dossier des sockets temporaires (`/tmp`).
    pub tmp: PathBuf,
    /// Programme `pupitre-askpass`, s'il existe.
    pub askpass: Option<PathBuf>,
    /// Programme d'agent à lancer.
    pub programme_agent: OsString,
}

impl EntreesSsh {
    /// Entrées issues de l'environnement du processus.
    pub fn depuis_env() -> EntreesSsh {
        let chemin = |v: &str| {
            std::env::var_os(v)
                .filter(|p| !p.is_empty())
                .map(PathBuf::from)
        };
        EntreesSsh {
            env_sock: chemin("SSH_AUTH_SOCK"),
            runtime: chemin("XDG_RUNTIME_DIR").filter(|p| p.is_absolute()),
            tmp: std::env::temp_dir(),
            askpass: chemin_askpass(),
            programme_agent: OsString::from("ssh-agent"),
        }
    }
}

/// `PUPITRE_ASKPASS`, sinon `pupitre-askpass` à côté de l'exécutable courant ;
/// seulement si le fichier existe.
pub fn chemin_askpass() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("PUPITRE_ASKPASS").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(p)).filter(|p| p.is_file());
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join("pupitre-askpass")))
        .filter(|p| p.is_file())
}

/// État SSH du processus de fond (possédé par la boucle centrale du serveur).
pub struct EtatSsh {
    socket_fond: PathBuf,
    entrees: EntreesSsh,
    /// Agent à indiquer aux shells.
    courant: Option<PathBuf>,
    /// Notre propre agent, s'il a fallu en lancer un (tué à la destruction).
    propre: Option<AgentPupitre>,
    derniere_sonde: Option<Instant>,
    changement: bool,
    message_du: bool,
    message_envoye: bool,
}

impl EtatSsh {
    /// Cherche un agent (ou lance le sien) à partir de l'environnement du processus.
    pub fn new(socket_fond: &Path, askpass: Option<PathBuf>) -> Self {
        let mut entrees = EntreesSsh::depuis_env();
        entrees.askpass = askpass;
        Self::avec_entrees(socket_fond, entrees)
    }

    /// Comme [`EtatSsh::new`], avec des entrées choisies.
    pub fn avec_entrees(socket_fond: &Path, entrees: EntreesSsh) -> Self {
        let mut etat = EtatSsh {
            socket_fond: socket_fond.to_path_buf(),
            entrees,
            courant: None,
            propre: None,
            derniere_sonde: None,
            changement: false,
            message_du: false,
            message_envoye: false,
        };
        etat.actualiser();
        etat
    }

    /// Variables d'environnement à donner au shell de la session `session`.
    pub fn variables(&mut self, session: SessionId) -> Vec<(String, String)> {
        self.actualiser();
        let texte = |p: &Path| p.to_string_lossy().into_owned();
        let mut v = Vec::new();
        if let Some(s) = &self.courant {
            v.push(("SSH_AUTH_SOCK".to_string(), texte(s)));
        }
        if let Some(a) = &self.entrees.askpass {
            v.push(("SSH_ASKPASS".to_string(), texte(a)));
            v.push(("SSH_ASKPASS_REQUIRE".to_string(), "force".to_string()));
        }
        v.push(("PUPITRE_SOCKET".to_string(), texte(&self.socket_fond)));
        v.push(("PUPITRE_SESSION".to_string(), session.to_string()));
        v
    }

    /// Socket de l'agent à indiquer aux shells, s'il y en a un.
    pub fn agent(&self) -> Option<&Path> {
        self.courant.as_deref()
    }

    /// Programme `pupitre-askpass`, s'il existe.
    pub fn askpass(&self) -> Option<&Path> {
        self.entrees.askpass.as_deref()
    }

    /// Vrai une fois après chaque nouvel agent adopté ou lancé.
    pub fn agent_change(&mut self) -> bool {
        std::mem::take(&mut self.changement)
    }

    /// Message d'indisponibilité de l'agent, rendu une seule fois pendant la vie du fond.
    pub fn message_a_envoyer(&mut self) -> Option<String> {
        if self.message_du && !self.message_envoye {
            self.message_envoye = true;
            return Some(message_sans_agent());
        }
        None
    }

    fn actualiser(&mut self) {
        let mut urgent = false;
        if let Some(a) = &mut self.propre {
            if !a.vivant() {
                // Notre agent est mort : on le relance sans attendre l'intervalle.
                self.propre = None;
                self.courant = None;
                urgent = true;
            }
        }
        let intervalle = if self.courant.is_none() {
            INTERVALLE_SANS_AGENT
        } else {
            INTERVALLE_SONDE
        };
        let du = urgent
            || self
                .derniere_sonde
                .is_none_or(|t| t.elapsed() >= intervalle);
        if !du {
            return;
        }
        if self.propre.is_some() {
            return; // notre agent tourne : rien à sonder
        }
        self.derniere_sonde = Some(Instant::now());
        if let Some(s) = &self.courant {
            if agent_joignable(s, DELAI_SONDE) {
                return;
            }
            self.courant = None;
        }
        let runtime = self
            .entrees
            .runtime
            .clone()
            .unwrap_or_else(|| PathBuf::from(SANS_RUNTIME));
        if let Some(trouve) = chercher_agent(
            self.entrees.env_sock.as_deref(),
            &runtime,
            &self.entrees.tmp,
            DELAI_SONDE,
        ) {
            self.adopter(trouve);
            return;
        }
        // Un socket de Pupitre resté d'un fond précédent et qui répond encore : repris tel quel
        // (il n'est pas à nous, donc jamais tué).
        if let Some(d) = self.socket_fond.parent() {
            let reste = d.join("agent.sock");
            if agent_joignable(&reste, DELAI_SONDE) {
                self.adopter(reste);
                return;
            }
        }
        match self.lancer_propre() {
            Some(a) => {
                let socket = a.socket().to_path_buf();
                self.propre = Some(a);
                self.adopter(socket);
            }
            None => self.message_du = true,
        }
    }

    fn adopter(&mut self, socket: PathBuf) {
        self.courant = Some(socket);
        self.changement = true;
        self.message_du = false;
    }

    /// Lance notre `ssh-agent` ; `ssh-agent` ne lit `SSH_ASKPASS` qu'au démarrage, d'où
    /// les variables : les confirmations de `ssh-add -c` passent alors par Pupitre.
    fn lancer_propre(&self) -> Option<AgentPupitre> {
        let dossier = self.socket_fond.parent()?;
        let mut env = Vec::new();
        if let Some(a) = &self.entrees.askpass {
            env.push(("SSH_ASKPASS".to_string(), a.to_string_lossy().into_owned()));
            env.push(("SSH_ASKPASS_REQUIRE".to_string(), "force".to_string()));
            env.push((
                "PUPITRE_SOCKET".to_string(),
                self.socket_fond.to_string_lossy().into_owned(),
            ));
        }
        AgentPupitre::lancer_programme(
            &self.entrees.programme_agent,
            &dossier.join("agent.sock"),
            &env,
        )
        .ok()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn agent_change_une_fois_par_agent() {
        let t = tempfile::tempdir().unwrap();
        let tmp = t.path().join("tmp");
        std::fs::create_dir(&tmp).unwrap();
        let entrees = EntreesSsh {
            env_sock: None,
            runtime: None,
            tmp,
            askpass: None,
            programme_agent: "ssh-agent".into(),
        };
        let mut e = EtatSsh::avec_entrees(&t.path().join("fond.sock"), entrees);
        assert!(e.agent_change(), "premier agent signalé");
        assert!(!e.agent_change(), "signalé une seule fois");
        let v = e.variables(7);
        assert!(v.iter().any(|(k, _)| k == "SSH_AUTH_SOCK"));
        assert!(!e.agent_change(), "même agent : rien de nouveau");
        assert!(e.message_a_envoyer().is_none());
    }

    #[test]
    fn sans_agent_pas_de_variable_et_message_unique() {
        let t = tempfile::tempdir().unwrap();
        let entrees = EntreesSsh {
            env_sock: None,
            runtime: None,
            tmp: t.path().to_path_buf(),
            askpass: None,
            programme_agent: "/nonexistent/ssh-agent".into(),
        };
        let mut e = EtatSsh::avec_entrees(&t.path().join("fond.sock"), entrees);
        let v = e.variables(1);
        assert!(!v.iter().any(|(k, _)| k == "SSH_AUTH_SOCK"));
        assert!(!e.agent_change());
        assert!(e.message_a_envoyer().is_some());
        assert!(e.message_a_envoyer().is_none());
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests_reprise {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn agent_sock_restant_est_repris() {
        let t = tempfile::tempdir().unwrap();
        let reste = t.path().join("agent.sock");
        let l = std::os::unix::net::UnixListener::bind(&reste).unwrap();
        std::thread::spawn(move || {
            for mut s in l.incoming().flatten() {
                let mut r = [0u8; 5];
                if s.read_exact(&mut r).is_ok() {
                    let _ = s.write_all(&[0, 0, 0, 5, 12, 0, 0, 0, 0]);
                }
            }
        });
        let tmp = t.path().join("tmp");
        std::fs::create_dir(&tmp).unwrap();
        let entrees = EntreesSsh {
            env_sock: None,
            runtime: None,
            tmp,
            askpass: None,
            programme_agent: "/nonexistent/ssh-agent".into(),
        };
        let mut e = EtatSsh::avec_entrees(&t.path().join("fond.sock"), entrees);
        let v = e.variables(1);
        assert!(v.contains(&(
            "SSH_AUTH_SOCK".to_string(),
            reste.to_string_lossy().into_owned()
        )));
        assert!(e.message_a_envoyer().is_none());
    }
}
