// SPDX-License-Identifier: GPL-3.0-or-later
//! Tests graphiques du choix du shell : menu « Nouvel onglet avec », shell des réglages,
//! messages d'un shell introuvable. Lancés par `scripts/test-x11.sh` et
//! `scripts/test-wayland.sh` ; ignorés sinon.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod commun;

use commun::*;
use gtk::glib;
use gtk::prelude::*;
use pupitre::fenetre::{FenetrePupitre, Onglet, Pupitre};
use pupitre::polices::{dossier_polices, enregistrer_polices_embarquees};
use pupitre::raccourcis::Raccourci;
use pupitre::shells::{path_courant, shells_proposes};
use pupitre_config::{Config, Invite, ShellDeclare};
use std::rc::Rc;
use std::time::Duration;
use vte4::prelude::*;

const NOM_APPLI: &str = "pupitre-test-shells";

fn main() {
    if !actives() {
        return;
    }
    bus_prive();
    compiler();
    let _perso = isoler_dossiers();
    assert!(enregistrer_polices_embarquees(&dossier_polices().unwrap()));
    glib::set_prgname(Some(NOM_APPLI));
    glib::set_application_name(NOM_APPLI);
    gtk::init().unwrap();

    let dossier = tempfile::tempdir().unwrap();
    let socket = dossier.path().join("pupitre/fond.sock");
    let app = gtk::Application::builder()
        .application_id("fr.pupitre.TestShells")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gtk::gio::Cancellable::NONE).unwrap();
    let pupitre = Pupitre::demarrer(&app, socket.clone(), reglages("", Vec::new()));
    let ok = attendre(Duration::from_secs(10), || {
        premiere_fenetre(&pupitre).is_some_and(|f| f.onglets().len() == 1)
    });
    assert!(ok, "pas de fenêtre au démarrage");

    let mut ex = Executeur::new();
    ex.lancer("menu_nouvel_onglet_avec", || menu(&pupitre));
    ex.lancer("onglet_ouvert_avec_le_shell_choisi", || choisi(&pupitre));
    ex.lancer("nouvel_onglet_reprend_le_shell_actif", || reprend(&pupitre));
    ex.lancer("nouvel_onglet_apres_connexion_shell_par_defaut", || {
        apres_connexion(&pupitre)
    });
    ex.lancer("shell_declare_lance_tel_quel", || declare(&pupitre));
    ex.lancer("integration_transmise_au_shell", || integration(&pupitre));
    ex.lancer("shell_configure_absent_message", || absent(&pupitre));
    ex.lancer("fish_absent_commande_installation", || {
        fish_absent(&pupitre)
    });
    drop(pupitre);
    fermer_sessions(&socket);
    ex.conclure();
}

/// Réglages d'essai : invite du shell (pas de Starship), shell et shells déclarés donnés.
fn reglages(shell: &str, shells: Vec<ShellDeclare>) -> Config {
    Config {
        shell: shell.into(),
        invite: Invite::Shell,
        shells,
        ..Config::default()
    }
}

fn premiere_fenetre(p: &Pupitre) -> Option<FenetrePupitre> {
    p.fenetres().into_iter().find(|f| f.fenetre().is_visible())
}

fn texte(o: &Onglet) -> String {
    o.terminal()
        .terminal()
        .text_format(vte4::Format::Text)
        .map(|s| s.to_string())
        .unwrap_or_default()
}

/// Tape `commande` dans l'onglet et attend `motif`.
fn executer(o: &Onglet, commande: &str, motif: &str) -> bool {
    o.terminal()
        .terminal()
        .feed_child(format!("{commande}\n").as_bytes());
    attendre(Duration::from_secs(10), || texte(o).contains(motif))
}

/// Active l'action `nom` (avec la cible du menu éventuelle) et rend le nouvel onglet.
fn ouvrir(f: &FenetrePupitre, nom: &str, cible: Option<&str>) -> Result<Rc<Onglet>, String> {
    let avant = f.onglets().len();
    let param = cible.map(|c| c.to_variant());
    WidgetExt::activate_action(f.fenetre(), nom, param.as_ref()).map_err(|e| e.to_string())?;
    let ok = attendre(Duration::from_secs(10), || f.onglets().len() == avant + 1);
    verifier(ok, || format!("{nom} : pas de nouvel onglet"))?;
    f.onglet_actif().ok_or_else(|| "aucun onglet actif".into())
}

fn declares() -> Vec<ShellDeclare> {
    vec![
        ShellDeclare {
            nom: "Serveur".into(),
            commande: vec![
                "/bin/sh".into(),
                "-c".into(),
                "echo declare-ok; sleep 20".into(),
            ],
        },
        ShellDeclare {
            nom: "Autre".into(),
            commande: vec!["/bin/sh".into()],
        },
    ]
}

fn menu(p: &Rc<Pupitre>) -> Resultat {
    p.appliquer_config(Ok(reglages("", declares())));
    let f = premiere_fenetre(p).ok_or("pas de fenêtre")?;
    let libelles: Vec<String> = f
        .entrees_nouvel_onglet_avec()
        .into_iter()
        .map(|(l, _)| l)
        .collect();
    let installes: Vec<String> = shells_proposes(&path_courant())
        .into_iter()
        .map(|s| s.nom)
        .collect();
    verifier(!installes.is_empty(), || "aucun shell installé ?".into())?;
    let attendu: Vec<String> = installes
        .iter()
        .cloned()
        .chain(["Serveur".to_string(), "Autre".to_string()])
        .collect();
    verifier(libelles == attendu, || {
        format!("sous-menu {libelles:?}, attendu {attendu:?}")
    })?;
    for faux in ["nologin", "false", "git-shell"] {
        verifier(!libelles.iter().any(|l| l == faux), || {
            format!("{faux} ne doit pas être proposé")
        })?;
    }
    Ok(())
}

fn choisi(p: &Rc<Pupitre>) -> Resultat {
    let f = premiere_fenetre(p).ok_or("pas de fenêtre")?;
    let (_, cible) = f
        .entrees_nouvel_onglet_avec()
        .into_iter()
        .find(|(l, _)| l == "sh")
        .ok_or("/bin/sh absent du sous-menu")?;
    let o = ouvrir(&f, "win.nouvel-onglet-avec", Some(&cible))?;
    verifier(
        executer(&o, "echo \"[$(ps -p $$ -o comm=)]\"", "[sh]"),
        || format!("le shell choisi n'est pas sh : {:?}", texte(&o)),
    )
}

/// Ctrl+Maj+T depuis l'onglet sh (ouvert par `choisi`) : le nouvel onglet est aussi sh.
fn reprend(p: &Rc<Pupitre>) -> Resultat {
    let f = premiere_fenetre(p).ok_or("pas de fenêtre")?;
    let o = ouvrir(&f, "win.nouvel-onglet", None)?;
    verifier(
        executer(&o, "echo \"[$(ps -p $$ -o comm=)]\"", "[sh]"),
        || {
            format!(
                "le shell de l'onglet actif n'est pas repris : {:?}",
                texte(&o)
            )
        },
    )
}

/// Une connexion n'est pas un shell : un nouvel onglet ouvert depuis elle prend le shell
/// des réglages.
fn apres_connexion(p: &Rc<Pupitre>) -> Resultat {
    p.appliquer_config(Ok(reglages("bash", declares())));
    let f = premiere_fenetre(p).ok_or("pas de fenêtre")?;
    let avant = f.onglets().len();
    f.ouvrir_connexion(
        vec![
            "/bin/sh".into(),
            "-c".into(),
            "echo connexion-ok; sleep 20".into(),
        ],
        "serveur".into(),
    );
    let ok = attendre(Duration::from_secs(10), || f.onglets().len() == avant + 1);
    verifier(ok, || "connexion : pas de nouvel onglet".into())?;
    let o = ouvrir(&f, "win.nouvel-onglet", None)?;
    verifier(
        executer(&o, "echo \"[$(ps -p $$ -o comm=)]\"", "[bash]"),
        || {
            format!(
                "pas le shell des réglages après une connexion : {:?}",
                texte(&o)
            )
        },
    )
}

/// Revient au premier onglet (shell par défaut) : un nouvel onglet en reprend le shell.
fn premier_onglet(f: &FenetrePupitre) {
    f.executer(Raccourci::Onglet(1));
}

fn declare(p: &Rc<Pupitre>) -> Resultat {
    let f = premiere_fenetre(p).ok_or("pas de fenêtre")?;
    let (_, cible) = f
        .entrees_nouvel_onglet_avec()
        .into_iter()
        .find(|(l, _)| l == "Serveur")
        .ok_or("« Serveur » absent du sous-menu")?;
    let o = ouvrir(&f, "win.nouvel-onglet-avec", Some(&cible))?;
    verifier(
        attendre(Duration::from_secs(10), || texte(&o).contains("declare-ok")),
        || format!("commande déclarée non lancée : {:?}", texte(&o)),
    )?;
    // Entrée d'un menu pas encore refait, pour un shell retiré de la configuration : rien ne
    // s'ouvre, et c'est dit.
    let avant = f.onglets().len();
    let disparu = "declare:Disparu".to_variant();
    WidgetExt::activate_action(f.fenetre(), "win.nouvel-onglet-avec", Some(&disparu))
        .map_err(|e| e.to_string())?;
    let attendu = "Ce shell n'est plus dans la configuration.";
    let ok = attendre(Duration::from_secs(2), || {
        f.messages().iter().any(|m| m == attendu)
    });
    verifier(ok && f.onglets().len() == avant, || {
        format!(
            "{} onglet(s), messages {:?}",
            f.onglets().len(),
            f.messages()
        )
    })
}

fn integration(p: &Rc<Pupitre>) -> Resultat {
    // Le shell des réglages reçoit ses variables par `Ouvrir` : bash avec l'intégration.
    p.appliquer_config(Ok(reglages("bash", Vec::new())));
    let f = premiere_fenetre(p).ok_or("pas de fenêtre")?;
    premier_onglet(&f);
    let o = ouvrir(&f, "win.nouvel-onglet", None)?;
    verifier(
        executer(&o, "echo \"[$PUPITRE_INVITE]\"", "[shell]"),
        || format!("PUPITRE_INVITE non transmise : {:?}", texte(&o)),
    )
}

fn messages_contenant(f: &FenetrePupitre, morceau: &str) -> Vec<String> {
    f.messages()
        .into_iter()
        .filter(|m| m.contains(morceau))
        .collect()
}

fn absent(p: &Rc<Pupitre>) -> Resultat {
    p.appliquer_config(Ok(reglages("nexistepas", Vec::new())));
    let f = premiere_fenetre(p).ok_or("pas de fenêtre")?;
    premier_onglet(&f);
    let o = ouvrir(&f, "win.nouvel-onglet", None)?;
    verifier(executer(&o, "echo vivant-$((1+1))", "vivant-2"), || {
        "l'onglet n'a pas le shell de connexion".into()
    })?;
    ouvrir(&f, "win.nouvel-onglet", None)?;
    let m = messages_contenant(&f, "nexistepas");
    verifier(
        m == ["Le shell « nexistepas » est introuvable : le shell de connexion est utilisé."],
        || format!("messages : {m:?}"),
    )
}

fn fish_absent(p: &Rc<Pupitre>) -> Resultat {
    p.appliquer_config(Ok(reglages("fish", Vec::new())));
    let f = premiere_fenetre(p).ok_or("pas de fenêtre")?;
    premier_onglet(&f);
    // PATH sans fish pendant l'ouverture seulement.
    let vide = tempfile::tempdir().unwrap();
    let ancien = std::env::var_os("PATH");
    std::env::set_var("PATH", vide.path());
    let r = ouvrir(&f, "win.nouvel-onglet", None);
    if let Some(a) = ancien {
        std::env::set_var("PATH", a);
    }
    r?;
    let m = messages_contenant(&f, "« fish »");
    verifier(m.len() == 1, || format!("messages : {m:?}"))?;
    let os = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
    if os.contains("debian") || os.contains("ubuntu") {
        verifier(m[0].contains("sudo apt install fish"), || {
            format!("pas de commande d'installation : {m:?}")
        })?;
    }
    Ok(())
}
