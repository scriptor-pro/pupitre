// SPDX-License-Identifier: GPL-3.0-or-later
//! Chargement dans l'agent SSH des clés dont la phrase de passe est mémorisée.
//!
//! Tout se passe hors du thread central (voir `serveur.rs`) : le trousseau et `ssh-add`
//! peuvent être lents. `ssh-add` obtient la phrase par `pupitre-askpass` en mode
//! trousseau ; aucune phrase ne passe donc par ce module ni par une ligne de commande.

use nix::sys::signal::{killpg, Signal};
use nix::unistd::Pid;
use pupitre_i18n::{t, ta, ArgI18n};
use pupitre_ssh::trousseau::{chemin_lisible, Coffre, EtatTrousseau};
use std::collections::HashSet;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Durée maximale d'un `ssh-add` avant qu'il soit tué.
pub const DELAI_SSH_ADD: Duration = Duration::from_secs(20);
/// Durée maximale de `ssh-add -L`.
const DELAI_LISTE: Duration = Duration::from_secs(5);
const SONDE: Duration = Duration::from_millis(20);

/// Texte du bandeau quand le trousseau est verrouillé.
pub fn message_verrouille() -> String {
    t("chargement-trousseau-verrouille")
}

/// Texte du bandeau quand une phrase mémorisée a été refusée.
pub fn message_oubliee(cle: &Path) -> String {
    ta(
        "chargement-memorisee-oubliee",
        &[("cle", ArgI18n::from(chemin_lisible(cle)))],
    )
}

/// Texte du bandeau quand la phrase qu'on vient de saisir (et de mémoriser) n'ouvre pas la clé.
pub fn message_non_memorisee(cle: &Path) -> String {
    ta(
        "chargement-non-memorisee",
        &[("cle", ArgI18n::from(chemin_lisible(cle)))],
    )
}

/// Ce qu'il faut charger.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Travail {
    /// Toutes les clés du trousseau (démarrage, agent relancé, nouvel essai).
    pub toutes: bool,
    /// Clés précises (une phrase vient d'être mémorisée).
    pub cles: Vec<PathBuf>,
}

impl Travail {
    pub fn est_vide(&self) -> bool {
        !self.toutes && self.cles.is_empty()
    }

    /// Ajoute `autre` à ce travail (regroupement des demandes pendant un chargement).
    pub fn fusionner(&mut self, autre: Travail) {
        self.toutes |= autre.toutes;
        for c in autre.cles {
            if !self.cles.contains(&c) {
                self.cles.push(c);
            }
        }
    }
}

/// Résultat d'un travail de chargement.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Rapport {
    /// Messages à montrer à l'utilisateur.
    pub messages: Vec<String>,
    /// Vrai si le trousseau était verrouillé (rien n'a pu être chargé).
    pub verrouille: bool,
}

/// Attend la fin de `enfant` au plus `delai` ; passé ce délai, tue tout son groupe de
/// processus (`ssh-add` et l'askpass qu'il a lancé). `None` = délai dépassé.
fn attendre(enfant: &mut std::process::Child, delai: Duration) -> Option<std::process::ExitStatus> {
    let limite = Instant::now() + delai;
    loop {
        match enfant.try_wait() {
            Ok(Some(s)) => return Some(s),
            Ok(None) => {}
            Err(_) => break,
        }
        if Instant::now() >= limite {
            break;
        }
        std::thread::sleep(SONDE);
    }
    if let Ok(pid) = i32::try_from(enfant.id()) {
        let _ = killpg(Pid::from_raw(pid), Signal::SIGKILL);
    }
    let _ = enfant.kill();
    let _ = enfant.wait();
    None
}

/// Charge `cle` dans l'agent `agent` avec `ssh-add -- <cle>`. La phrase vient de
/// `askpass` en mode trousseau. Le texte d'erreur ne contient jamais de sortie de
/// `ssh-add`.
pub fn charger_cle(agent: &Path, askpass: &Path, cle: &Path) -> Result<(), String> {
    charger_cle_delai(agent, askpass, cle, DELAI_SSH_ADD)
}

pub fn charger_cle_delai(
    agent: &Path,
    askpass: &Path,
    cle: &Path,
    delai: Duration,
) -> Result<(), String> {
    let mut enfant = Command::new("ssh-add")
        .arg("--")
        .arg(cle)
        .env("SSH_AUTH_SOCK", agent)
        .env("SSH_ASKPASS", askpass)
        .env("SSH_ASKPASS_REQUIRE", "force")
        .env("PUPITRE_ASKPASS_MODE", "trousseau")
        .env_remove("SSH_ASKPASS_PROMPT")
        .env_remove("PUPITRE_SOCKET")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|e| {
            ta(
                "chargement-ssh-add-lancement",
                &[("erreur", ArgI18n::from(e.to_string()))],
            )
        })?;
    match attendre(&mut enfant, delai) {
        Some(s) if s.success() => Ok(()),
        Some(s) => Err(ta(
            "chargement-ssh-add-echec",
            &[("etat", ArgI18n::from(s.to_string()))],
        )),
        None => Err(t("chargement-ssh-add-delai")),
    }
}

/// Deux premiers champs (type et clé en base 64) d'une ligne de clé publique.
fn empreinte_ligne(ligne: &str) -> Option<(String, String)> {
    let mut champs = ligne.split_whitespace();
    Some((champs.next()?.to_string(), champs.next()?.to_string()))
}

/// Clés déjà dans l'agent (`ssh-add -L`) ; vide si la liste est introuvable.
fn cles_de_l_agent(agent: &Path) -> HashSet<(String, String)> {
    let Ok(mut enfant) = Command::new("ssh-add")
        .arg("-L")
        .env("SSH_AUTH_SOCK", agent)
        .env_remove("SSH_ASKPASS")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
    else {
        return HashSet::new();
    };
    // Lecture dans un fil à part : la sortie d'un agent bavard ne doit pas bloquer l'attente.
    let Some(sortie) = enfant.stdout.take() else {
        return HashSet::new();
    };
    let lecteur = std::thread::spawn(move || {
        use std::io::Read;
        let mut texte = String::new();
        let _ = sortie.take(1 << 20).read_to_string(&mut texte);
        texte
    });
    let fini = attendre(&mut enfant, DELAI_LISTE).is_some();
    let texte = lecteur.join().unwrap_or_default();
    if !fini {
        return HashSet::new();
    }
    texte.lines().filter_map(empreinte_ligne).collect()
}

/// Vrai si la clé publique `<cle>.pub` est parmi `chargees`.
fn deja_chargee(cle: &Path, chargees: &HashSet<(String, String)>) -> bool {
    let mut pub_ = cle.as_os_str().to_owned();
    pub_.push(".pub");
    std::fs::read_to_string(PathBuf::from(pub_))
        .ok()
        .and_then(|t| t.lines().find_map(empreinte_ligne))
        .is_some_and(|e| chargees.contains(&e))
}

/// Exécute `travail` : charge les clés mémorisées absentes de l'agent.
pub fn executer(coffre: &dyn Coffre, agent: &Path, askpass: &Path, travail: &Travail) -> Rapport {
    let mut rapport = Rapport::default();
    match coffre.etat() {
        EtatTrousseau::Verrouille => {
            rapport.verrouille = true;
            return rapport;
        }
        EtatTrousseau::Absent => return rapport,
        EtatTrousseau::Disponible => {}
    }
    let mut cles = if travail.toutes {
        coffre.cles()
    } else {
        Vec::new()
    };
    for c in &travail.cles {
        if !cles.contains(c) {
            cles.push(c.clone());
        }
    }
    // Seules les clés qui sont des fichiers ordinaires : une entrée de trousseau ne doit
    // pas faire inventer une invite pour un chemin quelconque.
    cles.retain(|c| std::fs::metadata(c).is_ok_and(|m| m.is_file()));
    if cles.is_empty() {
        return rapport;
    }
    let chargees = cles_de_l_agent(agent);
    for cle in cles {
        // Seule une phrase effectivement lue est chargée ; trousseau muet ou erreur : on
        // n'essaie pas (et on ne dit rien).
        if deja_chargee(&cle, &chargees) || !matches!(coffre.lire(&cle), Ok(Some(_))) {
            continue;
        }
        // Après un échec, seule une entrée réellement absente (`Ok(None)`) prouve que
        // l'askpass a vu « Bad passphrase » et l'a oubliée. Pour tout autre cas (délai du
        // trousseau, agent disparu, `ssh-add` tué) l'entrée reste et on ne dit rien.
        if charger_cle(agent, askpass, &cle).is_err() && matches!(coffre.lire(&cle), Ok(None)) {
            rapport.messages.push(if travail.cles.contains(&cle) {
                message_non_memorisee(&cle)
            } else {
                message_oubliee(&cle)
            });
        }
    }
    rapport
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use pupitre_ssh::agent::AgentPupitre;
    use pupitre_ssh::trousseau::CoffreMemoire;
    use std::os::unix::fs::PermissionsExt;

    /// Clé jetable. `-N` met la phrase de test sur la ligne de commande : acceptable ici
    /// (phrase sans valeur, processus de test), interdit en production.
    fn cle_jetable(dossier: &Path, phrase: &str) -> PathBuf {
        let cle = dossier.join("cle");
        let ok = Command::new("ssh-keygen")
            .args(["-q", "-t", "ed25519", "-N", phrase, "-f"])
            .arg(&cle)
            .status()
            .unwrap()
            .success();
        assert!(ok);
        cle
    }

    /// Faux askpass : répond `phrase` à la première invite, puis refuse les nouveaux essais
    /// (comme `pupitre-askpass` en mode trousseau ; sans cela `ssh-add` insiste sans fin).
    fn askpass_fixe(dossier: &Path, phrase: &str) -> PathBuf {
        let p = dossier.join("askpass.sh");
        std::fs::write(
            &p,
            format!("#!/bin/sh\ncase \"$1\" in Bad*) exit 1;; esac\necho '{phrase}'\n"),
        )
        .unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700)).unwrap();
        p
    }

    fn agent_jetable(dossier: &Path) -> AgentPupitre {
        AgentPupitre::lancer(&dossier.join("agent.sock"), &[]).unwrap()
    }

    fn liste(agent: &Path) -> String {
        let s = Command::new("ssh-add")
            .arg("-l")
            .env("SSH_AUTH_SOCK", agent)
            .output()
            .unwrap();
        String::from_utf8_lossy(&s.stdout).into_owned()
    }

    #[test]
    fn charge_avec_la_bonne_phrase() {
        let t = tempfile::tempdir().unwrap();
        let cle = cle_jetable(t.path(), "bonne");
        let agent = agent_jetable(t.path());
        let ask = askpass_fixe(t.path(), "bonne");
        charger_cle(agent.socket(), &ask, &cle).unwrap();
        assert!(liste(agent.socket()).contains("ED25519"));
    }

    #[test]
    fn echoue_avec_une_mauvaise_phrase() {
        let t = tempfile::tempdir().unwrap();
        let cle = cle_jetable(t.path(), "bonne");
        let agent = agent_jetable(t.path());
        let ask = askpass_fixe(t.path(), "fausse");
        let debut = Instant::now();
        assert!(charger_cle(agent.socket(), &ask, &cle).is_err());
        assert!(debut.elapsed() < Duration::from_secs(20));
        assert!(!liste(agent.socket()).contains("ED25519"));
    }

    #[test]
    fn tue_ssh_add_et_l_askpass_apres_le_delai() {
        let t = tempfile::tempdir().unwrap();
        let cle = cle_jetable(t.path(), "bonne");
        let agent = agent_jetable(t.path());
        let ask = t.path().join("lent.sh");
        let marque = t.path().join("pid");
        std::fs::write(
            &ask,
            format!("#!/bin/sh\necho $$ > {}\nexec sleep 60\n", marque.display()),
        )
        .unwrap();
        std::fs::set_permissions(&ask, std::fs::Permissions::from_mode(0o700)).unwrap();
        let debut = Instant::now();
        let r = charger_cle_delai(agent.socket(), &ask, &cle, Duration::from_millis(700));
        assert!(r.unwrap_err().contains("délai"));
        assert!(debut.elapsed() < Duration::from_secs(5));
        let pid: i32 = std::fs::read_to_string(&marque)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        std::thread::sleep(Duration::from_millis(200));
        assert!(
            nix::sys::signal::kill(Pid::from_raw(pid), None).is_err(),
            "l'askpass lent aurait dû être tué"
        );
    }

    #[test]
    fn executer_saute_la_cle_deja_chargee_et_les_non_fichiers() {
        let t = tempfile::tempdir().unwrap();
        let cle = cle_jetable(t.path(), "bonne");
        let agent = agent_jetable(t.path());
        let ask = askpass_fixe(t.path(), "bonne");
        let coffre = CoffreMemoire::default();
        coffre.ecrire(&cle, "bonne").unwrap();
        coffre.ecrire(Path::new("/nulle/part/cle"), "x").unwrap();
        coffre.ecrire(t.path(), "dossier").unwrap();
        let travail = Travail {
            toutes: true,
            cles: vec![],
        };
        let r = executer(&coffre, agent.socket(), &ask, &travail);
        assert!(r.messages.is_empty() && !r.verrouille, "{r:?}");
        assert!(liste(agent.socket()).contains("ED25519"));
        // Deuxième passage : déjà chargée ; un askpass faux ne serait même pas appelé.
        let faux = askpass_fixe(t.path(), "fausse");
        let r = executer(&coffre, agent.socket(), &faux, &travail);
        assert!(r.messages.is_empty(), "{r:?}");
    }

    #[test]
    fn echec_sans_oubli_ne_dit_rien() {
        // Avec un faux askpass, l'entrée n'est pas oubliée : pas de message « oubliée ».
        let t = tempfile::tempdir().unwrap();
        let cle = cle_jetable(t.path(), "bonne");
        let agent = agent_jetable(t.path());
        let ask = askpass_fixe(t.path(), "fausse");
        let coffre = CoffreMemoire::default();
        coffre.ecrire(&cle, "fausse").unwrap();
        let r = executer(&coffre, agent.socket(), &ask, &Travail::default());
        assert!(r.messages.is_empty());
        let travail = Travail {
            toutes: true,
            cles: vec![],
        };
        let r = executer(&coffre, agent.socket(), &ask, &travail);
        assert!(r.messages.is_empty(), "{r:?}");
        assert!(coffre.lire(&cle).unwrap().is_some());
    }

    /// Coffre lent : la première lecture répond, les suivantes dépassent le délai (machine
    /// chargée, démon du trousseau occupé).
    struct CoffreLent {
        cle: PathBuf,
        lectures: std::sync::atomic::AtomicUsize,
    }

    impl Coffre for CoffreLent {
        fn etat(&self) -> EtatTrousseau {
            EtatTrousseau::Disponible
        }
        fn lire(
            &self,
            _: &Path,
        ) -> Result<Option<zeroize::Zeroizing<String>>, pupitre_ssh::trousseau::ErreurTrousseau>
        {
            use std::sync::atomic::Ordering;
            if self.lectures.fetch_add(1, Ordering::SeqCst) == 0 {
                Ok(Some(zeroize::Zeroizing::new("x".into())))
            } else {
                Err(pupitre_ssh::trousseau::ErreurTrousseau::Delai)
            }
        }
        fn ecrire(&self, _: &Path, _: &str) -> Result<(), pupitre_ssh::trousseau::ErreurTrousseau> {
            Err(pupitre_ssh::trousseau::ErreurTrousseau::Delai)
        }
        fn oublier(&self, _: &Path) -> Result<(), pupitre_ssh::trousseau::ErreurTrousseau> {
            Err(pupitre_ssh::trousseau::ErreurTrousseau::Delai)
        }
        fn cles(&self) -> Vec<PathBuf> {
            vec![self.cle.clone()]
        }
    }

    #[test]
    fn delai_du_trousseau_ne_dit_pas_oubliee() {
        // `ssh-add` échoue (l'askpass n'a rien pu lire), puis le trousseau ne répond pas :
        // on ne sait rien de l'entrée, aucun message « oubliée » ou « non mémorisée ».
        let t = tempfile::tempdir().unwrap();
        let cle = cle_jetable(t.path(), "bonne");
        let agent = agent_jetable(t.path());
        let ask = askpass_fixe(t.path(), "fausse");
        for saisie in [false, true] {
            let coffre = CoffreLent {
                cle: cle.clone(),
                lectures: Default::default(),
            };
            let travail = Travail {
                toutes: !saisie,
                cles: if saisie { vec![cle.clone()] } else { vec![] },
            };
            let r = executer(&coffre, agent.socket(), &ask, &travail);
            assert!(r.messages.is_empty(), "{r:?}");
            assert_eq!(
                coffre.lectures.load(std::sync::atomic::Ordering::SeqCst),
                2,
                "relu après l'échec"
            );
        }
    }

    /// Coffre à une clé dont l'entrée existe tant que le fichier `marque` existe : le faux
    /// askpass le supprime au « Bad passphrase », comme le vrai oublie l'entrée.
    struct CoffreMarque {
        marque: PathBuf,
        cle: PathBuf,
    }

    impl Coffre for CoffreMarque {
        fn etat(&self) -> EtatTrousseau {
            EtatTrousseau::Disponible
        }
        fn lire(
            &self,
            c: &Path,
        ) -> Result<Option<zeroize::Zeroizing<String>>, pupitre_ssh::trousseau::ErreurTrousseau>
        {
            Ok(
                (c == self.cle && self.marque.exists())
                    .then(|| zeroize::Zeroizing::new("x".into())),
            )
        }
        fn ecrire(&self, _: &Path, _: &str) -> Result<(), pupitre_ssh::trousseau::ErreurTrousseau> {
            Ok(())
        }
        fn oublier(&self, _: &Path) -> Result<(), pupitre_ssh::trousseau::ErreurTrousseau> {
            Ok(())
        }
        fn cles(&self) -> Vec<PathBuf> {
            vec![self.cle.clone()]
        }
    }

    /// Lance `executer` avec une mauvaise phrase qui fait oublier l'entrée ; `saisie` vrai =
    /// chargement d'une phrase qu'on vient de saisir (clé désignée), sinon chargement général.
    fn rapport_phrase_fausse(saisie: bool) -> (Rapport, PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let cle = cle_jetable(t.path(), "bonne");
        let agent = agent_jetable(t.path());
        let marque = t.path().join("entree");
        std::fs::write(&marque, "").unwrap();
        let ask = t.path().join("oublieux.sh");
        std::fs::write(
            &ask,
            format!(
                "#!/bin/sh\ncase \"$1\" in Bad*) rm -f {}; exit 1;; esac\necho fausse\n",
                marque.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&ask, std::fs::Permissions::from_mode(0o700)).unwrap();
        let coffre = CoffreMarque {
            marque,
            cle: cle.clone(),
        };
        let travail = Travail {
            toutes: !saisie,
            cles: if saisie { vec![cle.clone()] } else { vec![] },
        };
        (executer(&coffre, agent.socket(), &ask, &travail), cle)
    }

    #[test]
    fn phrase_fausse_au_demarrage_message_oubliee() {
        let (r, cle) = rapport_phrase_fausse(false);
        assert_eq!(r.messages, vec![message_oubliee(&cle)]);
    }

    #[test]
    fn phrase_saisie_fausse_message_non_memorisee() {
        let (r, cle) = rapport_phrase_fausse(true);
        assert_eq!(r.messages, vec![message_non_memorisee(&cle)]);
        assert_eq!(
            message_non_memorisee(Path::new("/opt/k")),
            "La phrase de passe saisie pour /opt/k n'a pas été mémorisée : elle n'ouvre pas la clé."
        );
    }

    #[test]
    fn travail_fusionne() {
        let mut a = Travail {
            toutes: false,
            cles: vec!["/a".into()],
        };
        a.fusionner(Travail {
            toutes: true,
            cles: vec!["/a".into(), "/b".into()],
        });
        assert!(a.toutes);
        assert_eq!(a.cles, vec![PathBuf::from("/a"), PathBuf::from("/b")]);
        assert!(Travail::default().est_vide());
    }

    #[test]
    fn texte_du_message_oublie() {
        assert_eq!(
            message_oubliee(Path::new("/opt/k")),
            "La phrase de passe mémorisée pour /opt/k ne fonctionne plus ; elle a été oubliée. Elle sera redemandée."
        );
    }
}
