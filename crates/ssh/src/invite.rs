// SPDX-License-Identifier: GPL-3.0-or-later
//! Reconnaissance des invites d'OpenSSH reçues par l'askpass de Pupitre.

pub use pupitre_proto::ssh::Genre;

use std::path::PathBuf;

const CLE_SSH: &str = "Enter passphrase for key '";
const CLE_ADD: &str = "Enter passphrase for ";
const CLE_RATEE: &str = "Bad passphrase, try again for ";
const SUFFIXE_CONFIRM: &str = " (will confirm each use)";

/// Rend un chemin absolu : `~` et chemins relatifs partent du dossier personnel.
fn absolu(brut: &str) -> Option<PathBuf> {
    if brut.is_empty() {
        return None;
    }
    let home = std::env::home_dir();
    if brut == "~" {
        return home;
    }
    if let Some(reste) = brut.strip_prefix("~/") {
        return home.map(|h| h.join(reste));
    }
    let p = PathBuf::from(brut);
    if p.is_absolute() {
        Some(p)
    } else {
        home.map(|h| h.join(p))
    }
}

/// Extrait le chemin d'une invite de phrase de passe, si elle en est une.
fn phrase_cle(invite: &str) -> Option<Genre> {
    let sans_fin = invite.trim_end();
    if let Some(reste) = sans_fin.strip_prefix(CLE_SSH) {
        // Forme `key '<chemin>':` ; le chemin peut contenir des `'`.
        let chemin = reste.strip_suffix("':")?;
        return Some(Genre::PhraseCle {
            cle: absolu(chemin)?,
            nouvel_essai: false,
        });
    }
    let (reste, nouvel_essai) = match sans_fin.strip_prefix(CLE_RATEE) {
        Some(r) => (r, true),
        None => (sans_fin.strip_prefix(CLE_ADD)?, false),
    };
    let chemin = reste.strip_suffix(':')?;
    let chemin = chemin.strip_suffix(SUFFIXE_CONFIRM).unwrap_or(chemin);
    Some(Genre::PhraseCle {
        cle: absolu(chemin)?,
        nouvel_essai,
    })
}

/// Classe une invite ; `prompt_env` est la valeur de `SSH_ASKPASS_PROMPT`.
pub fn classer_invite(invite: &str, prompt_env: Option<&str>) -> Genre {
    match prompt_env {
        Some("confirm") => return Genre::Confirmation,
        Some("none") => return Genre::Information,
        _ => {}
    }
    if let Some(genre) = phrase_cle(invite) {
        return genre;
    }
    if invite.contains("(yes/no") {
        return Genre::HoteInconnu;
    }
    // Visible seulement pour une demande de nom d'utilisateur ; tout ce qui
    // évoque un mot de passe reste masqué, même avec « username » dans l'URL.
    let bas = invite.trim_start().to_lowercase();
    let visible =
        bas.starts_with("username") && !bas.contains("password") && !bas.contains("passphrase");
    Genre::Texte { masque: !visible }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn phrase(c: &str, n: bool) -> Genre {
        Genre::PhraseCle {
            cle: PathBuf::from(c),
            nouvel_essai: n,
        }
    }

    #[test]
    fn phrase_ssh() {
        assert_eq!(
            classer_invite("Enter passphrase for key '/home/u/.ssh/id_ed25519': ", None),
            phrase("/home/u/.ssh/id_ed25519", false)
        );
    }

    #[test]
    fn phrase_ssh_add() {
        assert_eq!(
            classer_invite("Enter passphrase for /home/u/.ssh/id_rsa: ", None),
            phrase("/home/u/.ssh/id_rsa", false)
        );
        // Sans espace final ni avec saut de ligne.
        assert_eq!(
            classer_invite("Enter passphrase for /home/u/k:\n", None),
            phrase("/home/u/k", false)
        );
    }

    #[test]
    fn phrase_avec_confirmation() {
        assert_eq!(
            classer_invite(
                "Enter passphrase for /home/u/.ssh/id_rsa (will confirm each use): ",
                None
            ),
            phrase("/home/u/.ssh/id_rsa", false)
        );
    }

    #[test]
    fn phrase_incorrecte() {
        assert_eq!(
            classer_invite("Bad passphrase, try again for /home/u/.ssh/id_rsa: ", None),
            phrase("/home/u/.ssh/id_rsa", true)
        );
    }

    #[test]
    fn hote_inconnu_multiligne() {
        let t = "The authenticity of host 'h (1.2.3.4)' can't be established.\nED25519 key fingerprint is SHA256:abc.\nThis key is not known by any other names.\nAre you sure you want to continue connecting (yes/no/[fingerprint])? ";
        assert_eq!(classer_invite(t, None), Genre::HoteInconnu);
    }

    #[test]
    fn mot_de_passe_masque() {
        assert_eq!(
            classer_invite("alice@exemple.org's password: ", None),
            Genre::Texte { masque: true }
        );
        assert_eq!(
            classer_invite("Password for 'http://x@127.0.0.1:18401': ", None),
            Genre::Texte { masque: true }
        );
    }

    #[test]
    fn nom_utilisateur_git_visible() {
        assert_eq!(
            classer_invite("Username for 'http://127.0.0.1:18401': ", None),
            Genre::Texte { masque: false }
        );
    }

    #[test]
    fn mot_de_passe_git_avec_username_dans_l_url_masque() {
        assert_eq!(
            classer_invite("Password for 'https://myusername@exemple.org': ", None),
            Genre::Texte { masque: true }
        );
    }

    #[test]
    fn mot_de_passe_ssh_utilisateur_username_masque() {
        assert_eq!(
            classer_invite("myusername@hote's password: ", None),
            Genre::Texte { masque: true }
        );
    }

    #[test]
    fn confirm_et_none() {
        assert_eq!(
            classer_invite("Allow use of key x?", Some("confirm")),
            Genre::Confirmation
        );
        assert_eq!(classer_invite("Texte", Some("none")), Genre::Information);
        // Valeur inconnue : classement par le texte.
        assert_eq!(
            classer_invite("Password: ", Some("autre")),
            Genre::Texte { masque: true }
        );
    }

    #[test]
    fn chemin_avec_espace_et_apostrophe() {
        assert_eq!(
            classer_invite("Enter passphrase for key '/home/u/mes clés/l'id': ", None),
            phrase("/home/u/mes clés/l'id", false)
        );
    }

    #[test]
    fn chemins_relatifs_et_tilde() {
        let Some(home) = std::env::home_dir() else {
            return;
        };
        assert_eq!(
            classer_invite("Enter passphrase for key '~/.ssh/id': ", None),
            phrase(&home.join(".ssh/id").to_string_lossy(), false)
        );
        assert_eq!(
            classer_invite("Enter passphrase for cle: ", None),
            phrase(&home.join("cle").to_string_lossy(), false)
        );
    }

    #[test]
    fn invite_vide_texte_masque() {
        assert_eq!(classer_invite("", None), Genre::Texte { masque: true });
    }

    #[test]
    fn entrees_etranges_sans_panique() {
        let enorme = "é'".repeat(100_000);
        let cas = [
            "Enter passphrase for key '",
            "Enter passphrase for key ': ",
            "Enter passphrase for : ",
            "Enter passphrase for ",
            "Bad passphrase, try again for ",
            "(yes/no",
            "\0\u{1F600}\n\r",
            "Enter passphrase for key 'é': \u{301}",
            &enorme,
        ];
        for c in cas {
            let _ = classer_invite(c, None);
            let _ = classer_invite(c, Some("confirm"));
        }
    }
}
