// SPDX-License-Identifier: GPL-3.0-or-later
//! Distribution et commande d'installation d'un paquet.

use crate::detection::SHELLS_CONNUS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Distribution {
    Debian,
    Fedora,
    Arch,
    OpenSuse,
    Alpine,
    Autre,
}

fn depuis_id(id: &str) -> Option<Distribution> {
    match id {
        "debian" | "ubuntu" | "linuxmint" | "raspbian" | "pop" => Some(Distribution::Debian),
        "fedora" | "rhel" | "centos" | "rocky" | "almalinux" => Some(Distribution::Fedora),
        "arch" | "manjaro" | "endeavouros" => Some(Distribution::Arch),
        "opensuse" | "opensuse-leap" | "opensuse-tumbleweed" | "suse" | "sles" => {
            Some(Distribution::OpenSuse)
        }
        "alpine" => Some(Distribution::Alpine),
        _ => None,
    }
}

fn valeur(os_release: &str, cle: &str) -> Option<String> {
    os_release.lines().find_map(|l| {
        let v = l.trim().strip_prefix(cle)?.strip_prefix('=')?;
        Some(
            v.trim()
                .trim_matches(|c| c == '"' || c == '\'')
                .to_lowercase(),
        )
    })
}

/// Distribution d'après le contenu de `/etc/os-release` (`ID`, puis chaque mot de `ID_LIKE`).
pub fn distribution(os_release: &str) -> Distribution {
    if let Some(d) = valeur(os_release, "ID").and_then(|id| depuis_id(&id)) {
        return d;
    }
    valeur(os_release, "ID_LIKE")
        .and_then(|l| l.split_whitespace().find_map(depuis_id))
        .unwrap_or(Distribution::Autre)
}

/// Paquets de Starship (pas un shell).
const PAQUETS_STARSHIP: &[(Distribution, &str)] = &[
    (Distribution::Debian, "starship"),
    (Distribution::Fedora, "starship"),
    (Distribution::Arch, "starship"),
    (Distribution::OpenSuse, "starship"),
    (Distribution::Alpine, "starship"),
];

/// Commande d'installation d'un shell (nom ou exécutable) ou de `starship` ; `None` si le
/// paquet n'est pas connu pour cette distribution.
pub fn commande_installation(shell: &str, d: Distribution) -> Option<String> {
    let paquets = if shell == "starship" {
        PAQUETS_STARSHIP
    } else {
        SHELLS_CONNUS
            .iter()
            .find(|s| s.executable == shell || s.nom == shell)?
            .paquets
    };
    let paquet = paquets.iter().find(|(x, _)| *x == d)?.1;
    let debut = match d {
        Distribution::Debian => "sudo apt install",
        Distribution::Fedora => "sudo dnf install",
        Distribution::Arch => "sudo pacman -S",
        Distribution::OpenSuse => "sudo zypper install",
        Distribution::Alpine => "sudo apk add",
        Distribution::Autre => return None,
    };
    Some(format!("{debut} {paquet}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distribution_par_id_like() {
        assert_eq!(
            distribution("ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n"),
            Distribution::Debian
        );
        assert_eq!(distribution("ID=fedora\n"), Distribution::Fedora);
        assert_eq!(distribution("ID=\"nixos\"\n"), Distribution::Autre);
    }

    #[test]
    fn commande_installation_debian() {
        assert_eq!(
            commande_installation("fish", Distribution::Debian).as_deref(),
            Some("sudo apt install fish")
        );
        assert_eq!(
            commande_installation("nu", Distribution::Debian).as_deref(),
            Some("sudo apt install nushell")
        );
        assert_eq!(
            commande_installation("starship", Distribution::Arch).as_deref(),
            Some("sudo pacman -S starship")
        );
    }

    #[test]
    fn commande_inconnue_none() {
        assert_eq!(commande_installation("pwsh", Distribution::Debian), None);
        assert_eq!(commande_installation("inconnu", Distribution::Debian), None);
        assert_eq!(commande_installation("fish", Distribution::Autre), None);
    }
}
