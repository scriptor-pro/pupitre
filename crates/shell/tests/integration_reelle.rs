// SPDX-License-Identifier: GPL-3.0-or-later
//! Intégration réelle : chaque shell installé est lancé dans un PTY, avec ce que produit
//! `pupitre_shell::lancement`, dans un `HOME` temporaire (jamais le vrai).
//! Un shell absent (ou Starship absent pour le mode Starship) : test sauté avec un message.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use pupitre_shell::{lancement, Lancement, ModeInvite, Ressources};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Délai maximal généreux : la machine peut être chargée.
const DELAI: Duration = Duration::from_secs(25);

// ---------------------------------------------------------------------------------------
// Outils
// ---------------------------------------------------------------------------------------

fn dans_path(nom: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(nom))
        .find(|p| p.is_file())
}

fn dossier_integration() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/shell")
}

/// Marques extraites du flux : `A`, `B`, `C`, `D;<code>` (lettres inconnues ignorées,
/// paramètres après le code ignorés, terminateurs `BEL` et `ESC \` acceptés).
fn marques(flux: &[u8]) -> Vec<String> {
    let mut res = Vec::new();
    for charge in charges_osc(flux) {
        let Some(reste) = charge.strip_prefix("133;") else {
            continue;
        };
        let mut parts = reste.split(';');
        match parts.next().unwrap_or("") {
            l @ ("A" | "B" | "C") => res.push(l.to_string()),
            "D" => res.push(match parts.next() {
                Some(c) if !c.is_empty() => format!("D;{c}"),
                _ => "D".to_string(),
            }),
            _ => {}
        }
    }
    res
}

/// Chemins annoncés par les OSC 7 (`file://hôte/chemin`).
fn dossiers(flux: &[u8]) -> Vec<String> {
    charges_osc(flux)
        .iter()
        .filter_map(|c| c.strip_prefix("7;file://"))
        .map(|r| r.find('/').map_or(String::new(), |i| r[i..].to_string()))
        .collect()
}

/// Contenu (entre `ESC ]` et le terminateur) de chaque séquence OSC du flux.
fn charges_osc(flux: &[u8]) -> Vec<String> {
    let mut res = Vec::new();
    let mut i = 0;
    while i + 1 < flux.len() {
        if flux[i] == 0x1b && flux[i + 1] == b']' {
            let debut = i + 2;
            let mut j = debut;
            while j < flux.len() {
                if flux[j] == 0x07 || (flux[j] == 0x1b && flux.get(j + 1) == Some(&b'\\')) {
                    break;
                }
                j += 1;
            }
            if j >= flux.len() {
                break; // séquence incomplète
            }
            res.push(String::from_utf8_lossy(&flux[debut..j]).into_owned());
            i = j + 1;
        } else {
            i += 1;
        }
    }
    res
}

/// Marques sans les réaffichages de l'invite : nushell (reedline) redessine l'invite à chaque
/// frappe, donc `A B` en plusieurs exemplaires d'affilée ; une paire `A B` suivie d'un `A` est
/// une invite remplacée par la suivante, on ne garde que la dernière.
fn normalisees(flux: &[u8]) -> Vec<String> {
    let m = marques(flux);
    let mut res: Vec<String> = Vec::new();
    for x in m {
        if x == "A" && res.len() >= 2 && res[res.len() - 2] == "A" && res[res.len() - 1] == "B" {
            res.truncate(res.len() - 2);
        }
        res.push(x);
    }
    res
}

fn compte(m: &[String], v: &str) -> usize {
    m.iter().filter(|x| x.as_str() == v).count()
}

fn sauter(raison: &str) {
    eprintln!("SAUTÉ : {raison}");
}

// ---------------------------------------------------------------------------------------
// Banc d'essai : HOME temporaire + ressources
// ---------------------------------------------------------------------------------------

struct Banc {
    home: tempfile::TempDir,
    _exec: tempfile::TempDir,
    config_starship: PathBuf,
    /// Dossier `assets/shell` utilisé (la copie d'un test peut le remplacer).
    integration: PathBuf,
}

impl Banc {
    fn nouveau() -> Banc {
        let home = tempfile::tempdir().unwrap();
        let exec = tempfile::tempdir().unwrap();
        for d in [".config", ".local/share", ".local/state", ".cache"] {
            std::fs::create_dir_all(home.path().join(d)).unwrap();
        }
        let config_starship = exec.path().join("starship.toml");
        std::fs::write(
            &config_starship,
            "format = \"STAR> \"\nadd_newline = false\n",
        )
        .unwrap();
        Banc {
            home,
            _exec: exec,
            config_starship,
            integration: dossier_integration(),
        }
    }

    fn ecrire(&self, relatif: &str, contenu: &str) {
        let p = self.home.path().join(relatif);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, contenu).unwrap();
    }

    /// Fichiers habituels de l'utilisateur : posent `VU=1` et une invite `MONINVITE> `.
    fn config_utilisateur(&self, shell: &str) {
        match shell {
            "bash" => self.ecrire(".bashrc", "VU=1\nPS1='MONINVITE> '\n"),
            "zsh" => self.ecrire(".zshrc", "VU=1\nPROMPT='MONINVITE> '\n"),
            "fish" => self.ecrire(
                ".config/fish/config.fish",
                "set -g VU 1\nfunction fish_prompt\n    echo -n 'MONINVITE> '\nend\n",
            ),
            "nu" => {
                self.ecrire(".config/nushell/env.nu", "");
                self.ecrire(
                    ".config/nushell/config.nu",
                    "$env.VU = \"1\"\n$env.PROMPT_COMMAND = {|| \"MONINVITE> \" }\n$env.PROMPT_INDICATOR = \"\"\n",
                );
            }
            "elvish" => self.ecrire(
                ".config/elvish/rc.elv",
                "set-env VU 1\nset edit:prompt = { put 'MONINVITE> ' }\nset edit:rprompt = { put '' }\n",
            ),
            _ => {}
        }
    }
}

/// Commandes d'un shell (syntaxe propre).
struct Syntaxe {
    ok: &'static str,
    ko: &'static str,
    echo_vu: &'static str,
}

fn syntaxe(shell: &str) -> Syntaxe {
    match shell {
        "nu" => Syntaxe {
            ok: "^true",
            ko: "^false",
            echo_vu: "print $\"R=($env.VU)\"",
        },
        "elvish" => Syntaxe {
            ok: "true",
            ko: "false",
            echo_vu: "echo R=$E:VU",
        },
        _ => Syntaxe {
            ok: "true",
            ko: "false",
            echo_vu: "echo R=$VU",
        },
    }
}

// ---------------------------------------------------------------------------------------
// Session PTY
// ---------------------------------------------------------------------------------------

struct Session {
    enfant: Box<dyn Child + Send + Sync>,
    ecrivain: Arc<Mutex<Box<dyn Write + Send>>>,
    tampon: Arc<Mutex<Vec<u8>>>,
    _maitre: Box<dyn MasterPty + Send>,
}

/// Nom du binaire pour chaque shell testé.
fn binaire(shell: &str) -> &str {
    shell
}

impl Session {
    /// Lance `shell` comme le fait Pupitre. `None` (avec message) si le shell, ou Starship
    /// en mode Starship, est absent.
    fn lancer(banc: &Banc, shell: &str, mode: ModeInvite) -> Option<Session> {
        Self::lancer_avec(banc, shell, mode, |_| {})
    }

    fn lancer_avec(
        banc: &Banc,
        shell: &str,
        mode: ModeInvite,
        ajuster: impl FnOnce(&mut Vec<(String, String)>),
    ) -> Option<Session> {
        let starship = dans_path("starship");
        if mode == ModeInvite::Starship && starship.is_none() {
            sauter("Starship n'est pas installé");
            return None;
        }
        let (session, l) = Self::lancer_ressources(banc, shell, mode, starship, ajuster)?;
        assert_eq!(
            l.avertissement, None,
            "{shell} : fichiers d'intégration absents ou Starship manquant"
        );
        Some(session)
    }

    /// Lance `shell` avec `starship` comme Starship connu de Pupitre (`None` : introuvable).
    /// Renvoie aussi ce que `lancement` a produit.
    fn lancer_ressources(
        banc: &Banc,
        shell: &str,
        mode: ModeInvite,
        starship: Option<PathBuf>,
        ajuster: impl FnOnce(&mut Vec<(String, String)>),
    ) -> Option<(Session, Lancement)> {
        let Some(chemin) = dans_path(binaire(shell)) else {
            sauter(&format!("{shell} n'est pas installé"));
            return None;
        };
        let r = Ressources {
            integration: Some(banc.integration.clone()),
            starship,
            config_starship: banc.config_starship.clone(),
        };
        let l = lancement(&chemin, mode, &r);
        assert!(
            l.argv.len() > 1 || l.env.iter().any(|(k, _)| k == "ZDOTDIR"),
            "{shell} : lancement tel quel (fichiers d'intégration absents) : {:?}",
            l.argv
        );
        let mut env = l.env.clone();
        // L'origine de ZDOTDIR ne doit pas dépendre de l'environnement du test.
        env.retain(|(k, _)| k != "PUPITRE_ZDOTDIR_ORIGINE");
        env.push(("PUPITRE_ZDOTDIR_ORIGINE".into(), String::new()));
        ajuster(&mut env);

        let pty = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 200,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut cmd = CommandBuilder::new(&l.argv[0]);
        cmd.args(&l.argv[1..]);
        cmd.env_clear();
        let home = banc.home.path();
        cmd.env("PATH", std::env::var_os("PATH").unwrap_or_default());
        cmd.env("HOME", home);
        cmd.env("XDG_CONFIG_HOME", home.join(".config"));
        cmd.env("XDG_DATA_HOME", home.join(".local/share"));
        cmd.env("XDG_STATE_HOME", home.join(".local/state"));
        cmd.env("XDG_CACHE_HOME", home.join(".cache"));
        cmd.env("TERM", "xterm-256color");
        cmd.env("LANG", "C.UTF-8");
        cmd.env("USER", "essai");
        for (k, v) in &env {
            cmd.env(k, v);
        }
        cmd.cwd(home);
        let enfant = pty.slave.spawn_command(cmd).unwrap();
        drop(pty.slave);

        let ecrivain: Arc<Mutex<Box<dyn Write + Send>>> =
            Arc::new(Mutex::new(pty.master.take_writer().unwrap()));
        let tampon = Arc::new(Mutex::new(Vec::new()));
        let mut lecteur = pty.master.try_clone_reader().unwrap();
        {
            let tampon = Arc::clone(&tampon);
            let ecrivain = Arc::clone(&ecrivain);
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                while let Ok(n) = lecteur.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    let morceau = &buf[..n];
                    // Réponses aux requêtes de terminal (certains shells attendent).
                    let mut rep: Vec<u8> = Vec::new();
                    if morceau.windows(3).any(|w| w == b"\x1b[c")
                        || morceau.windows(4).any(|w| w == b"\x1b[0c")
                    {
                        rep.extend_from_slice(b"\x1b[?62;c");
                    }
                    if morceau.windows(4).any(|w| w == b"\x1b[6n") {
                        rep.extend_from_slice(b"\x1b[1;1R");
                    }
                    if !rep.is_empty() {
                        if let Ok(mut e) = ecrivain.lock() {
                            let _ = e.write_all(&rep);
                            let _ = e.flush();
                        }
                    }
                    if let Ok(mut t) = tampon.lock() {
                        t.extend_from_slice(morceau);
                    }
                }
            });
        }
        Some((
            Session {
                enfant,
                ecrivain,
                tampon,
                _maitre: pty.master,
            },
            l,
        ))
    }

    fn envoyer(&self, texte: &str) {
        let mut e = self.ecrivain.lock().unwrap();
        e.write_all(texte.as_bytes()).unwrap();
        e.flush().unwrap();
    }

    fn flux(&self) -> Vec<u8> {
        self.tampon.lock().unwrap().clone()
    }

    fn texte(&self) -> String {
        String::from_utf8_lossy(&self.flux()).into_owned()
    }

    /// Attend que `condition` soit vraie sur le flux ; panique avec le flux en cas de délai.
    fn attendre(&self, quoi: &str, condition: impl Fn(&[u8]) -> bool) {
        let fin = Instant::now() + DELAI;
        while Instant::now() < fin {
            if condition(&self.flux()) {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!(
            "délai dépassé en attendant : {quoi}\nflux reçu : {:?}\nmarques : {:?}",
            self.texte(),
            marques(&self.flux())
        );
    }

    fn attendre_marques(&self, n: usize) {
        self.attendre(&format!("{n} marques"), |f| normalisees(f).len() >= n);
    }

    fn attendre_texte(&self, motif: &str) {
        self.attendre(motif, |f| String::from_utf8_lossy(f).contains(motif));
    }

    /// Laisse arriver d'éventuelles marques en trop avant de compter.
    fn stabiliser(&self) {
        std::thread::sleep(Duration::from_millis(400));
    }

    fn fermer(mut self) {
        self.envoyer("exit\n");
        let fin = Instant::now() + DELAI;
        while Instant::now() < fin {
            if matches!(self.enfant.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let _ = self.enfant.kill();
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.enfant.kill();
    }
}

/// Modes à essayer : le mode Shell toujours, Starship si présent.
fn modes() -> Vec<ModeInvite> {
    let mut v = vec![ModeInvite::Shell];
    if dans_path("starship").is_some() {
        v.push(ModeInvite::Starship);
    } else {
        sauter("mode Starship (Starship absent)");
    }
    v
}

// ---------------------------------------------------------------------------------------
// Scénarios
// ---------------------------------------------------------------------------------------

fn verifier_config(shell: &str) {
    let banc = Banc::nouveau();
    banc.config_utilisateur(shell);
    for mode in modes() {
        let Some(s) = Session::lancer(&banc, shell, mode) else {
            return;
        };
        s.attendre_marques(if shell == "elvish" { 1 } else { 2 });
        s.envoyer(&format!("{}\n", syntaxe(shell).echo_vu));
        s.attendre_texte("R=1");
        s.fermer();
    }
}

/// `true` puis `false` : A B C D;0 A B C D;1 A B (elvish : A C D A C D A, sans B ni code).
fn verifier_marques(shell: &str, banc: &Banc, mode: ModeInvite) {
    let Some(s) = Session::lancer(banc, shell, mode) else {
        return;
    };
    let sx = syntaxe(shell);
    let attendu: Vec<&str> = if shell == "elvish" {
        vec!["A", "C", "D", "A", "C", "D", "A"]
    } else {
        vec!["A", "B", "C", "D;0", "A", "B", "C", "D;1", "A", "B"]
    };
    let (n1, n2, n3) = if shell == "elvish" {
        (1, 4, 7)
    } else {
        (2, 6, 10)
    };
    s.attendre_marques(n1);
    s.envoyer(&format!("{}\n", sx.ok));
    s.attendre_marques(n2);
    s.envoyer(&format!("{}\n", sx.ko));
    s.attendre_marques(n3);
    s.stabiliser();
    let m = normalisees(&s.flux());
    let m_attendu: Vec<String> = attendu.iter().map(|x| x.to_string()).collect();
    if shell == "elvish" {
        // elvish ne donne pas le code de sortie : `D` sans code accepté.
        let sans_code: Vec<String> = m
            .iter()
            .map(|x| x.split(';').next().unwrap_or("").to_string())
            .collect();
        assert_eq!(sans_code, m_attendu, "{shell} {mode:?} : {:?}", s.texte());
    } else {
        assert_eq!(m, m_attendu, "{shell} {mode:?} : {:?}", s.texte());
    }
    let texte = s.texte();
    match mode {
        ModeInvite::Starship => assert!(texte.contains("STAR> "), "invite Starship absente"),
        ModeInvite::Shell => assert!(texte.contains("MONINVITE> "), "invite native absente"),
    }
    s.fermer();
}

fn verifier_marques_tous_modes(shell: &str) {
    let banc = Banc::nouveau();
    banc.config_utilisateur(shell);
    for mode in modes() {
        verifier_marques(shell, &banc, mode);
    }
}

fn verifier_dossier(shell: &str) {
    let banc = Banc::nouveau();
    banc.config_utilisateur(shell);
    for mode in modes() {
        let Some(s) = Session::lancer(&banc, shell, mode) else {
            return;
        };
        s.attendre_marques(if shell == "elvish" { 1 } else { 2 });
        s.envoyer("cd /tmp\n");
        s.attendre("OSC 7 avec /tmp", |f| {
            dossiers(f).iter().any(|d| d == "/tmp")
        });
        s.fermer();
    }
}

macro_rules! tests_par_shell {
    ($($shell:literal => $nom:ident),*) => {
        $(
            mod $nom {
                use super::*;
                #[test]
                fn charge_la_config_utilisateur() { verifier_config($shell); }
                #[test]
                fn marques_d_une_commande() { verifier_marques_tous_modes($shell); }
                #[test]
                fn dossier_courant() { verifier_dossier($shell); }
            }
        )*
    };
}

tests_par_shell!("bash" => bash, "zsh" => zsh, "fish" => fish, "nu" => nushell);

#[test]
fn marques_uniques_avec_crochets_utilisateur() {
    for shell in ["bash", "zsh"] {
        for mode in modes() {
            let banc = Banc::nouveau();
            let init_starship = if dans_path("starship").is_some() {
                format!("eval \"$(starship init {shell} --print-full-init)\"\n")
            } else {
                String::new()
            };
            let config = if shell == "bash" {
                format!("PS1='MONINVITE> '\nPROMPT_COMMAND='printf HOOKUTIL'\n{init_starship}")
            } else {
                format!("PROMPT='MONINVITE> '\nprecmd() {{ print -n HOOKUTIL }}\n{init_starship}")
            };
            banc.ecrire(if shell == "bash" { ".bashrc" } else { ".zshrc" }, &config);
            let Some(s) = Session::lancer(&banc, shell, mode) else {
                break;
            };
            s.attendre_marques(2);
            s.envoyer("true\n");
            s.attendre_marques(6);
            s.envoyer("false\n");
            s.attendre_marques(10);
            s.stabiliser();
            let m: Vec<String> = normalisees(&s.flux());
            let attendu: Vec<String> = ["A", "B", "C", "D;0", "A", "B", "C", "D;1", "A", "B"]
                .iter()
                .map(|x| x.to_string())
                .collect();
            assert_eq!(m, attendu, "{shell} {mode:?} : {:?}", s.texte());
            assert!(
                s.texte().contains("HOOKUTIL"),
                "crochet de l'utilisateur non exécuté"
            );
            s.fermer();
        }
    }
}

#[test]
fn fish_sans_double_marque() {
    let banc = Banc::nouveau();
    banc.config_utilisateur("fish");
    for mode in modes() {
        let Some(s) = Session::lancer(&banc, "fish", mode) else {
            return;
        };
        s.attendre_marques(2);
        // Frappe lente : les réaffichages de la ligne ne doivent pas redoubler A ni B.
        for c in "echo bonjour".chars() {
            s.envoyer(&c.to_string());
            std::thread::sleep(Duration::from_millis(30));
        }
        s.envoyer("\n");
        s.attendre_marques(6);
        s.stabiliser();
        let m = marques(&s.flux());
        assert_eq!(compte(&m, "A"), 2, "{mode:?} : {m:?}");
        assert_eq!(compte(&m, "B"), 2, "{mode:?} : {m:?}");
        assert_eq!(compte(&m, "C"), 1, "{mode:?} : {m:?}");
        assert_eq!(compte(&m, "D;0"), 1, "{mode:?} : {m:?}");
        assert_eq!(m.len(), 6, "{mode:?} : {m:?}");
        s.fermer();
    }
}

#[test]
fn mode_shell_garde_l_invite_native() {
    for shell in ["bash", "zsh", "fish", "nu"] {
        let banc = Banc::nouveau();
        banc.config_utilisateur(shell);
        let Some(s) = Session::lancer(&banc, shell, ModeInvite::Shell) else {
            continue;
        };
        s.attendre_marques(2);
        s.stabiliser();
        let texte = s.texte();
        assert!(texte.contains("MONINVITE> "), "{shell} : {texte:?}");
        assert!(!texte.contains("STAR> "), "{shell} : {texte:?}");
        let m = marques(&s.flux());
        assert!(
            m.contains(&"A".to_string()) && m.contains(&"B".to_string()),
            "{shell} : {m:?}"
        );
        s.fermer();
    }
}

#[test]
fn elvish_invite_geree() {
    let banc = Banc::nouveau();
    banc.config_utilisateur("elvish");
    // Mode Shell : config de l'utilisateur et son invite ; marques A, C, D sans B.
    verifier_config("elvish");
    verifier_marques_tous_modes("elvish");
    verifier_dossier("elvish");
}

#[test]
fn zsh_zdotdir_de_l_utilisateur() {
    let banc = Banc::nouveau();
    let zd = banc.home.path().join("mes-zsh");
    std::fs::create_dir_all(&zd).unwrap();
    std::fs::write(zd.join(".zshenv"), "VENV=1\n").unwrap();
    std::fs::write(zd.join(".zshrc"), "VU=1\nPROMPT='MONINVITE> '\n").unwrap();
    for mode in modes() {
        let zds = zd.to_string_lossy().into_owned();
        let Some(s) = Session::lancer_avec(&banc, "zsh", mode, |env| {
            env.retain(|(k, _)| k != "PUPITRE_ZDOTDIR_ORIGINE");
            env.push(("PUPITRE_ZDOTDIR_ORIGINE".into(), zds));
        }) else {
            return;
        };
        s.attendre_marques(2);
        s.envoyer("echo R=$VU$VENV Z=$ZDOTDIR\n");
        s.attendre_texte("R=11");
        // ZDOTDIR rendu à sa valeur d'origine pour les processus lancés ensuite.
        s.attendre_texte(&format!("Z={}", zd.display()));
        s.fermer();
    }
}

#[test]
fn zsh_connexion_charge_zprofile_et_zlogin() {
    // Mode login : .zprofile et .zlogin de l'utilisateur sont aussi relayés.
    let banc = Banc::nouveau();
    banc.ecrire(".zprofile", "VP=1\n");
    banc.ecrire(".zlogin", "VL=1\n");
    banc.ecrire(".zshrc", "PROMPT='MONINVITE> '\n");
    let Some(chemin) = dans_path("zsh") else {
        sauter("zsh n'est pas installé");
        return;
    };
    let r = Ressources {
        integration: Some(dossier_integration()),
        starship: None,
        config_starship: banc.config_starship.clone(),
    };
    let l = lancement(&chemin, ModeInvite::Shell, &r);
    assert_eq!(l.avertissement, None);
    let pty = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 200,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut cmd = CommandBuilder::new(&l.argv[0]);
    cmd.args(["-l", "-i"]);
    cmd.env_clear();
    cmd.env("PATH", std::env::var_os("PATH").unwrap_or_default());
    cmd.env("HOME", banc.home.path());
    cmd.env("TERM", "xterm-256color");
    for (k, v) in &l.env {
        if k != "PUPITRE_ZDOTDIR_ORIGINE" {
            cmd.env(k, v);
        }
    }
    cmd.env("PUPITRE_ZDOTDIR_ORIGINE", "");
    cmd.cwd(banc.home.path());
    let mut enfant = pty.slave.spawn_command(cmd).unwrap();
    drop(pty.slave);
    let mut ecr = pty.master.take_writer().unwrap();
    let mut lec = pty.master.try_clone_reader().unwrap();
    let tampon = Arc::new(Mutex::new(Vec::new()));
    let t2 = Arc::clone(&tampon);
    std::thread::spawn(move || {
        let mut b = [0u8; 4096];
        while let Ok(n) = lec.read(&mut b) {
            if n == 0 {
                break;
            }
            t2.lock().unwrap().extend_from_slice(&b[..n]);
        }
    });
    let fin = Instant::now() + DELAI;
    let attendre = |motif: &str| {
        while Instant::now() < fin {
            if String::from_utf8_lossy(&tampon.lock().unwrap()).contains(motif) {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!(
            "délai : {motif} ; flux {:?}",
            String::from_utf8_lossy(&tampon.lock().unwrap())
        );
    };
    attendre("MONINVITE> ");
    ecr.write_all(b"echo R=$VP$VL\n").unwrap();
    attendre("R=11");
    let _ = enfant.kill();
}

// ---------------------------------------------------------------------------------------
// Aucune écriture dans HOME
// ---------------------------------------------------------------------------------------

/// Fichiers d'historique ou de cache propres au shell, que le shell écrit de lui-même
/// (sans rapport avec nos fichiers) : exclus de la comparaison.
fn exclu(relatif: &str) -> bool {
    let chemin = relatif.trim_end_matches('/');
    let nom = chemin.rsplit('/').next().unwrap_or(chemin);
    const DOSSIERS: [&str; 5] = [
        ".cache/fish",
        ".cache/starship",
        ".local/share/fish",
        ".local/share/elvish",
        ".local/state/elvish",
    ];
    matches!(
        nom,
        ".bash_history"
            | ".zsh_history"
            | ".zcompdump"
            | "fish_history"
            | "fish_variables"
            | "history.txt"
            | "history.sqlite3"
            | "history.sqlite3-shm"
            | "history.sqlite3-wal"
    ) || DOSSIERS
        .iter()
        .any(|d| chemin == *d || chemin.starts_with(&format!("{d}/")))
}

fn instantane(racine: &Path) -> BTreeMap<String, Vec<u8>> {
    fn parcourir(racine: &Path, d: &Path, res: &mut BTreeMap<String, Vec<u8>>) {
        let Ok(entrees) = std::fs::read_dir(d) else {
            return;
        };
        for e in entrees.flatten() {
            let p = e.path();
            let rel = p
                .strip_prefix(racine)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            if exclu(&rel) {
                continue;
            }
            if p.is_dir() {
                // Seuls les fichiers comptent (fish crée lui-même des dossiers de config vides).
                parcourir(racine, &p, res);
            } else {
                res.insert(rel, std::fs::read(&p).unwrap_or_default());
            }
        }
    }
    let mut res = BTreeMap::new();
    parcourir(racine, racine, &mut res);
    res
}

#[test]
fn aucune_ecriture_dans_home() {
    for shell in ["bash", "zsh", "fish", "nu", "elvish"] {
        for mode in modes() {
            let banc = Banc::nouveau();
            banc.config_utilisateur(shell);
            let avant = instantane(banc.home.path());
            let Some(s) = Session::lancer(&banc, shell, mode) else {
                break;
            };
            let sx = syntaxe(shell);
            s.attendre_marques(if shell == "elvish" { 1 } else { 2 });
            s.envoyer(&format!("{}\n", sx.ok));
            s.attendre_marques(if shell == "elvish" { 4 } else { 6 });
            s.envoyer("cd /tmp\n");
            s.attendre("OSC 7 avec /tmp", |f| {
                dossiers(f).iter().any(|d| d == "/tmp")
            });
            s.fermer();
            let apres = instantane(banc.home.path());
            let ajoutes: Vec<_> = apres.keys().filter(|k| !avant.contains_key(*k)).collect();
            let perdus: Vec<_> = avant.keys().filter(|k| !apres.contains_key(*k)).collect();
            let modifies: Vec<_> = avant
                .iter()
                .filter(|(k, v)| apres.get(*k).is_some_and(|a| a != *v))
                .map(|(k, _)| k)
                .collect();
            assert!(
                ajoutes.is_empty() && perdus.is_empty() && modifies.is_empty(),
                "{shell} {mode:?} : ajoutés {ajoutes:?}, disparus {perdus:?}, modifiés {modifies:?}"
            );
        }
    }
}

// ---------------------------------------------------------------------------------------
// Robustesse (tour de correction 1)
// ---------------------------------------------------------------------------------------

fn copier_dossier(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap().flatten() {
        let d = dst.join(e.file_name());
        if e.path().is_dir() {
            copier_dossier(&e.path(), &d);
        } else {
            std::fs::copy(e.path(), d).unwrap();
        }
    }
}

#[test]
fn chemin_d_integration_avec_espace_et_apostrophe() {
    let racine = tempfile::tempdir().unwrap();
    let copie = racine.path().join("Mes Documents d'été \"x\"/assets shell");
    copier_dossier(&dossier_integration(), &copie);
    for shell in ["fish", "nu", "bash", "zsh"] {
        let mut banc = Banc::nouveau();
        banc.integration = copie.clone();
        banc.config_utilisateur(shell);
        let Some(s) = Session::lancer(&banc, shell, ModeInvite::Shell) else {
            continue;
        };
        s.attendre_marques(2);
        s.envoyer(&format!("{}\n", syntaxe(shell).echo_vu));
        s.attendre_texte("R=1");
        s.stabiliser();
        let m = normalisees(&s.flux());
        assert!(
            m.contains(&"A".to_string()) && m.contains(&"B".to_string()),
            "{shell} : {m:?}"
        );
        assert!(s.texte().contains("MONINVITE> "), "{shell}");
        s.fermer();
    }
}

#[test]
fn elvish_rc_utilisateur_en_erreur_marques_quand_meme() {
    let banc = Banc::nouveau();
    banc.ecrire(
        ".config/elvish/rc.elv",
        "set-env VU 1\nfail 'rc cassé'\nset-env APRES 1\n",
    );
    for mode in modes() {
        let Some(s) = Session::lancer(&banc, "elvish", mode) else {
            return;
        };
        s.attendre_marques(1);
        s.envoyer("true\n");
        s.attendre_marques(4);
        s.stabiliser();
        let m: Vec<String> = normalisees(&s.flux())
            .iter()
            .map(|x| x.split(';').next().unwrap_or("").to_string())
            .collect();
        assert_eq!(m, ["A", "C", "D", "A"], "{mode:?} : {:?}", s.texte());
        s.fermer();
    }
}

#[test]
fn elvish_rc_de_repli_dot_elvish() {
    let banc = Banc::nouveau();
    banc.ecrire(".elvish/rc.elv", "set-env VU 1\n");
    let Some(s) = Session::lancer(&banc, "elvish", ModeInvite::Shell) else {
        return;
    };
    s.attendre_marques(1);
    s.envoyer("echo R=$E:VU\n");
    s.attendre_texte("R=1");
    s.fermer();
}

#[test]
fn starship_non_executable_garde_l_invite_de_l_utilisateur() {
    use std::os::unix::fs::PermissionsExt;
    if dans_path("starship").is_none() {
        sauter("Starship n'est pas installé");
        return;
    }
    for shell in ["bash", "zsh", "fish", "nu", "elvish"] {
        let banc = Banc::nouveau();
        banc.config_utilisateur(shell);
        let faux = banc._exec.path().join("faux-starship");
        std::fs::write(&faux, "#!/bin/sh\nexit 1\n").unwrap();
        std::fs::set_permissions(&faux, std::fs::Permissions::from_mode(0o644)).unwrap();
        let faux_txt = faux.to_string_lossy().into_owned();
        let Some(s) = Session::lancer_avec(&banc, shell, ModeInvite::Starship, |env| {
            env.retain(|(k, _)| k != "PUPITRE_STARSHIP");
            env.push(("PUPITRE_STARSHIP".into(), faux_txt));
        }) else {
            return;
        };
        let debut = if shell == "elvish" { 1 } else { 2 };
        s.attendre_marques(debut);
        s.envoyer(&format!("{}\n", syntaxe(shell).ok));
        s.attendre_marques(if shell == "elvish" { 4 } else { 6 });
        s.stabiliser();
        let texte = s.texte();
        assert!(texte.contains("MONINVITE> "), "{shell} : {texte:?}");
        let minuscules = texte.to_lowercase();
        assert!(
            !minuscules.contains("permission denied") && !minuscules.contains("error"),
            "{shell} : erreur affichée : {texte:?}"
        );
        s.fermer();
    }
}

#[test]
fn starship_absent_invite_utilisateur() {
    for shell in ["bash", "zsh"] {
        let Some(chemin) = dans_path(shell) else {
            sauter(&format!("{shell} n'est pas installé"));
            continue;
        };
        let banc = Banc::nouveau();
        banc.config_utilisateur(shell);
        // PATH sans Starship : seul le shell testé y figure (lien dans un dossier à part).
        let bin = banc._exec.path().join("bin-sans-starship");
        std::fs::create_dir_all(&bin).unwrap();
        std::os::unix::fs::symlink(&chemin, bin.join(shell)).unwrap();
        let bin_txt = bin.to_string_lossy().into_owned();
        let (s, l) = Session::lancer_ressources(&banc, shell, ModeInvite::Starship, None, |env| {
            env.retain(|(k, _)| k != "PATH");
            env.push(("PATH".into(), bin_txt));
        })
        .unwrap();
        // Un seul avertissement, produit par `lancement` ; l'invite habituelle est gardée.
        let avertissement = l.avertissement.as_deref().unwrap_or("");
        assert!(
            avertissement.contains("Starship"),
            "{shell} : {avertissement:?}"
        );
        assert!(l.env.contains(&("PUPITRE_INVITE".into(), "shell".into())));
        assert!(!l
            .env
            .iter()
            .any(|(k, _)| k == "PUPITRE_STARSHIP" || k == "STARSHIP_CONFIG"));
        s.attendre_marques(2);
        s.envoyer(&format!("{}\n", syntaxe(shell).ok));
        s.attendre_marques(6);
        s.stabiliser();
        let flux = s.flux();
        let texte = s.texte();
        assert!(texte.contains("MONINVITE> "), "{shell} : {texte:?}");
        assert!(!texte.contains("STAR> "), "{shell} : {texte:?}");
        let m = normalisees(&flux);
        assert_eq!(compte(&m, "A"), 2, "{shell} : {m:?}");
        assert_eq!(compte(&m, "B"), 2, "{shell} : {m:?}");
        assert_eq!(compte(&m, "C"), 1, "{shell} : {m:?}");
        // Les fichiers de shell n'affichent rien eux-mêmes : pas d'avertissement dans le flux.
        let minuscules = texte.to_lowercase();
        assert!(
            !minuscules.contains("starship") && !minuscules.contains("not found"),
            "{shell} : texte inattendu : {texte:?}"
        );
        s.fermer();
    }
}
