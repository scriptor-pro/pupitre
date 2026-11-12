// SPDX-License-Identifier: GPL-3.0-or-later
//! Choix du shell d'un onglet : réglages de `pupitre.toml`, shells installés, lancement avec
//! l'intégration de Pupitre, et sous-commande `pupitre shells`. Sans GTK.

use pupitre_config::{chemin_config, Config, Invite};
use pupitre_fond::session::login_shell;
use pupitre_i18n::{t, ta, ArgI18n};
use pupitre_shell::{
    chercher_starship, commande_installation, creer_config_starship_si_absente, distribution,
    lancement, shells_installes, Distribution, Lancement, ModeInvite, Niveau, Ressources,
    ShellInstalle, SHELLS_CONNUS,
};
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Programmes listés dans `/etc/shells` qui ne sont pas des shells utilisables.
const FAUX_SHELLS: &[&str] = &[
    "nologin",
    "false",
    "true",
    "git-shell",
    "sync",
    "shutdown",
    "halt",
];

/// Ce que l'on ouvre dans un nouvel onglet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choix {
    /// Le shell des réglages (`shell`), sinon le shell de connexion.
    Defaut,
    /// Un shell installé choisi dans le menu, lancé avec l'intégration de Pupitre.
    Installe(PathBuf),
    /// Une commande déclarée dans `[[shells]]`, lancée telle quelle.
    Declare(Vec<String>),
    /// Une commande quelconque (connexion SSH), lancée telle quelle.
    Commande(Vec<String>),
}

/// Dossier des fichiers d'intégration : `$PUPITRE_ASSETS/shell`, sinon `/usr/share/pupitre/shell`
/// (paquet installé), sinon `assets/shell` des sources.
pub fn dossier_integration() -> Option<PathBuf> {
    let candidats = [
        std::env::var_os("PUPITRE_ASSETS")
            .filter(|v| !v.is_empty())
            .map(|v| PathBuf::from(v).join("shell")),
        Some(PathBuf::from("/usr/share/pupitre/shell")),
        Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/shell")),
    ];
    candidats.into_iter().flatten().find(|d| d.is_dir())
}

/// Fichier de l'invite Starship : `starship.toml` à côté de `pupitre.toml`.
pub fn chemin_starship() -> PathBuf {
    chemin_config().with_file_name("starship.toml")
}

/// Ressources de lancement pour le `PATH` donné.
pub fn ressources(path: &OsStr) -> Ressources {
    Ressources {
        integration: dossier_integration(),
        starship: chercher_starship(std::env::var_os("PUPITRE_STARSHIP").as_deref(), path),
        config_starship: chemin_starship(),
        cache_starship: pupitre_config::dossier_etat().join("starship"),
    }
}

fn est_executable(p: &Path) -> bool {
    std::fs::metadata(p)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// Résout le réglage `shell` : un chemin (contenant `/`) doit désigner un exécutable ; un nom
/// est cherché dans les dossiers de `path`.
pub fn trouver_shell(shell: &str, path: &OsStr) -> Option<PathBuf> {
    if shell.contains('/') {
        let p = PathBuf::from(shell);
        return est_executable(&p).then_some(p);
    }
    std::env::split_paths(path)
        .filter(|d| !d.as_os_str().is_empty())
        .map(|d| d.join(shell))
        .find(|p| est_executable(p))
}

fn shell_de_connexion() -> PathBuf {
    login_shell()
        .into_iter()
        .next()
        .map_or_else(|| PathBuf::from("/bin/sh"), PathBuf::from)
}

fn distribution_courante() -> Distribution {
    distribution(&std::fs::read_to_string("/etc/os-release").unwrap_or_default())
}

fn joindre(a: Option<String>, b: Option<String>) -> Option<String> {
    match (a, b) {
        (Some(a), Some(b)) => Some(format!("{a} {b}")),
        (a, b) => a.or(b),
    }
}

/// Mode d'invite effectif : Starship demandé mais fichier d'invite impossible à créer =
/// invite du shell gardée, avec un avertissement.
fn mode_invite(config: &Config, r: &Ressources) -> (ModeInvite, Option<String>) {
    if config.invite == Invite::Shell {
        return (ModeInvite::Shell, None);
    }
    match creer_config_starship_si_absente(&r.config_starship) {
        Ok(()) => (ModeInvite::Starship, None),
        Err(e) => (
            ModeInvite::Shell,
            Some(ta(
                "shell-starship-config-impossible",
                &[
                    (
                        "chemin",
                        ArgI18n::from(r.config_starship.display().to_string()),
                    ),
                    ("erreur", ArgI18n::from(e.to_string())),
                ],
            )),
        ),
    }
}

/// Tout ce qu'il faut pour lancer `choix` : arguments, variables et avertissement éventuel.
/// Ne plante jamais : un shell introuvable donne le shell de connexion et un avertissement.
pub fn preparer(choix: &Choix, config: &Config, path: &OsStr) -> Lancement {
    let commande = |argv: &Vec<String>| Lancement {
        argv: argv.clone(),
        env: Vec::new(),
        avertissement: None,
    };
    let r = ressources(path);
    match choix {
        Choix::Declare(argv) | Choix::Commande(argv) => commande(argv),
        Choix::Installe(shell) => {
            let (mode, avert) = mode_invite(config, &r);
            let mut l = lancement(shell, mode, &r);
            l.avertissement = joindre(avert, l.avertissement);
            l
        }
        Choix::Defaut => {
            let (mode, avert) = mode_invite(config, &r);
            let (shell, absent) = if config.shell.trim().is_empty() {
                (shell_de_connexion(), None)
            } else {
                match trouver_shell(config.shell.trim(), path) {
                    Some(s) => (s, None),
                    None => (shell_de_connexion(), Some(shell_absent(&config.shell))),
                }
            };
            let mut l = lancement(&shell, mode, &r);
            l.avertissement = joindre(absent, joindre(avert, l.avertissement));
            l
        }
    }
}

/// Avertissement « shell réglé mais introuvable », avec la commande d'installation si on la
/// connaît.
fn shell_absent(shell: &str) -> String {
    let nom = shell.rsplit('/').next().unwrap_or(shell);
    let args = [("shell", ArgI18n::from(shell))];
    match commande_installation(nom, distribution_courante()) {
        Some(c) => ta(
            "shell-configure-absent-installer",
            &[args[0].clone(), ("commande", ArgI18n::from(c))],
        ),
        None => ta("shell-configure-absent", &args),
    }
}

/// Vrai pour les entrées de `/etc/shells` qui ne sont pas de vrais shells.
fn est_faux_shell(s: &ShellInstalle) -> bool {
    FAUX_SHELLS.contains(&s.nom.as_str())
}

/// Shells installés qu'on propose (sans `nologin` & co), pour le `PATH` donné.
pub fn shells_proposes(path: &OsStr) -> Vec<ShellInstalle> {
    // `PUPITRE_ETC_SHELLS` : variable d'essai, remplace `/etc/shells`.
    let fichier = std::env::var_os("PUPITRE_ETC_SHELLS")
        .filter(|v| !v.is_empty())
        .map_or_else(|| PathBuf::from("/etc/shells"), PathBuf::from);
    let etc = std::fs::read_to_string(fichier).unwrap_or_default();
    shells_installes(&etc, path)
        .into_iter()
        .filter(|s| !est_faux_shell(s))
        .collect()
}

/// Valeur d'action du menu pour un shell installé.
pub const PREFIXE_INSTALLE: &str = "chemin:";
/// Valeur d'action du menu pour un shell déclaré (suivi de son nom).
pub const PREFIXE_DECLARE: &str = "declare:";

/// Entrées du sous-menu « Nouvel onglet avec » : `(libellé, valeur de l'action)`. Les shells
/// installés d'abord, puis ceux déclarés dans `[[shells]]`.
pub fn entrees_menu(config: &Config, path: &OsStr) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = shells_proposes(path)
        .into_iter()
        .map(|s| {
            (
                s.nom.clone(),
                format!("{PREFIXE_INSTALLE}{}", s.chemin.display()),
            )
        })
        .collect();
    v.extend(
        config
            .shells
            .iter()
            .map(|d| (d.nom.clone(), format!("{PREFIXE_DECLARE}{}", d.nom))),
    );
    v
}

/// Retrouve le choix désigné par la valeur d'une action du menu ; `None` si le shell déclaré
/// n'existe plus (réglages modifiés depuis).
pub fn choix_de_cible(cible: &str, config: &Config) -> Option<Choix> {
    if let Some(chemin) = cible.strip_prefix(PREFIXE_INSTALLE) {
        return Some(Choix::Installe(PathBuf::from(chemin)));
    }
    let nom = cible.strip_prefix(PREFIXE_DECLARE)?;
    config
        .shells
        .iter()
        .find(|d| d.nom == nom)
        .map(|d| Choix::Declare(d.commande.clone()))
}

/// Texte du niveau de prise en charge.
fn libelle_niveau(n: Niveau) -> String {
    t(match n {
        Niveau::Complet => "shells-niveau-complet",
        Niveau::InviteGeree => "shells-niveau-invite",
        Niveau::TelQuel => "shells-niveau-tel-quel",
    })
}

/// `pupitre shells` : shells installés avec leur niveau, puis les shells connus absents avec la
/// commande d'installation. Code 0.
pub fn executer_shells(path: &OsStr, sortie: &mut impl Write) -> i32 {
    let installes = shells_proposes(path);
    let _ = writeln!(sortie, "{}", t("shells-installes"));
    if installes.is_empty() {
        let _ = writeln!(sortie, "  {}", t("shells-aucun"));
    }
    for s in &installes {
        let _ = writeln!(
            sortie,
            "  {}  {}",
            s.chemin.display(),
            ta(
                "shells-niveau",
                &[("niveau", ArgI18n::from(libelle_niveau(s.niveau)))]
            )
        );
    }
    let d = distribution_courante();
    let absents: Vec<_> = SHELLS_CONNUS
        .iter()
        .filter(|c| !installes.iter().any(|s| s.nom == c.executable))
        .collect();
    if !absents.is_empty() {
        let _ = writeln!(sortie, "\n{}", t("shells-absents"));
        for c in absents {
            let ligne = match commande_installation(c.executable, d) {
                Some(cmd) => ta(
                    "shells-absent-installer",
                    &[
                        ("nom", ArgI18n::from(c.nom)),
                        ("niveau", ArgI18n::from(libelle_niveau(c.niveau))),
                        ("commande", ArgI18n::from(cmd)),
                    ],
                ),
                None => ta(
                    "shells-absent",
                    &[
                        ("nom", ArgI18n::from(c.nom)),
                        ("niveau", ArgI18n::from(libelle_niveau(c.niveau))),
                    ],
                ),
            };
            let _ = writeln!(sortie, "  {ligne}");
        }
    }
    0
}

/// `PATH` courant (vide s'il est absent).
pub fn path_courant() -> OsString {
    std::env::var_os("PATH").unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faux_shells_filtres() {
        for n in ["nologin", "false", "git-shell"] {
            let s = ShellInstalle {
                chemin: PathBuf::from(format!("/usr/sbin/{n}")),
                nom: n.into(),
                niveau: Niveau::TelQuel,
            };
            assert!(est_faux_shell(&s), "{n}");
        }
    }

    #[test]
    fn trouver_shell_par_nom_et_chemin() {
        let path = OsString::from("/usr/bin:/bin");
        assert!(trouver_shell("sh", &path).is_some());
        assert!(trouver_shell("/bin/sh", &path).is_some());
        assert!(trouver_shell("nexistepas", &path).is_none());
        assert!(trouver_shell("/nexiste/pas", &path).is_none());
        assert!(trouver_shell("/etc", &path).is_none());
    }
}
