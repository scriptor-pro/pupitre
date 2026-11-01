// SPDX-License-Identifier: GPL-3.0-or-later
//! Shells connus, installés, et niveau de prise en charge.

use crate::installation::Distribution as D;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Prise en charge d'un shell par Pupitre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Niveau {
    /// Intégration complète (marques de commande, titre, etc.).
    Complet,
    /// L'invite est gérée par Pupitre, sans le reste de l'intégration.
    InviteGeree,
    /// Lancé sans rien changer.
    TelQuel,
}

pub struct ShellConnu {
    pub nom: &'static str,
    pub executable: &'static str,
    pub niveau: Niveau,
    pub paquets: &'static [(D, &'static str)],
}

pub const SHELLS_CONNUS: &[ShellConnu] = &[
    ShellConnu {
        nom: "bash",
        executable: "bash",
        niveau: Niveau::Complet,
        paquets: &[
            (D::Debian, "bash"),
            (D::Fedora, "bash"),
            (D::Arch, "bash"),
            (D::OpenSuse, "bash"),
            (D::Alpine, "bash"),
        ],
    },
    ShellConnu {
        nom: "zsh",
        executable: "zsh",
        niveau: Niveau::Complet,
        paquets: &[
            (D::Debian, "zsh"),
            (D::Fedora, "zsh"),
            (D::Arch, "zsh"),
            (D::OpenSuse, "zsh"),
            (D::Alpine, "zsh"),
        ],
    },
    ShellConnu {
        nom: "fish",
        executable: "fish",
        niveau: Niveau::Complet,
        paquets: &[
            (D::Debian, "fish"),
            (D::Fedora, "fish"),
            (D::Arch, "fish"),
            (D::OpenSuse, "fish"),
            (D::Alpine, "fish"),
        ],
    },
    ShellConnu {
        nom: "nushell",
        executable: "nu",
        niveau: Niveau::Complet,
        paquets: &[
            (D::Debian, "nushell"),
            (D::Fedora, "nushell"),
            (D::Arch, "nushell"),
            (D::OpenSuse, "nushell"),
            (D::Alpine, "nushell"),
        ],
    },
    ShellConnu {
        nom: "elvish",
        executable: "elvish",
        niveau: Niveau::InviteGeree,
        paquets: &[
            (D::Debian, "elvish"),
            (D::Fedora, "elvish"),
            (D::Arch, "elvish"),
            (D::Alpine, "elvish"),
        ],
    },
    ShellConnu {
        nom: "xonsh",
        executable: "xonsh",
        niveau: Niveau::InviteGeree,
        paquets: &[
            (D::Debian, "xonsh"),
            (D::Fedora, "xonsh"),
            (D::Arch, "xonsh"),
        ],
    },
    ShellConnu {
        nom: "tcsh",
        executable: "tcsh",
        niveau: Niveau::InviteGeree,
        paquets: &[
            (D::Debian, "tcsh"),
            (D::Fedora, "tcsh"),
            (D::Arch, "tcsh"),
            (D::OpenSuse, "tcsh"),
            (D::Alpine, "tcsh"),
        ],
    },
    ShellConnu {
        nom: "ion",
        executable: "ion",
        niveau: Niveau::InviteGeree,
        paquets: &[],
    },
    ShellConnu {
        nom: "pwsh",
        executable: "pwsh",
        niveau: Niveau::InviteGeree,
        paquets: &[],
    },
];

/// Nom de fichier d'un shell, sans le `-` des shells de connexion.
pub(crate) fn nom_fichier(chemin: &Path) -> String {
    let nom = chemin
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    nom.trim_start_matches('-').to_string()
}

/// Niveau de prise en charge, d'après le nom du fichier.
pub fn niveau_de(chemin: &Path) -> Niveau {
    let nom = nom_fichier(chemin);
    SHELLS_CONNUS
        .iter()
        .find(|s| s.executable == nom)
        .map_or(Niveau::TelQuel, |s| s.niveau)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellInstalle {
    pub chemin: PathBuf,
    pub nom: String,
    pub niveau: Niveau,
}

/// Shells présents : ceux de `/etc/shells` (texte), puis les shells connus trouvés dans `PATH`.
/// Dédoublonnés par chemin réel ; les fichiers absents sont ignorés.
pub fn shells_installes(etc_shells: &str, path: &OsStr) -> Vec<ShellInstalle> {
    let mut candidats: Vec<PathBuf> = etc_shells
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with('/'))
        .map(PathBuf::from)
        .collect();
    for dossier in std::env::split_paths(path) {
        for s in SHELLS_CONNUS {
            candidats.push(dossier.join(s.executable));
        }
    }
    let mut vus: Vec<PathBuf> = Vec::new();
    let mut res = Vec::new();
    for c in candidats {
        let Ok(reel) = c.canonicalize() else { continue };
        if !reel.is_file() || vus.contains(&reel) {
            continue;
        }
        vus.push(reel);
        res.push(ShellInstalle {
            nom: nom_fichier(&c),
            niveau: niveau_de(&c),
            chemin: c,
        });
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn detecte_etc_shells_et_path() {
        let t = tempfile::tempdir().unwrap();
        let (a, b) = (t.path().join("bin"), t.path().join("usr-bin"));
        fs::create_dir(&a).unwrap();
        fs::create_dir(&b).unwrap();
        fs::write(a.join("bash"), "").unwrap();
        std::os::unix::fs::symlink(a.join("bash"), b.join("bash")).unwrap();
        fs::write(b.join("fish"), "").unwrap();
        let etc = format!(
            "# commentaire\n{}\n{}\n/n/existe/pas\n",
            a.join("bash").display(),
            b.join("bash").display()
        );
        let path = std::env::join_paths([&a, &b]).unwrap();
        let v = shells_installes(&etc, &path);
        let noms: Vec<&str> = v.iter().map(|s| s.nom.as_str()).collect();
        assert_eq!(noms, ["bash", "fish"]);
        assert_eq!(v[0].chemin, a.join("bash"));
        assert_eq!(v[1].niveau, Niveau::Complet);
    }

    #[test]
    fn niveaux_conformes_a_la_conception() {
        for n in ["bash", "zsh", "fish", "nu", "-bash"] {
            assert_eq!(
                niveau_de(Path::new(&format!("/usr/bin/{n}"))),
                Niveau::Complet,
                "{n}"
            );
        }
        for n in ["elvish", "xonsh", "tcsh", "ion", "pwsh"] {
            assert_eq!(
                niveau_de(Path::new(&format!("/usr/bin/{n}"))),
                Niveau::InviteGeree,
                "{n}"
            );
        }
        assert_eq!(niveau_de(Path::new("/bin/dash")), Niveau::TelQuel);
    }
}
