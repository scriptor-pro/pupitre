// SPDX-License-Identifier: GPL-3.0-or-later
//! Tests graphiques de la fenêtre de demande SSH (phrase de passe, question d'OpenSSH).
//! Lancés par `scripts/test-x11.sh` et `scripts/test-wayland.sh` ; ignorés sinon.
//!
//! Le vrai `ssh-add` est tapé dans un onglet : il appelle `pupitre-askpass`, qui passe par
//! le processus de fond. L'agent SSH est celui, isolé, que le fond lance dans le dossier
//! temporaire. Le trousseau est un `gnome-keyring-daemon` jetable, démarré ici sur le bus
//! privé du test ; sans lui, les vérifications de mémorisation sont sautées.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod commun;

use commun::*;
use gtk::glib;
use gtk::prelude::*;
use pupitre::fenetre::{FenetrePupitre, Onglet, Pupitre};
use pupitre::polices::{dossier_polices, enregistrer_polices_embarquees};
use pupitre_config::Config;
use pupitre_ssh::trousseau::{chemin_lisible, Coffre, CoffreSecretService};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use std::time::Duration;
use vte4::prelude::*;

const NOM_APPLI: &str = "pupitre-test-ssh";
const TITRE: &str = "Phrase de passe SSH";
const PHRASE: &str = "phrase-de-test-é";

fn main() {
    if !actives() {
        return;
    }
    bus_prive();
    // Le marqueur du trousseau jetable des scripts désigne un trousseau du bus extérieur,
    // absent de ce bus privé : on ne garde que celui qu'on démarre ici.
    std::env::remove_var("PUPITRE_TROUSSEAU_JETABLE");
    compiler();
    let perso = isoler_dossiers();
    assert!(enregistrer_polices_embarquees(&dossier_polices().unwrap()));
    glib::set_prgname(Some(NOM_APPLI));
    glib::set_application_name(NOM_APPLI);
    gtk::init().unwrap();

    let trousseau = TrousseauJetable::demarrer();
    if trousseau.is_some() {
        std::env::set_var("PUPITRE_TROUSSEAU_JETABLE", "1");
    }
    std::env::set_var("PUPITRE_FOND", fond_sans_trousseau());

    let dossier = tempfile::tempdir().unwrap();
    let socket = dossier.path().join("pupitre/fond.sock");
    let app = gtk::Application::builder()
        .application_id("fr.pupitre.TestSsh")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gtk::gio::Cancellable::NONE).unwrap();
    let pupitre = Pupitre::demarrer(&app, socket.clone(), Config::default());
    let cle = creer_cle(perso.path());
    let avec_trousseau = trousseau.is_some();

    let mut ex = Executeur::new();
    ex.lancer("ssh_add_ouvre_la_fenetre", || {
        ouvre_la_fenetre(&pupitre, &cle, avec_trousseau)
    });
    ex.lancer("phrase_validee_cle_dans_l_agent", || {
        phrase_validee(&pupitre, &cle, avec_trousseau)
    });
    ex.lancer("phrase_non_memorisee", || {
        phrase_non_memorisee(&pupitre, &cle, avec_trousseau)
    });
    ex.lancer("annuler_ne_repond_rien", || annuler(&pupitre, &cle));
    ex.lancer("fenetre_fermee_quand_demande_annulee", || {
        fermee_si_annulee(&pupitre, &cle)
    });
    ex.lancer("trousseau_absent_case_inactive", || {
        trousseau_absent(&pupitre, &cle)
    });
    ex.lancer("fenetre_dans_la_bonne_fenetre", || {
        bonne_fenetre(&pupitre, &cle)
    });
    drop(pupitre);
    fermer_sessions(&socket);
    drop(trousseau);
    ex.conclure();
}

/// `gnome-keyring-daemon` jetable sur le bus privé du test, déverrouillé, avec ses
/// propres dossiers (mêmes garde-fous que `scripts/trousseau-test.sh`). Tué à la fin.
struct TrousseauJetable {
    demon: Child,
    _dossier: tempfile::TempDir,
}

impl TrousseauJetable {
    fn demarrer() -> Option<Self> {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        let dossier = tempfile::tempdir().ok()?;
        let d = dossier.path();
        for s in ["run", "data", "cfg", "cache"] {
            std::fs::create_dir_all(d.join(s)).ok()?;
        }
        std::fs::set_permissions(d.join("run"), std::fs::Permissions::from_mode(0o700)).ok()?;
        let mut demon = match Command::new("gnome-keyring-daemon")
            .args(["--foreground", "--unlock", "--components=secrets"])
            .env("HOME", d)
            .env("XDG_DATA_HOME", d.join("data"))
            .env("XDG_RUNTIME_DIR", d.join("run"))
            .env("XDG_CONFIG_HOME", d.join("cfg"))
            .env("XDG_CACHE_HOME", d.join("cache"))
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                println!("trousseau jetable indisponible ({e}) : mémorisation non testée");
                return None;
            }
        };
        if let Some(mut stdin) = demon.stdin.take() {
            let _ = stdin.write_all(b"test");
        }
        let pid = demon.id().to_string();
        let mut proprietaire = String::new();
        attendre(Duration::from_secs(5), || {
            proprietaire = pid_du_secret_service();
            !proprietaire.is_empty()
        });
        let g = TrousseauJetable {
            demon,
            _dossier: dossier,
        };
        if proprietaire != pid {
            println!(
                "Secret Service du bus (« {proprietaire} ») différent du démon jetable ({pid}) : \
                 mémorisation non testée"
            );
            return None;
        }
        Some(g)
    }
}

impl Drop for TrousseauJetable {
    fn drop(&mut self) {
        let _ = self.demon.kill();
        let _ = self.demon.wait();
    }
}

fn pid_du_secret_service() -> String {
    Command::new("busctl")
        .args(["--user", "status", "org.freedesktop.secrets"])
        .output()
        .ok()
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .find_map(|l| l.strip_prefix("PID=").map(str::to_string))
        })
        .unwrap_or_default()
}

/// Clé de test chiffrée (jamais utilisée ailleurs).
fn creer_cle(dossier: &Path) -> PathBuf {
    let cle = dossier.join("cle");
    let ok = Command::new("ssh-keygen")
        .args(["-t", "ed25519", "-q", "-N", PHRASE, "-f"])
        .arg(&cle)
        .status()
        .unwrap()
        .success();
    assert!(ok, "ssh-keygen");
    cle
}

fn texte(o: &Onglet) -> String {
    o.terminal()
        .terminal()
        .text_format(vte4::Format::Text)
        .map(|s| s.to_string())
        .unwrap_or_default()
}

fn taper(o: &Onglet, commande: &str) {
    o.terminal()
        .terminal()
        .feed_child(format!("{commande}\n").as_bytes());
}

/// Tape `commande` et attend `motif` à l'écran.
fn executer(o: &Onglet, commande: &str, motif: &str) -> bool {
    taper(o, commande);
    attendre(Duration::from_secs(10), || texte(o).contains(motif))
}

fn onglet(f: &FenetrePupitre) -> Result<Rc<Onglet>, String> {
    let ok = attendre(Duration::from_secs(10), || f.onglet_actif().is_some());
    verifier(ok, || "aucun onglet".into())?;
    f.onglet_actif().ok_or_else(|| "aucun onglet".into())
}

/// Premier onglet de la première fenêtre, agent vidé.
fn preparer(p: &Rc<Pupitre>, marque: &str) -> Result<Rc<Onglet>, String> {
    let ok = attendre(Duration::from_secs(10), || !p.fenetres().is_empty());
    verifier(ok, || "pas de fenêtre".into())?;
    let o = onglet(&p.fenetres()[0])?;
    verifier(
        executer(
            &o,
            &format!("ssh-add -D; echo {marque}-$((1+1))"),
            &format!("{marque}-2"),
        ),
        || format!("shell muet : {:?}", texte(&o)),
    )?;
    Ok(o)
}

fn fenetre_demande() -> Option<gtk::Window> {
    gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Window>().ok())
        .find(|w| w.is_visible() && w.title().as_deref() == Some(TITRE))
}

fn attendre_fenetre() -> Result<gtk::Window, String> {
    attendre(Duration::from_secs(10), || fenetre_demande().is_some());
    fenetre_demande().ok_or_else(|| format!("fenêtre « {TITRE} » absente"))
}

fn descendants(w: &gtk::Widget, v: &mut Vec<gtk::Widget>) {
    let mut e = w.first_child();
    while let Some(x) = e {
        e = x.next_sibling();
        v.push(x.clone());
        descendants(&x, v);
    }
}

fn contenu(fenetre: &gtk::Window) -> Vec<gtk::Widget> {
    let mut v = Vec::new();
    descendants(fenetre.upcast_ref(), &mut v);
    v
}

fn etiquettes(fenetre: &gtk::Window) -> Vec<String> {
    contenu(fenetre)
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Label>().ok())
        .map(|l| l.text().to_string())
        .collect()
}

fn saisie(fenetre: &gtk::Window) -> Result<gtk::PasswordEntry, String> {
    contenu(fenetre)
        .into_iter()
        .find_map(|w| w.downcast::<gtk::PasswordEntry>().ok())
        .ok_or_else(|| "pas de saisie de mot de passe".into())
}

fn case(fenetre: &gtk::Window) -> Result<gtk::CheckButton, String> {
    contenu(fenetre)
        .into_iter()
        .find_map(|w| w.downcast::<gtk::CheckButton>().ok())
        .ok_or_else(|| "pas de case à cocher".into())
}

/// Ferme proprement la fenêtre restante (réponse « annulé ») et vide la saisie du shell.
fn nettoyer(o: &Onglet) {
    if let Some(f) = fenetre_demande() {
        f.close();
    }
    o.terminal().terminal().feed_child(b"\x03");
    patienter(Duration::from_millis(300));
}

fn bouton(fenetre: &gtk::Window, etiquette: &str) -> Result<gtk::Button, String> {
    contenu(fenetre)
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Button>().ok())
        .find(|b| b.label().as_deref() == Some(etiquette))
        .ok_or_else(|| format!("bouton « {etiquette} » absent"))
}

fn valider(fenetre: &gtk::Window, phrase: &str) -> Resultat {
    let s = saisie(fenetre)?;
    s.set_text(phrase);
    s.emit_by_name::<()>("activate", &[]);
    Ok(())
}

fn ouvre_la_fenetre(p: &Rc<Pupitre>, cle: &Path, avec_trousseau: bool) -> Resultat {
    let o = preparer(p, "debut")?;
    taper(&o, &format!("ssh-add '{}'", cle.display()));
    let f = attendre_fenetre()?;
    let lisible = chemin_lisible(cle);
    let etiq = etiquettes(&f);
    verifier(
        etiq.iter()
            .any(|t| t.starts_with("Phrase de passe de la clé") && t.contains(&lisible)),
        || format!("texte de la question absent : {etiq:?}"),
    )?;
    let s = saisie(&f)?;
    let ok = attendre(Duration::from_secs(5), || {
        GtkWindowExt::focus(&f)
            .is_some_and(|w| w == *s.upcast_ref::<gtk::Widget>() || w.is_ancestor(&s))
    });
    verifier(ok, || "le focus n'est pas dans la saisie".into())?;
    verifier(f.is_modal(), || "fenêtre non modale".into())?;
    if avec_trousseau {
        let c = case(&f)?;
        verifier(c.is_active() && c.is_sensitive(), || {
            "case « Mémoriser » non cochée par défaut".to_string()
        })?;
    }
    let (ok, sortie) = sonder_attendu("tests/a11y/demande.json");
    nettoyer(&o);
    verifier(ok, || sortie)
}

/// Lance la sonde AT-SPI en mode vérification sans bloquer la boucle GTK.
fn sonder_attendu(fichier: &str) -> (bool, String) {
    let mut enfant = Command::new("python3")
        .arg(sonde_a11y())
        .args(["--app", NOM_APPLI, "--attendu"])
        .arg(racine().join(fichier))
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    attendre(Duration::from_secs(30), || {
        matches!(enfant.try_wait(), Ok(Some(_)))
    });
    let _ = enfant.kill();
    let o = enfant.wait_with_output().unwrap();
    (
        o.status.success(),
        String::from_utf8_lossy(&o.stdout).into_owned(),
    )
}

/// Empreinte de la clé publique, pour la chercher dans `ssh-add -l`.
fn empreinte(cle: &Path) -> String {
    let s = Command::new("ssh-keygen")
        .arg("-lf")
        .arg(cle.with_extension("pub"))
        .output()
        .unwrap();
    String::from_utf8_lossy(&s.stdout)
        .split_whitespace()
        .nth(1)
        .unwrap_or_default()
        .to_string()
}

fn dans_l_agent(o: &Onglet, cle: &Path, marque: &str) -> bool {
    executer(
        o,
        &format!(
            "ssh-add -l | grep -qF '{}' && echo {marque}-$((1+1))",
            empreinte(cle)
        ),
        &format!("{marque}-2"),
    )
}

fn phrase_validee(p: &Rc<Pupitre>, cle: &Path, avec_trousseau: bool) -> Resultat {
    let o = preparer(p, "val0")?;
    taper(&o, &format!("ssh-add '{}'", cle.display()));
    let f = attendre_fenetre()?;
    valider(&f, PHRASE)?;
    let ferme = attendre(Duration::from_secs(5), || fenetre_demande().is_none());
    verifier(ferme, || "la fenêtre reste ouverte après Entrée".into())?;
    verifier(dans_l_agent(&o, cle, "val1"), || {
        format!("clé absente de l'agent : {:?}", texte(&o))
    })?;
    if avec_trousseau {
        let coffre = CoffreSecretService::ouvrir();
        let ok = attendre(Duration::from_secs(5), || {
            coffre.lire(cle).as_deref().map(String::as_str) == Some(PHRASE)
        });
        let _ = coffre.oublier(cle);
        verifier(ok, || "phrase absente du trousseau".into())?;
    } else {
        println!("  (mémorisation non vérifiée : pas de trousseau jetable)");
    }
    Ok(())
}

fn phrase_non_memorisee(p: &Rc<Pupitre>, cle: &Path, avec_trousseau: bool) -> Resultat {
    if !avec_trousseau {
        println!("  sautée : pas de trousseau jetable");
        return Ok(());
    }
    let o = preparer(p, "nm0")?;
    taper(&o, &format!("ssh-add '{}'", cle.display()));
    let f = attendre_fenetre()?;
    let c = case(&f)?;
    verifier(c.is_active(), || "case décochée d'emblée".into())?;
    c.set_active(false);
    valider(&f, PHRASE)?;
    verifier(dans_l_agent(&o, cle, "nm1"), || {
        format!("clé absente de l'agent : {:?}", texte(&o))
    })?;
    patienter(Duration::from_millis(500));
    let coffre = CoffreSecretService::ouvrir();
    let present = coffre.lire(cle).is_some();
    let _ = coffre.oublier(cle);
    verifier(!present, || {
        "phrase mémorisée malgré la case décochée".into()
    })
}

fn annuler(p: &Rc<Pupitre>, cle: &Path) -> Resultat {
    let o = preparer(p, "an0")?;
    taper(
        &o,
        &format!("ssh-add '{}'; echo an1-code-$?", cle.display()),
    );
    let f = attendre_fenetre()?;
    bouton(&f, "Annuler")?.emit_clicked();
    let ok = attendre(Duration::from_secs(10), || {
        let t = texte(&o);
        t.contains("an1-code-1") || t.contains("an1-code-2")
    });
    verifier(ok, || {
        format!("ssh-add ne termine pas en échec : {:?}", texte(&o))
    })?;
    verifier(
        attendre(Duration::from_secs(2), || fenetre_demande().is_none()),
        || "fenêtre toujours ouverte".into(),
    )
}

fn fermee_si_annulee(p: &Rc<Pupitre>, cle: &Path) -> Resultat {
    let o = preparer(p, "fe0")?;
    taper(&o, &format!("ssh-add '{}'", cle.display()));
    attendre_fenetre()?;
    o.terminal().terminal().feed_child(b"\x03");
    let ok = attendre(Duration::from_secs(2), || fenetre_demande().is_none());
    nettoyer(&o);
    verifier(ok, || "fenêtre toujours ouverte 2 s après Ctrl+C".into())
}

fn trousseau_absent(p: &Rc<Pupitre>, cle: &Path) -> Resultat {
    let o = preparer(p, "ab0")?;
    taper(
        &o,
        &format!(
            "DBUS_SESSION_BUS_ADDRESS=unix:path=/inexistant ssh-add '{}'",
            cle.display()
        ),
    );
    let f = attendre_fenetre()?;
    let c = case(&f)?;
    let etiq = etiquettes(&f);
    nettoyer(&o);
    verifier(!c.is_sensitive() && !c.is_active(), || {
        "la case devrait être inactive et décochée".into()
    })?;
    verifier(
        etiq.iter()
            .any(|t| t == "Aucun trousseau disponible : la phrase sera demandée à chaque fois."),
        || format!("explication absente : {etiq:?}"),
    )
}

fn bonne_fenetre(p: &Rc<Pupitre>, cle: &Path) -> Resultat {
    let seconde = p.nouvelle_fenetre();
    let o = onglet(&seconde)?;
    verifier(executer(&o, "echo deux-$((1+1))", "deux-2"), || {
        "le shell de la seconde fenêtre ne répond pas".into()
    })?;
    taper(&o, &format!("ssh-add '{}'", cle.display()));
    let f = attendre_fenetre()?;
    let parent = f.transient_for();
    nettoyer(&o);
    seconde.fenetre().close();
    verifier(
        parent.as_ref() == Some(seconde.fenetre().upcast_ref::<gtk::Window>()),
        || format!("parent : {parent:?}"),
    )
}
