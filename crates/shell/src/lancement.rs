// SPDX-License-Identifier: GPL-3.0-or-later
//! Construction du lancement d'un shell (arguments et variables).

use crate::detection::{niveau_de, nom_fichier, Niveau};
use crate::installation::{commande_installation, distribution, Distribution};
use pupitre_i18n::{t, ta, ArgI18n};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeInvite {
    Starship,
    Shell,
}

pub struct Ressources {
    /// Dossier `assets/shell` (fichiers d'intégration par shell).
    pub integration: Option<PathBuf>,
    pub starship: Option<PathBuf>,
    pub config_starship: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lancement {
    pub argv: Vec<String>,
    pub env: Vec<(String, String)>,
    pub avertissement: Option<String>,
}

fn texte(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// Chaîne fish entre guillemets simples (seuls `\\` et `\'` y sont spéciaux).
fn guillemets_fish(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// Chaîne nushell entre guillemets doubles (`\\` et `\"` échappés) : seule forme qui admet
/// n'importe quel caractère, et qui reste une constante lisible par `source`.
fn guillemets_nushell(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Lancement de `shell`. Ne vérifie que l'existence des fichiers d'intégration ; le repli
/// quand le shell lui-même est absent revient à l'appelant.
pub fn lancement(shell: &Path, mode: ModeInvite, r: &Ressources) -> Lancement {
    let os = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
    lancement_pour(shell, mode, r, distribution(&os))
}

fn tel_quel(shell: &Path, avertissement: Option<String>) -> Lancement {
    Lancement {
        argv: vec![texte(shell)],
        env: Vec::new(),
        avertissement,
    }
}

pub(crate) fn lancement_pour(
    shell: &Path,
    mode: ModeInvite,
    r: &Ressources,
    d: Distribution,
) -> Lancement {
    let nom = nom_fichier(shell);
    // Fichier d'intégration attendu (relatif au dossier d'intégration), s'il existe pour ce shell.
    let relatif = match nom.as_str() {
        "bash" => "bash/pupitre.bash",
        "zsh" => "zsh/.zshrc",
        "fish" => "fish/pupitre.fish",
        "nu" => "nushell/pupitre.nu",
        "elvish" => "elvish/pupitre.elv",
        // xonsh, tcsh, ion, pwsh : mécanisme non validé ; autres : rien à faire.
        _ => return tel_quel(shell, None),
    };
    if niveau_de(shell) == Niveau::TelQuel {
        return tel_quel(shell, None);
    }
    let Some(dossier) = r.integration.as_deref() else {
        return tel_quel(
            shell,
            Some(ta(
                "shell-integration-absente",
                &[("shell", ArgI18n::from(nom))],
            )),
        );
    };
    let fichier = dossier.join(relatif);
    if !fichier.is_file() {
        return tel_quel(
            shell,
            Some(ta(
                "shell-integration-absente",
                &[("shell", ArgI18n::from(nom))],
            )),
        );
    }

    let mut argv = vec![texte(shell)];
    let mut env: Vec<(String, String)> = Vec::new();
    match nom.as_str() {
        "bash" => argv.extend(["--rcfile".into(), texte(&fichier), "-i".into()]),
        "zsh" => {
            env.push(("ZDOTDIR".into(), texte(&dossier.join("zsh"))));
            env.push((
                "PUPITRE_ZDOTDIR_ORIGINE".into(),
                std::env::var("ZDOTDIR").unwrap_or_default(),
            ));
        }
        "fish" => argv.extend([
            "-C".into(),
            format!("source {}", guillemets_fish(&texte(&fichier))),
        ]),
        "nu" => argv.extend([
            "-e".into(),
            format!("source {}", guillemets_nushell(&texte(&fichier))),
        ]),
        _ => argv.extend(["-rc".into(), texte(&fichier)]),
    }

    env.push(("PUPITRE_INTEGRATION".into(), texte(dossier)));
    let mut avertissement = None;
    let starship = if mode == ModeInvite::Starship {
        match &r.starship {
            Some(s) => Some(s),
            None => {
                avertissement = Some(match commande_installation("starship", d) {
                    Some(c) => ta(
                        "shell-starship-absent-installer",
                        &[("commande", ArgI18n::from(c))],
                    ),
                    None => t("shell-starship-absent"),
                });
                None
            }
        }
    } else {
        None
    };
    if let Some(s) = &r.starship {
        env.push(("PUPITRE_STARSHIP".into(), texte(s)));
    }
    if starship.is_some() {
        env.push(("PUPITRE_INVITE".into(), "starship".into()));
        env.push(("STARSHIP_CONFIG".into(), texte(&r.config_starship)));
    } else {
        env.push(("PUPITRE_INVITE".into(), "shell".into()));
    }
    Lancement {
        argv,
        env,
        avertissement,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn faux_dossier() -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        for f in ["bash/pupitre.bash", "zsh/.zshrc", "fish/pupitre.fish"] {
            let p = t.path().join(f);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, "").unwrap();
        }
        t
    }

    fn res(t: &tempfile::TempDir, starship: bool) -> Ressources {
        Ressources {
            integration: Some(t.path().to_path_buf()),
            starship: starship.then(|| PathBuf::from("/usr/bin/starship")),
            config_starship: PathBuf::from("/cfg/starship.toml"),
        }
    }

    fn var<'a>(l: &'a Lancement, k: &str) -> Option<&'a str> {
        l.env.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }

    #[test]
    fn lancement_bash_complet() {
        let t = faux_dossier();
        let l = lancement_pour(
            Path::new("/bin/bash"),
            ModeInvite::Starship,
            &res(&t, true),
            Distribution::Debian,
        );
        assert_eq!(l.argv[0], "/bin/bash");
        let i = l.argv.iter().position(|a| a == "--rcfile").unwrap();
        assert_eq!(
            l.argv[i + 1],
            t.path().join("bash/pupitre.bash").to_string_lossy()
        );
        assert_eq!(var(&l, "PUPITRE_INVITE"), Some("starship"));
        assert_eq!(var(&l, "STARSHIP_CONFIG"), Some("/cfg/starship.toml"));
        assert_eq!(var(&l, "PUPITRE_STARSHIP"), Some("/usr/bin/starship"));
        assert!(var(&l, "PUPITRE_INTEGRATION").is_some());
        assert_eq!(l.avertissement, None);
    }

    #[test]
    fn echappement_des_chemins() {
        assert_eq!(guillemets_fish("/a b/c"), "'/a b/c'");
        assert_eq!(guillemets_fish("/l'ami\\x"), "'/l\\'ami\\\\x'");
        assert_eq!(guillemets_nushell("/a b/c"), "\"/a b/c\"");
        assert_eq!(guillemets_nushell("/a\"b\\c"), "\"/a\\\"b\\\\c\"");
    }

    #[test]
    fn lancement_mode_shell() {
        let t = faux_dossier();
        let l = lancement_pour(
            Path::new("/bin/bash"),
            ModeInvite::Shell,
            &res(&t, true),
            Distribution::Debian,
        );
        assert_eq!(var(&l, "PUPITRE_INVITE"), Some("shell"));
        assert_eq!(var(&l, "STARSHIP_CONFIG"), None);
        assert_eq!(l.avertissement, None);
    }

    #[test]
    fn lancement_zsh_et_fish() {
        let t = faux_dossier();
        let z = lancement_pour(
            Path::new("/bin/zsh"),
            ModeInvite::Shell,
            &res(&t, false),
            Distribution::Debian,
        );
        assert_eq!(z.argv, ["/bin/zsh"]);
        assert_eq!(
            var(&z, "ZDOTDIR"),
            Some(t.path().join("zsh").to_str().unwrap())
        );
        assert!(var(&z, "PUPITRE_ZDOTDIR_ORIGINE").is_some());
        let f = lancement_pour(
            Path::new("/bin/fish"),
            ModeInvite::Shell,
            &res(&t, false),
            Distribution::Debian,
        );
        assert_eq!(f.argv[1], "-C");
        assert!(f.argv[2].starts_with("source "));
    }

    #[test]
    fn lancement_tel_quel() {
        let t = faux_dossier();
        for s in ["/bin/dash", "/usr/bin/pwsh"] {
            let l = lancement_pour(
                Path::new(s),
                ModeInvite::Starship,
                &res(&t, true),
                Distribution::Debian,
            );
            assert_eq!(l.argv, [s]);
            assert!(l.env.is_empty());
        }
    }

    #[test]
    fn integration_absente_tel_quel_avec_avertissement() {
        let t = tempfile::tempdir().unwrap();
        let l = lancement_pour(
            Path::new("/bin/bash"),
            ModeInvite::Starship,
            &res(&t, true),
            Distribution::Debian,
        );
        assert_eq!(l.argv, ["/bin/bash"]);
        assert!(l.env.is_empty());
        assert!(l.avertissement.is_some());
        let mut r = res(&t, true);
        r.integration = None;
        let l = lancement_pour(
            Path::new("/bin/bash"),
            ModeInvite::Starship,
            &r,
            Distribution::Debian,
        );
        assert!(l.avertissement.is_some());
    }

    #[test]
    fn starship_absent_mode_shell_force() {
        let t = faux_dossier();
        let l = lancement_pour(
            Path::new("/bin/bash"),
            ModeInvite::Starship,
            &res(&t, false),
            Distribution::Debian,
        );
        assert_eq!(var(&l, "PUPITRE_INVITE"), Some("shell"));
        assert_eq!(var(&l, "STARSHIP_CONFIG"), None);
        assert_eq!(
            l.avertissement.as_deref(),
            Some("Starship introuvable : l'invite habituelle du shell est gardée. Pour l'installer : sudo apt install starship")
        );
        let l = lancement_pour(
            Path::new("/bin/bash"),
            ModeInvite::Starship,
            &res(&t, false),
            Distribution::Autre,
        );
        assert_eq!(
            l.avertissement.as_deref(),
            Some("Starship introuvable : l'invite habituelle du shell est gardée.")
        );
    }
}
