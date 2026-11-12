// SPDX-License-Identifier: GPL-3.0-or-later
//! Menu « Préférences » : ouvre les fichiers de configuration dans l'éditeur de texte par
//! défaut. Un fichier absent est d'abord créé avec son contenu par défaut commenté ; un
//! fichier existant n'est jamais modifié.

use gtk::gio;
use gtk::prelude::*;
use pupitre_config::{chemin_config, chemin_connexions, creer_defaut_si_absent, ecrire_atomique};
use pupitre_i18n::{t, ta, ArgI18n};
use pupitre_shell::creer_config_starship_si_absente;
use std::ffi::OsStr;
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

/// Éditeur en terminal choisi par l'utilisateur : `$VISUAL`, sinon `$EDITOR` (convention des
/// terminaux), découpé en mots, si son programme existe (`path` : dossiers de recherche).
pub fn editeur_terminal(
    visual: Option<&str>,
    editor: Option<&str>,
    path: &OsStr,
) -> Option<Vec<String>> {
    [visual, editor].into_iter().flatten().find_map(|v| {
        let argv = decouper_mots(v)?;
        crate::shells::trouver_shell(argv.first()?, path)?;
        Some(argv)
    })
}

/// Découpe une commande en mots comme le shell : espaces, guillemets simples (tout littéral),
/// guillemets doubles (`\` n'y protège que `"`, `\`, `$`, `` ` ``), `\` hors guillemets.
/// `None` si un guillemet n'est pas fermé ou si la commande finit par `\`. Aucune expansion.
fn decouper_mots(s: &str) -> Option<Vec<String>> {
    let mut mots = Vec::new();
    let mut mot = String::new();
    let mut dans_mot = false;
    let mut car = s.chars();
    while let Some(c) = car.next() {
        match c {
            c if c.is_whitespace() => {
                if dans_mot {
                    mots.push(std::mem::take(&mut mot));
                    dans_mot = false;
                }
            }
            '\'' => {
                dans_mot = true;
                loop {
                    match car.next()? {
                        '\'' => break,
                        x => mot.push(x),
                    }
                }
            }
            '"' => {
                dans_mot = true;
                loop {
                    match car.next()? {
                        '"' => break,
                        '\\' => match car.next()? {
                            x @ ('"' | '\\' | '$' | '`') => mot.push(x),
                            x => {
                                mot.push('\\');
                                mot.push(x);
                            }
                        },
                        x => mot.push(x),
                    }
                }
            }
            '\\' => {
                dans_mot = true;
                mot.push(car.next()?);
            }
            x => {
                dans_mot = true;
                mot.push(x);
            }
        }
    }
    if dans_mot {
        mots.push(mot);
    }
    Some(mots)
}

/// Application qui ouvrira `chemin` : celle associée à son type, sinon l'éditeur de texte
/// par défaut (souvent, aucune n'est associée à `.toml` : le bureau demanderait alors
/// laquelle utiliser). `None` : le choix est laissé au bureau.
pub(crate) fn appli_pour(chemin: &Path) -> Option<gio::AppInfo> {
    let (genre, _) = gio::content_type_guess(Some(chemin), None);
    gio::AppInfo::default_for_type(&genre, false)
        .or_else(|| gio::AppInfo::default_for_type("text/plain", false))
}

/// Lanceur réel : l'application de [`appli_pour`], sinon celle que choisit le bureau
/// (via le portail sous Wayland).
pub(crate) fn lanceur_gtk(parent: gtk::Window) -> Lanceur {
    Box::new(move |chemin, fin| {
        let fichier = gio::File::for_path(chemin);
        if let Some(appli) = appli_pour(chemin) {
            let contexte = WidgetExt::display(&parent).app_launch_context();
            fin(appli
                .launch(&[fichier], Some(&contexte))
                .map_err(|e| e.to_string()));
            return;
        }
        let launcher = gtk::FileLauncher::new(Some(&fichier));
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

    /// Aucune application associée au type du fichier (cas fréquent de `.toml`) : l'éditeur
    /// de texte par défaut est pris, au lieu de laisser le bureau demander laquelle utiliser.
    #[test]
    fn sans_appli_pour_le_type_editeur_de_texte() {
        let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let d = tempfile::tempdir().unwrap();
        let (donnees, config) = (d.path().join("data"), d.path().join("config"));
        std::fs::create_dir_all(donnees.join("applications")).unwrap();
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(
            donnees.join("applications/editeur-essai.desktop"),
            "[Desktop Entry]\nType=Application\nName=Éditeur d'essai\nExec=true %f\n\
             MimeType=text/plain;\n",
        )
        .unwrap();
        std::fs::write(
            config.join("mimeapps.list"),
            "[Default Applications]\ntext/plain=editeur-essai.desktop\n",
        )
        .unwrap();
        // Seuls ces dossiers : aucune application du système ne peut répondre.
        let anciens: Vec<_> = [
            "XDG_DATA_HOME",
            "XDG_DATA_DIRS",
            "XDG_CONFIG_HOME",
            "XDG_CONFIG_DIRS",
        ]
        .iter()
        .map(|v| (*v, std::env::var_os(v)))
        .collect();
        std::env::set_var("XDG_DATA_HOME", &donnees);
        std::env::set_var("XDG_DATA_DIRS", &donnees);
        std::env::set_var("XDG_CONFIG_HOME", &config);
        std::env::set_var("XDG_CONFIG_DIRS", &config);
        let appli = appli_pour(&config.join("pupitre/pupitre.toml"));
        for (v, a) in anciens {
            match a {
                Some(a) => std::env::set_var(v, a),
                None => std::env::remove_var(v),
            }
        }
        assert_eq!(
            appli.and_then(|a| a.id()).as_deref(),
            Some("editeur-essai.desktop")
        );
    }

    #[test]
    fn editeur_du_terminal_visual_puis_editor() {
        let path = std::ffi::OsString::from("/usr/bin:/bin");
        let e = |v: Option<&str>, ed: Option<&str>| editeur_terminal(v, ed, &path);
        assert_eq!(e(Some("cat"), Some("sh")), Some(vec!["cat".to_string()]));
        // Arguments gardés ; programme introuvable : on passe au suivant.
        assert_eq!(
            e(Some("nexistepas -w"), Some("sh -e")),
            Some(vec!["sh".to_string(), "-e".to_string()])
        );
        assert_eq!(e(None, Some("/bin/sh")), Some(vec!["/bin/sh".to_string()]));
        assert_eq!(e(Some("  "), None), None);
        assert_eq!(e(Some("nexistepas"), None), None);
        assert_eq!(e(None, None), None);
        // Guillemets compris ; guillemet non fermé : valeur ignorée, on passe à la suivante.
        assert_eq!(
            e(Some("sh -c ''"), None),
            Some(vec!["sh".to_string(), "-c".to_string(), String::new()])
        );
        assert_eq!(e(Some("sh 'a"), Some("cat")), Some(vec!["cat".to_string()]));
    }

    #[test]
    fn decoupage_comme_le_shell() {
        let d = |s: &str| decouper_mots(s);
        let v = |m: &[&str]| Some(m.iter().map(|x| x.to_string()).collect::<Vec<_>>());
        assert_eq!(
            d("emacsclient -t -a ''"),
            v(&["emacsclient", "-t", "-a", ""])
        );
        assert_eq!(
            d("\"/opt/mon éditeur/ed\" -w"),
            v(&["/opt/mon éditeur/ed", "-w"])
        );
        assert_eq!(d("vim\\ x  'a b'c"), v(&["vim x", "a bc"]));
        assert_eq!(d("ed \"a\\\"b\""), v(&["ed", "a\"b"]));
        assert_eq!(d("   "), v(&[]));
        assert_eq!(d("ed 'non fermé"), None);
        assert_eq!(d("ed \"non fermé"), None);
        assert_eq!(d("ed \\"), None);
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
