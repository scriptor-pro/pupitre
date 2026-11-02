// SPDX-License-Identifier: GPL-3.0-or-later
//! Menu « Préférences » : ouvre les fichiers de configuration dans l'éditeur de texte par
//! défaut. Un fichier absent est d'abord créé avec son contenu par défaut commenté ; un
//! fichier existant n'est jamais modifié.

use gtk::gio;
use pupitre_config::{chemin_config, chemin_connexions, creer_defaut_si_absent, ecrire_atomique};
use pupitre_i18n::{t, ta, ArgI18n};
use pupitre_shell::creer_config_starship_si_absente;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// Les fichiers que le menu permet de modifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FichierPrefs {
    /// `pupitre.toml`
    Configuration,
    /// `starship.toml`
    Invite,
    /// `connexions.toml`
    Connexions,
}

/// Contenu d'un `connexions.toml` créé vide : valide, avec un exemple en commentaire.
pub const CONNEXIONS_DEFAUT: &str = "\
# Connexions SSH mémorisées par Pupitre.
# Une connexion par bloc [[connexion]] ; seuls « nom » et « hote » sont obligatoires.
# Pupitre vérifie ce fichier à chaque enregistrement et signale toute erreur.
#
# [[connexion]]
# nom = \"serveur\"
# hote = \"serveur.exemple.org\"
# utilisateur = \"alice\"
# port = 22
# cle = \"/home/alice/.ssh/id_ed25519\"
";

/// Chemin complet du fichier.
pub fn chemin(f: FichierPrefs) -> PathBuf {
    match f {
        FichierPrefs::Configuration => chemin_config(),
        FichierPrefs::Invite => crate::shells::chemin_starship(),
        FichierPrefs::Connexions => chemin_connexions(),
    }
}

/// Crée le fichier s'il manque (jamais de modification d'un fichier existant) et renvoie
/// son chemin.
pub fn creer_si_absent(f: FichierPrefs) -> io::Result<PathBuf> {
    let c = chemin(f);
    match f {
        FichierPrefs::Configuration => {
            creer_defaut_si_absent(&c)?;
        }
        FichierPrefs::Invite => creer_config_starship_si_absente(&c)?,
        FichierPrefs::Connexions => match std::fs::symlink_metadata(&c) {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                if let Some(d) = c.parent().filter(|d| !d.as_os_str().is_empty()) {
                    std::fs::create_dir_all(d)?;
                }
                ecrire_atomique(&c, CONNEXIONS_DEFAUT.as_bytes())?;
            }
            Err(e) => return Err(e),
        },
    }
    Ok(c)
}

/// Rappel appelé une fois, avec le résultat du lancement.
pub type Fin = Box<dyn FnOnce(Result<(), String>)>;
/// Ouvre un fichier dans l'éditeur ; remplaçable pour simuler un échec.
pub type Lanceur = Box<dyn FnOnce(&Path, Fin)>;

fn message_echec(chemin: &Path) -> String {
    ta(
        "prefs-ouverture-impossible",
        &[("chemin", ArgI18n::from(chemin.display().to_string()))],
    )
}

/// Crée le fichier si besoin puis le confie à `lanceur`. `signaler` reçoit le message à
/// afficher (et annoncer) : confirmation, ou échec avec le chemin complet.
pub fn ouvrir_avec(f: FichierPrefs, lanceur: Lanceur, signaler: Rc<dyn Fn(String)>) {
    let c = match creer_si_absent(f) {
        Ok(c) => c,
        Err(_) => {
            signaler(message_echec(&chemin(f)));
            return;
        }
    };
    let s = signaler.clone();
    let c2 = c.clone();
    lanceur(
        &c,
        Box::new(move |r| match r {
            Ok(()) => s(t("prefs-ouvert")),
            Err(_) => s(message_echec(&c2)),
        }),
    );
}

/// Lanceur réel : l'éditeur par défaut, via le portail sous Wayland.
pub(crate) fn lanceur_gtk(parent: gtk::Window) -> Lanceur {
    Box::new(move |chemin, fin| {
        let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(chemin)));
        launcher.launch(Some(&parent), gio::Cancellable::NONE, move |r| {
            fin(r.map_err(|e| e.to_string()))
        });
    })
}

/// Ouvre le fichier dans l'éditeur de texte par défaut.
pub fn ouvrir(f: FichierPrefs, parent: &gtk::Window, signaler: impl Fn(String) + 'static) {
    ouvrir_avec(f, lanceur_gtk(parent.clone()), Rc::new(signaler));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::sync::Mutex;

    static ENV: Mutex<()> = Mutex::new(());

    fn avec_config<R>(corps: impl FnOnce(&Path) -> R) -> R {
        let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let d = tempfile::tempdir().unwrap();
        let ancien = std::env::var_os("XDG_CONFIG_HOME");
        std::env::set_var("XDG_CONFIG_HOME", d.path());
        let r = corps(d.path());
        match ancien {
            Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
        r
    }

    #[test]
    fn chemins_preferences() {
        avec_config(|base| {
            let p = base.join("pupitre");
            assert_eq!(chemin(FichierPrefs::Configuration), p.join("pupitre.toml"));
            assert_eq!(chemin(FichierPrefs::Invite), p.join("starship.toml"));
            assert_eq!(chemin(FichierPrefs::Connexions), p.join("connexions.toml"));
        });
    }

    #[test]
    fn fichier_cree_si_absent() {
        avec_config(|_| {
            for f in [
                FichierPrefs::Configuration,
                FichierPrefs::Invite,
                FichierPrefs::Connexions,
            ] {
                let c = chemin(f);
                assert!(!c.exists());
                assert_eq!(creer_si_absent(f).unwrap(), c);
                let contenu = std::fs::read(&c).unwrap();
                assert!(!contenu.is_empty(), "{f:?} vide");
                // Existant : inchangé, octet pour octet.
                let perso = b"# a moi\n\xff[x]\n";
                std::fs::write(&c, perso).unwrap();
                creer_si_absent(f).unwrap();
                assert_eq!(std::fs::read(&c).unwrap(), perso);
            }
        });
    }

    #[test]
    fn connexions_par_defaut_valide_et_vide() {
        avec_config(|_| {
            let c = creer_si_absent(FichierPrefs::Connexions).unwrap();
            assert_eq!(std::fs::read_to_string(&c).unwrap(), CONNEXIONS_DEFAUT);
            assert!(pupitre_config::charger_connexions(&c).unwrap().is_empty());
        });
    }

    #[test]
    fn echec_du_lanceur_donne_le_chemin() {
        avec_config(|_| {
            let recus = Rc::new(RefCell::new(Vec::new()));
            let r = recus.clone();
            let lanceur: Lanceur = Box::new(|_, fin| fin(Err("pas d'éditeur".into())));
            ouvrir_avec(
                FichierPrefs::Connexions,
                lanceur,
                Rc::new(move |m| r.borrow_mut().push(m)),
            );
            let recus = recus.borrow();
            assert_eq!(recus.len(), 1);
            let c = chemin(FichierPrefs::Connexions);
            assert!(recus[0].contains(&c.display().to_string()), "{recus:?}");
        });
    }

    #[test]
    fn succes_du_lanceur_confirme() {
        avec_config(|_| {
            let recus = Rc::new(RefCell::new(Vec::new()));
            let r = recus.clone();
            let lanceur: Lanceur = Box::new(|_, fin| fin(Ok(())));
            ouvrir_avec(
                FichierPrefs::Invite,
                lanceur,
                Rc::new(move |m| r.borrow_mut().push(m)),
            );
            assert_eq!(
                *recus.borrow(),
                vec!["Les modifications s'appliquent dès l'enregistrement.".to_string()]
            );
        });
    }
}
