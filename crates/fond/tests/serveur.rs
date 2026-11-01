// SPDX-License-Identifier: GPL-3.0-or-later
//! Tests d'intégration du serveur de `pupitre-fond` : un vrai socket, de vrais shells.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pupitre_fond::serveur::{
    preparer_dossier, run_avec, run_avec_coffre, socket_path, CoffreCommun,
};
use pupitre_fond::ssh::EntreesSsh;
use pupitre_proto::{
    read_frame, write_frame, ClientMsg, DemandeId, EtatTrousseau, Genre, ServerMsg, SessionId,
    VERSION,
};
use pupitre_ssh::trousseau::{Coffre, CoffreMemoire, CoffreSecretService};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

struct Serveur {
    dossier: tempfile::TempDir,
    socket: PathBuf,
    fin: mpsc::Receiver<std::io::Result<()>>,
}

/// Entrées SSH isolées : ni l'agent ni le `/tmp` de l'utilisateur ne sont consultés.
impl Drop for Serveur {
    /// Le serveur tourne dans un fil que la fin du processus de test interrompt sans le
    /// laisser nettoyer : l'agent qu'il a lancé (propre à ce dossier de test) est tué ici.
    fn drop(&mut self) {
        let motif = format!("ssh-agent -D -a {}", self.dossier.path().display());
        let _ = std::process::Command::new("pkill")
            .args(["-f", &motif])
            .status();
    }
}

fn entrees_isolees(dossier: &Path) -> EntreesSsh {
    let tmp = dossier.join("tmp");
    std::fs::create_dir_all(&tmp).unwrap();
    EntreesSsh {
        env_sock: None,
        runtime: Some(dossier.join("runtime")),
        tmp,
        askpass: None,
        programme_agent: "ssh-agent".into(),
    }
}

fn demarrer(idle: Duration) -> Serveur {
    demarrer_avec(idle, |_| {})
}

/// Démarre un serveur dont les entrées SSH sont isolées puis ajustées par `regler`.
fn demarrer_avec(idle: Duration, regler: impl FnOnce(&mut EntreesSsh)) -> Serveur {
    demarrer_coffre(idle, regler, Arc::new(CoffreMemoire::default()))
}

/// Comme [`demarrer_avec`], avec le dépôt des phrases de passe choisi.
fn demarrer_coffre(
    idle: Duration,
    regler: impl FnOnce(&mut EntreesSsh),
    coffre: CoffreCommun,
) -> Serveur {
    // Un shell de test ne doit pas hériter de l'agent ou de l'askpass de l'utilisateur.
    static NETTOYAGE: std::sync::Once = std::sync::Once::new();
    NETTOYAGE.call_once(|| {
        for v in ["SSH_AUTH_SOCK", "SSH_ASKPASS", "SSH_ASKPASS_REQUIRE"] {
            std::env::remove_var(v);
        }
    });
    let dossier = tempfile::tempdir().unwrap();
    let socket = dossier.path().join("pupitre/fond.sock");
    let mut entrees = entrees_isolees(dossier.path());
    regler(&mut entrees);
    let (tx, fin) = mpsc::channel();
    let s = socket.clone();
    thread::spawn(move || {
        let _ = tx.send(run_avec_coffre(&s, idle, entrees, coffre));
    });
    Serveur {
        dossier,
        socket,
        fin,
    }
}

/// Faux agent SSH : répond à chaque connexion par une liste d'identités vide. Son dossier
/// est en `0700`, comme `XDG_RUNTIME_DIR` ou ceux de `ssh-agent` (un dossier modifiable
/// par le groupe serait refusé).
fn faux_agent(chemin: &Path) {
    use std::io::{Read, Write};
    let dossier = chemin.parent().unwrap();
    std::fs::create_dir_all(dossier).unwrap();
    std::fs::set_permissions(dossier, std::fs::Permissions::from_mode(0o700)).unwrap();
    let l = std::os::unix::net::UnixListener::bind(chemin).unwrap();
    thread::spawn(move || {
        for mut s in l.incoming().flatten() {
            let mut req = [0u8; 5];
            if s.read_exact(&mut req).is_ok() {
                let _ = s.write_all(&[0, 0, 0, 5, 12, 0, 0, 0, 0]);
            }
        }
    });
}

fn agent_repond(socket: &Path) -> bool {
    pupitre_ssh::agent::agent_joignable(socket, Duration::from_secs(1))
}

struct Client {
    lecture: UnixStream,
    ecriture: UnixStream,
}

impl Client {
    fn brut(socket: &Path) -> Client {
        let limite = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(s) = UnixStream::connect(socket) {
                s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
                return Client {
                    ecriture: s.try_clone().unwrap(),
                    lecture: s,
                };
            }
            assert!(Instant::now() < limite, "serveur injoignable");
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn connecter(socket: &Path) -> Client {
        let mut c = Client::brut(socket);
        c.envoyer(ClientMsg::Hello { version: VERSION });
        assert_eq!(c.recevoir(), ServerMsg::Welcome { version: VERSION });
        c
    }

    fn envoyer(&mut self, m: ClientMsg) {
        write_frame(&mut self.ecriture, &m).unwrap();
    }

    fn recevoir(&mut self) -> ServerMsg {
        read_frame(&mut self.lecture).unwrap()
    }

    /// Lit jusqu'au premier message accepté par `f`.
    fn attendre<T>(&mut self, mut f: impl FnMut(&ServerMsg) -> Option<T>) -> T {
        let limite = Instant::now() + Duration::from_secs(10);
        while Instant::now() < limite {
            let m = self.recevoir();
            if let Some(t) = f(&m) {
                return t;
            }
        }
        panic!("message attendu non reçu");
    }

    /// Vrai si rien n'arrive pendant `delai`.
    fn silence(&mut self, delai: Duration) -> bool {
        self.lecture.set_read_timeout(Some(delai)).unwrap();
        let rien = read_frame::<_, ServerMsg>(&mut self.lecture).is_err();
        self.lecture
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        rien
    }

    /// Envoie une demande de phrase de passe (comme `pupitre-askpass`).
    fn demander(&mut self) {
        self.envoyer(ClientMsg::Demander {
            session: None,
            invite: "Enter passphrase for /tmp/x: ".into(),
            genre: Genre::PhraseCle {
                cle: "/tmp/x".into(),
                nouvel_essai: false,
            },
            trousseau: EtatTrousseau::Absent,
        });
    }

    /// Attend une `Demande` et rend son numéro.
    fn attendre_demande(&mut self) -> DemandeId {
        self.attendre(|m| match m {
            ServerMsg::Demande {
                demande, invite, ..
            } => {
                assert_eq!(invite, "Enter passphrase for /tmp/x: ");
                Some(*demande)
            }
            _ => None,
        })
    }

    fn repondre(&mut self, demande: DemandeId, texte: &str) {
        self.envoyer(ClientMsg::Repondre {
            demande,
            reponse: Some(texte.to_string().into()),
            memoriser: false,
        });
    }

    fn ouvrir(&mut self, argv: &[&str]) -> SessionId {
        self.envoyer(ClientMsg::Open {
            argv: Some(argv.iter().map(|s| s.to_string()).collect()),
            cwd: None,
            cols: 80,
            rows: 24,
        });
        self.attendre(|m| match m {
            ServerMsg::Opened { id } => Some(*id),
            ServerMsg::OpenFailed { error } => panic!("{error}"),
            _ => None,
        })
    }

    /// Valeur de la variable `nom` dans le shell d'une nouvelle session
    /// (`<unset>` si elle n'existe pas).
    fn variable_du_shell(&mut self, nom: &str) -> String {
        let script = format!("printf 'VAL=%s\\n' \"${{{nom}-<unset>}}\"; sleep 1");
        let id = self.ouvrir(&["/bin/sh", "-c", &script]);
        let mut tout = Vec::new();
        self.attendre(|m| match m {
            ServerMsg::Output { id: i, data } if *i == id => {
                tout.extend(data);
                let t = String::from_utf8_lossy(&tout).into_owned();
                let debut = t.find("VAL=")? + 4;
                let fin = t[debut..].find('\n')?;
                Some(t[debut..debut + fin].trim_end().to_string())
            }
            _ => None,
        })
    }

    /// Lit la sortie d'une session jusqu'à trouver `motif`.
    fn sortie_contient(&mut self, id: SessionId, motif: &str) {
        let mut tout = Vec::new();
        self.attendre(|m| match m {
            ServerMsg::Output { id: i, data } if *i == id => {
                tout.extend(data);
                String::from_utf8_lossy(&tout).contains(motif).then_some(())
            }
            _ => None,
        })
    }
}

#[test]
fn poignee_de_main() {
    let s = demarrer(Duration::from_secs(30));
    Client::connecter(&s.socket);
    let mut c = Client::brut(&s.socket);
    c.envoyer(ClientMsg::Hello { version: 99 });
    assert!(matches!(c.recevoir(), ServerMsg::Error(e) if e.contains("version")));
    assert!(
        read_frame::<_, ServerMsg>(&mut c.lecture).is_err(),
        "connexion fermée après l'erreur"
    );
}

#[test]
fn hello_obligatoire() {
    let s = demarrer(Duration::from_secs(30));
    let mut c = Client::brut(&s.socket);
    c.envoyer(ClientMsg::List);
    assert!(matches!(c.recevoir(), ServerMsg::Error(_)));
    assert!(read_frame::<_, ServerMsg>(&mut c.lecture).is_err());
}

#[test]
fn ouvrir_saisir_lire() {
    let s = demarrer(Duration::from_secs(30));
    let mut c = Client::connecter(&s.socket);
    let id = c.ouvrir(&["/bin/sh"]);
    c.envoyer(ClientMsg::Input {
        id,
        data: b"echo cou''cou\n".to_vec(),
    });
    c.sortie_contient(id, "coucou");
}

#[test]
fn ouverture_impossible_signalee() {
    let s = demarrer(Duration::from_secs(30));
    let mut c = Client::connecter(&s.socket);
    c.envoyer(ClientMsg::Open {
        argv: Some(vec!["/nexiste/pas".into()]),
        cwd: None,
        cols: 80,
        rows: 24,
    });
    c.attendre(|m| match m {
        ServerMsg::OpenFailed { .. } => Some(()),
        ServerMsg::Opened { .. } => Some(()), // repli possible : Exited suivra
        _ => None,
    });
}

#[test]
fn fin_de_session_diffusee() {
    let s = demarrer(Duration::from_secs(30));
    let mut a = Client::connecter(&s.socket);
    let mut b = Client::connecter(&s.socket);
    let id = a.ouvrir(&["/bin/sh", "-c", "exit 4"]);
    for c in [&mut a, &mut b] {
        let code = c.attendre(|m| match m {
            ServerMsg::Exited { id: i, code } if *i == id => Some(*code),
            _ => None,
        });
        assert_eq!(code, Some(4));
    }
    a.envoyer(ClientMsg::List);
    assert_eq!(
        a.attendre(|m| if let ServerMsg::Sessions(v) = m {
            Some(v.len())
        } else {
            None
        }),
        0
    );
}

#[test]
fn session_survit_au_client() {
    let s = demarrer(Duration::from_secs(30));
    let id = {
        let mut a = Client::connecter(&s.socket);
        let id = a.ouvrir(&["/bin/sh"]);
        a.envoyer(ClientMsg::Input {
            id,
            data: b"echo tra''ce\n".to_vec(),
        });
        a.sortie_contient(id, "trace");
        id
    }; // a se déconnecte
    thread::sleep(Duration::from_millis(200));
    let mut b = Client::connecter(&s.socket);
    b.envoyer(ClientMsg::List);
    let v = b.attendre(|m| {
        if let ServerMsg::Sessions(v) = m {
            Some(v.clone())
        } else {
            None
        }
    });
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].id, id);
    b.envoyer(ClientMsg::Attach { id });
    let relu = b.attendre(|m| match m {
        ServerMsg::Replay { id: i, data } if *i == id => Some(data.clone()),
        _ => None,
    });
    assert!(String::from_utf8_lossy(&relu).contains("trace"));
}

#[test]
fn fermer_une_session() {
    let s = demarrer(Duration::from_secs(30));
    let mut c = Client::connecter(&s.socket);
    let id = c.ouvrir(&["/bin/sh"]);
    c.envoyer(ClientMsg::Close { id });
    c.attendre(|m| matches!(m, ServerMsg::Exited { id: i, .. } if *i == id).then_some(()));
}

#[test]
fn session_inconnue_signalee() {
    let s = demarrer(Duration::from_secs(30));
    let mut c = Client::connecter(&s.socket);
    c.envoyer(ClientMsg::Input {
        id: 12345,
        data: b"x".to_vec(),
    });
    assert!(matches!(c.recevoir(), ServerMsg::Error(_)));
    c.envoyer(ClientMsg::List); // la connexion reste utilisable
    assert!(matches!(c.recevoir(), ServerMsg::Sessions(_)));
}

#[test]
fn occupation_diffusee() {
    let s = demarrer(Duration::from_secs(30));
    let mut c = Client::connecter(&s.socket);
    let id = c.ouvrir(&["/bin/sh"]);
    c.envoyer(ClientMsg::Input {
        id,
        data: b"sleep 1\n".to_vec(),
    });
    c.attendre(|m| matches!(m, ServerMsg::Busy { id: i, busy: true } if *i == id).then_some(()));
    c.attendre(|m| matches!(m, ServerMsg::Busy { id: i, busy: false } if *i == id).then_some(()));
}

#[test]
fn sans_xdg_runtime_dir_repli_sur_tmp() {
    std::env::remove_var("XDG_RUNTIME_DIR");
    let uid = nix::unistd::getuid();
    let attendu = PathBuf::from(format!("/tmp/pupitre-{uid}/fond.sock"));
    assert_eq!(socket_path(), attendu);
    preparer_dossier(&attendu).unwrap();
    let m = std::fs::metadata(attendu.parent().unwrap()).unwrap();
    assert_eq!(m.permissions().mode() & 0o777, 0o700);
}

#[test]
fn dossier_trop_ouvert_refuse() {
    let d = tempfile::tempdir().unwrap();
    let dossier = d.path().join("pupitre");
    std::fs::create_dir(&dossier).unwrap();
    std::fs::set_permissions(&dossier, std::fs::Permissions::from_mode(0o777)).unwrap();
    assert!(preparer_dossier(&dossier.join("fond.sock")).is_err());
}

#[test]
fn socket_perime_est_remplace() {
    let dossier = tempfile::tempdir().unwrap();
    let socket = dossier.path().join("pupitre/fond.sock");
    preparer_dossier(&socket).unwrap();
    // Un socket abandonné : lié puis fermé, le fichier reste.
    drop(std::os::unix::net::UnixListener::bind(&socket).unwrap());
    assert!(socket.exists());
    let s = socket.clone();
    let entrees = entrees_isolees(dossier.path());
    thread::spawn(move || run_avec(&s, Duration::from_secs(30), entrees));
    Client::connecter(&socket);
}

#[test]
fn second_fond_s_arrete_sans_erreur() {
    let s = demarrer(Duration::from_secs(30));
    Client::connecter(&s.socket);
    let debut = Instant::now();
    let entrees = entrees_isolees(s.dossier.path());
    run_avec(&s.socket, Duration::from_secs(30), entrees).unwrap();
    assert!(debut.elapsed() < Duration::from_secs(2));
    Client::connecter(&s.socket); // le premier tourne toujours
}

#[test]
fn arret_si_inactif() {
    let s = demarrer(Duration::from_millis(200));
    let r = s
        .fin
        .recv_timeout(Duration::from_secs(2))
        .expect("le serveur ne s'est pas arrêté");
    r.unwrap();
    assert!(!s.socket.exists(), "socket supprimé à l'arrêt");
}

#[test]
fn pas_d_arret_tant_qu_une_session_vit() {
    let s = demarrer(Duration::from_millis(200));
    {
        let mut c = Client::connecter(&s.socket);
        c.ouvrir(&["/bin/sh"]);
    }
    assert!(
        s.fin.recv_timeout(Duration::from_secs(1)).is_err(),
        "arrêté malgré une session"
    );
}

#[test]
fn flux_massif_ne_bloque_pas() {
    let s = demarrer(Duration::from_secs(30));
    let mut a = Client::connecter(&s.socket);
    let id = a.ouvrir(&["/bin/sh", "-c", "yes | head -c 50000000; sleep 5"]);
    // a ne lit plus rien : il ne doit pas bloquer le serveur.
    let _ = id;
    thread::sleep(Duration::from_millis(300));
    let mut b = Client::connecter(&s.socket);
    let debut = Instant::now();
    b.envoyer(ClientMsg::List);
    b.attendre(|m| matches!(m, ServerMsg::Sessions(_)).then_some(()));
    assert!(
        debut.elapsed() < Duration::from_secs(1),
        "{:?}",
        debut.elapsed()
    );
}

#[test]
fn client_v1_accueilli_sans_message_v2() {
    let s = demarrer(Duration::from_secs(30));
    let mut c = Client::brut(&s.socket);
    c.envoyer(ClientMsg::Hello { version: 1 });
    assert_eq!(c.recevoir(), ServerMsg::Welcome { version: 1 });
    // Fonctionnement normal ; aucun message de la version 2 ne doit arriver.
    let id = c.ouvrir(&["sh", "-c", "echo bonjour; sleep 1"]);
    let mut tout = Vec::new();
    c.attendre(|m| {
        assert!(!m.est_v2(), "message v2 reçu par un client v1 : {m:?}");
        match m {
            ServerMsg::Output { id: i, data } if *i == id => {
                tout.extend(data);
                String::from_utf8_lossy(&tout)
                    .contains("bonjour")
                    .then_some(())
            }
            _ => None,
        }
    });
    // Les demandes v2 d'un client v1 sont ignorées sans faire tomber la connexion.
    c.envoyer(ClientMsg::ChargerCle {
        cle: PathBuf::from("/nulle/part"),
    });
    c.envoyer(ClientMsg::List);
    c.attendre(|m| {
        assert!(!m.est_v2());
        matches!(m, ServerMsg::Sessions(_)).then_some(())
    });
}

#[test]
fn client_v2_negocie_2() {
    let s = demarrer(Duration::from_secs(30));
    let mut c = Client::brut(&s.socket);
    c.envoyer(ClientMsg::Hello { version: 2 });
    assert_eq!(c.recevoir(), ServerMsg::Welcome { version: 2 });
    // Hors de la plage acceptée (trop récent, ou nul) : refus.
    let mut c = Client::brut(&s.socket);
    c.envoyer(ClientMsg::Hello { version: 4 });
    assert!(
        matches!(c.recevoir(), ServerMsg::Error(e) if e.contains("version du protocole incompatible"))
    );
    let mut c = Client::brut(&s.socket);
    c.envoyer(ClientMsg::Hello { version: 0 });
    assert!(
        matches!(c.recevoir(), ServerMsg::Error(e) if e.contains("version du protocole incompatible"))
    );
}

#[test]
fn shell_recoit_ssh_auth_sock() {
    let s = demarrer(Duration::from_secs(30));
    let mut c = Client::connecter(&s.socket);
    let sock = PathBuf::from(c.variable_du_shell("SSH_AUTH_SOCK"));
    assert_eq!(sock, s.dossier.path().join("pupitre/agent.sock"));
    assert!(agent_repond(&sock));
    assert_eq!(
        c.variable_du_shell("PUPITRE_SOCKET"),
        s.socket.to_string_lossy()
    );
    assert!(c
        .variable_du_shell("PUPITRE_SESSION")
        .parse::<u64>()
        .is_ok());
}

#[test]
fn agent_de_l_environnement_reutilise() {
    let dossier = tempfile::tempdir().unwrap();
    let faux = dossier.path().join("env/agent");
    faux_agent(&faux);
    let f = faux.clone();
    let s = demarrer_avec(Duration::from_secs(30), move |e| e.env_sock = Some(f));
    let mut c = Client::connecter(&s.socket);
    assert_eq!(PathBuf::from(c.variable_du_shell("SSH_AUTH_SOCK")), faux);
    assert!(!s.dossier.path().join("pupitre/agent.sock").exists());
}

#[test]
fn agent_existant_retrouve() {
    let dossier = tempfile::tempdir().unwrap();
    let rt = dossier.path().join("rt");
    let faux = rt.join("openssh_agent");
    faux_agent(&faux);
    let r = rt.clone();
    let s = demarrer_avec(Duration::from_secs(30), move |e| e.runtime = Some(r));
    let mut c = Client::connecter(&s.socket);
    assert_eq!(PathBuf::from(c.variable_du_shell("SSH_AUTH_SOCK")), faux);
}

#[test]
fn agent_relance_apres_mort_et_tue_a_la_fin() {
    let s = demarrer_avec(Duration::from_millis(500), |_| {});
    let agent = s.dossier.path().join("pupitre/agent.sock");
    let mut c = Client::connecter(&s.socket);
    assert_eq!(PathBuf::from(c.variable_du_shell("SSH_AUTH_SOCK")), agent);
    let motif = agent.to_string_lossy().into_owned();
    let tuer = || {
        std::process::Command::new("pkill")
            .args(["-f", &format!("ssh-agent -D -a {motif}")])
            .status()
            .unwrap()
            .success()
    };
    let presente = || {
        std::process::Command::new("pgrep")
            .args(["-f", &format!("ssh-agent -D -a {motif}")])
            .output()
            .unwrap()
            .status
            .success()
    };
    assert!(tuer(), "ssh-agent de Pupitre introuvable");
    let limite = Instant::now() + Duration::from_secs(5);
    // Mort pour de bon (plus de ligne de commande : zombie au pire) avant la session suivante.
    while agent_repond(&agent) || presente() {
        assert!(Instant::now() < limite);
        thread::sleep(Duration::from_millis(50));
    }
    // Relancé dès la session suivante, sans attendre l'intervalle de sonde.
    assert_eq!(PathBuf::from(c.variable_du_shell("SSH_AUTH_SOCK")), agent);
    assert!(agent_repond(&agent));
    // Le fond s'arrête (plus de client) : l'agent est tué avec lui.
    drop(c);
    let r = s.fin.recv_timeout(Duration::from_secs(20));
    assert!(matches!(r, Ok(Ok(()))), "le fond devait s'arrêter : {r:?}");
    let limite = Instant::now() + Duration::from_secs(5);
    while presente() {
        assert!(Instant::now() < limite, "ssh-agent encore là");
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn variables_askpass_presentes() {
    let s = demarrer_avec(Duration::from_secs(30), |e| {
        e.askpass = Some("/bin/true".into())
    });
    let mut c = Client::connecter(&s.socket);
    assert_eq!(c.variable_du_shell("SSH_ASKPASS"), "/bin/true");
    assert_eq!(c.variable_du_shell("SSH_ASKPASS_REQUIRE"), "force");
}

#[test]
fn sans_askpass_pas_de_variable() {
    let s = demarrer(Duration::from_secs(30));
    let mut c = Client::connecter(&s.socket);
    assert_eq!(c.variable_du_shell("SSH_ASKPASS"), "<unset>");
    assert_eq!(c.variable_du_shell("SSH_ASKPASS_REQUIRE"), "<unset>");
}

#[test]
fn sans_ssh_agent_message() {
    let s = demarrer_avec(Duration::from_secs(30), |e| {
        e.programme_agent = "/nonexistent/ssh-agent".into();
    });
    let mut c = Client::connecter(&s.socket);
    let id = c.ouvrir(&["/bin/sh", "-c", "sleep 1"]);
    let texte = c.attendre(|m| match m {
        ServerMsg::Message(t) => Some(t.clone()),
        _ => None,
    });
    assert_eq!(
        texte,
        "Agent SSH indisponible : les phrases de passe seront demandées à chaque connexion."
    );
    assert!(id > 0);
    // Une seule fois : la session suivante n'en provoque pas d'autre.
    let mut c2 = Client::connecter(&s.socket);
    assert_eq!(c2.variable_du_shell("SSH_AUTH_SOCK"), "<unset>");
    c2.envoyer(ClientMsg::List);
    c2.attendre(|m| matches!(m, ServerMsg::Sessions(_)).then_some(()));
}

/// Client v2 qui sera demandeur : accueilli, puis `Demander`. Les `Demande` reçues entre
/// les deux (interface d'avant) sont ignorées par `attendre_reponse`.
fn demandeur(socket: &Path) -> Client {
    let mut c = Client::brut(socket);
    c.envoyer(ClientMsg::Hello { version: 2 });
    c.demander();
    c
}

fn attendre_reponse(c: &mut Client) -> (Option<String>, bool) {
    c.attendre(|m| match m {
        ServerMsg::Reponse { reponse, memoriser } => {
            Some((reponse.as_ref().map(|r| r.to_string()), *memoriser))
        }
        _ => None,
    })
}

#[test]
fn demande_relayee_puis_reponse() {
    let s = demarrer(Duration::from_secs(30));
    let mut ui = Client::connecter(&s.socket);
    let mut ask = demandeur(&s.socket);
    let d = ui.attendre_demande();
    ui.envoyer(ClientMsg::Repondre {
        demande: d,
        reponse: Some("secret".to_string().into()),
        memoriser: true,
    });
    assert_eq!(attendre_reponse(&mut ask), (Some("secret".into()), true));
}

#[test]
fn premiere_reponse_gagne() {
    let s = demarrer(Duration::from_secs(30));
    let mut a = Client::connecter(&s.socket);
    let mut b = Client::connecter(&s.socket);
    let mut ask = demandeur(&s.socket);
    let da = a.attendre_demande();
    let db = b.attendre_demande();
    assert_eq!(da, db);
    a.repondre(da, "premier");
    b.attendre(|m| {
        matches!(m, ServerMsg::DemandeClose { demande } if *demande == da).then_some(())
    });
    b.repondre(da, "second");
    assert_eq!(attendre_reponse(&mut ask), (Some("premier".into()), false));
    // La seconde réponse est ignorée : rien d'autre pour le demandeur.
    assert!(ask.silence(Duration::from_millis(300)));
    // L'interface gagnante ne reçoit pas de DemandeClose.
    assert!(a.silence(Duration::from_millis(300)));
}

#[test]
fn demande_annulee_si_askpass_part() {
    let s = demarrer(Duration::from_secs(30));
    let mut ui = Client::connecter(&s.socket);
    let ask = demandeur(&s.socket);
    let d = ui.attendre_demande();
    drop(ask);
    ui.attendre(|m| {
        matches!(m, ServerMsg::DemandeClose { demande } if *demande == d).then_some(())
    });
}

#[test]
fn sans_interface_refus_immediat() {
    let s = demarrer(Duration::from_secs(30));
    let mut ask = demandeur(&s.socket);
    assert_eq!(attendre_reponse(&mut ask), (None, false));
}

#[test]
fn demande_renvoyee_a_une_interface_qui_arrive() {
    let s = demarrer(Duration::from_secs(30));
    let mut a = Client::connecter(&s.socket);
    let mut ask = demandeur(&s.socket);
    let d = a.attendre_demande();
    // Une interface arrive plus tard : elle reçoit la demande encore ouverte.
    let mut b = Client::connecter(&s.socket);
    assert_eq!(b.attendre_demande(), d);
    // Et à nouveau sur List.
    b.envoyer(ClientMsg::List);
    assert_eq!(b.attendre_demande(), d);
    b.repondre(d, "ok");
    assert_eq!(attendre_reponse(&mut ask), (Some("ok".into()), false));
}

#[test]
fn client_v1_ne_recoit_pas_de_demande() {
    let s = demarrer(Duration::from_secs(30));
    let mut v1 = Client::brut(&s.socket);
    v1.envoyer(ClientMsg::Hello { version: 1 });
    assert_eq!(v1.recevoir(), ServerMsg::Welcome { version: 1 });
    // Seul client v1 : pas une interface, refus immédiat.
    let mut ask = demandeur(&s.socket);
    assert_eq!(attendre_reponse(&mut ask), (None, false));
    assert!(v1.silence(Duration::from_millis(300)));
}

#[test]
fn demandeur_ne_retient_pas_le_fond() {
    let s = demarrer(Duration::from_millis(300));
    let mut ask = demandeur(&s.socket);
    assert_eq!(attendre_reponse(&mut ask), (None, false));
    s.fin
        .recv_timeout(Duration::from_secs(3))
        .expect("le fond reste vivant à cause d'un demandeur")
        .unwrap();
}

// ---------------------------------------------------------------------------------------
// Chargement des clés mémorisées dans l'agent
// ---------------------------------------------------------------------------------------

/// Vrai seulement si `marque` (valeur de `PUPITRE_TROUSSEAU_JETABLE`, posée par
/// `scripts/trousseau-test.sh`) est le PID du démon qui possède `org.freedesktop.secrets`
/// sur le bus courant : une variable égarée (shell lancé sous le script, ancienne valeur
/// « 1 ») ne suffit jamais à approcher un trousseau.
fn trousseau_jetable_verifie(marque: Option<String>) -> bool {
    let Some(pid) = marque.filter(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
    else {
        return false;
    };
    std::process::Command::new("busctl")
        .args(["--user", "status", "org.freedesktop.secrets"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .is_some_and(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .any(|l| l.strip_prefix("PID=") == Some(pid.as_str()))
        })
}

/// Garde : ces tests écrivent dans le trousseau (et le vident), ils ne tournent que sous le
/// trousseau jetable de `scripts/trousseau-test.sh` (voir `scripts/test-x11.sh`), vérifié
/// par son PID.
fn jetable() -> bool {
    let ok = trousseau_jetable_verifie(std::env::var("PUPITRE_TROUSSEAU_JETABLE").ok());
    if !ok {
        println!("ignoré : trousseau jetable absent");
    }
    ok
}

/// Les tests du trousseau partagent un seul trousseau, et chaque fond charge TOUTES les clés
/// qui y sont : on les exécute l'un après l'autre, sur un trousseau vidé.
fn exclusif() -> Option<std::sync::MutexGuard<'static, ()>> {
    static VERROU: std::sync::Mutex<()> = std::sync::Mutex::new(());
    if !jetable() {
        return None;
    }
    let garde = VERROU.lock().unwrap_or_else(|e| e.into_inner());
    let coffre = CoffreSecretService::ouvrir();
    for c in coffre.cles() {
        let _ = coffre.oublier(&c);
    }
    Some(garde)
}

/// `pupitre-askpass` compilé (une fois) dans le dossier `target/debug` du dépôt.
fn binaire_askpass() -> PathBuf {
    static COMPILATION: std::sync::Once = std::sync::Once::new();
    let racine = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    COMPILATION.call_once(|| {
        let ok = std::process::Command::new(env!("CARGO"))
            .args([
                "build",
                "-q",
                "-p",
                "pupitre-ssh",
                "--bin",
                "pupitre-askpass",
            ])
            .current_dir(&racine)
            .status()
            .unwrap()
            .success();
        assert!(ok, "compilation de pupitre-askpass");
    });
    racine.join("target/debug/pupitre-askpass")
}

/// Clé jetable chiffrée. Le `-N` met la phrase (sans valeur) de test sur la ligne de
/// commande d'`ssh-keygen` : toléré dans un test, interdit dans le code de production.
fn cle_chiffree(dossier: &Path, phrase: &str) -> PathBuf {
    let cle = dossier.join("cle-test");
    let ok = std::process::Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", phrase, "-f"])
        .arg(&cle)
        .status()
        .unwrap()
        .success();
    assert!(ok, "ssh-keygen");
    cle
}

fn lister(agent: &Path) -> String {
    let s = std::process::Command::new("ssh-add")
        .arg("-l")
        .env("SSH_AUTH_SOCK", agent)
        .output()
        .unwrap();
    String::from_utf8_lossy(&s.stdout).into_owned()
}

/// Fond avec le vrai (jetable) Secret Service, l'askpass compilé et un agent à lui.
fn demarrer_reel(coffre: &Arc<CoffreSecretService>) -> Serveur {
    let askpass = binaire_askpass();
    demarrer_coffre(
        Duration::from_secs(6),
        |e| e.askpass = Some(askpass),
        coffre.clone(),
    )
}

fn attendre_chargee(s: &Serveur) -> bool {
    let agent = s.dossier.path().join("pupitre/agent.sock");
    let limite = Instant::now() + Duration::from_secs(20);
    while Instant::now() < limite {
        if lister(&agent).contains("ED25519") {
            return true;
        }
        thread::sleep(Duration::from_millis(100));
    }
    false
}

/// Fin du fond (et donc de son agent) : l'inactivité l'arrête.
fn arreter(s: Serveur) {
    s.fin
        .recv_timeout(Duration::from_secs(20))
        .expect("fond arrêté")
        .unwrap();
}

#[test]
fn cle_memorisee_chargee_au_demarrage() {
    let Some(_exclusif) = exclusif() else {
        return;
    };
    let t = tempfile::tempdir().unwrap();
    let cle = cle_chiffree(t.path(), "phrase-de-test");
    let coffre = Arc::new(CoffreSecretService::ouvrir());
    coffre.ecrire(&cle, "phrase-de-test").unwrap();
    let s = demarrer_reel(&coffre);
    let chargee = attendre_chargee(&s);
    let _ = coffre.oublier(&cle);
    assert!(chargee, "la clé mémorisée aurait dû être chargée");
    arreter(s);
}

#[test]
fn phrase_memorisee_fausse_est_oubliee() {
    let Some(_exclusif) = exclusif() else {
        return;
    };
    let t = tempfile::tempdir().unwrap();
    let cle = cle_chiffree(t.path(), "la-bonne");
    let coffre = Arc::new(CoffreSecretService::ouvrir());
    coffre.ecrire(&cle, "la-fausse").unwrap();
    let debut = Instant::now();
    let s = demarrer_reel(&coffre);
    // Un client v1 (qui n'est pas une interface v2) retient le fond sans recevoir de message.
    let mut tenue = Client::brut(&s.socket);
    tenue.envoyer(ClientMsg::Hello { version: 1 });
    assert_eq!(tenue.recevoir(), ServerMsg::Welcome { version: 1 });
    // Aucune interface connectée pendant le chargement : l'entrée est oubliée et le message
    // est gardé par le fond jusqu'à la première interface qui parle. (Si l'échec n'était pas
    // encore rapporté à sa connexion, il lui serait envoyé directement : le test vaut dans
    // les deux ordres.)
    let limite = Instant::now() + Duration::from_secs(20);
    while !matches!(coffre.lire(&cle), Ok(None)) {
        assert!(Instant::now() < limite, "entrée jamais oubliée");
        thread::sleep(Duration::from_millis(100));
    }
    thread::sleep(Duration::from_millis(300));
    let mut ui = Client::connecter(&s.socket);
    ui.envoyer(ClientMsg::List);
    let texte = ui.attendre(|m| match m {
        ServerMsg::Message(t) if t.contains("ne fonctionne plus") => Some(t.clone()),
        _ => None,
    });
    let attendu = format!(
        "La phrase de passe mémorisée pour {} ne fonctionne plus ; elle a été oubliée. Elle sera redemandée.",
        pupitre_ssh::trousseau::chemin_lisible(&cle)
    );
    assert_eq!(texte, attendu);
    assert!(matches!(coffre.lire(&cle), Ok(None)), "entrée oubliée");
    assert!(debut.elapsed() < Duration::from_secs(20));
    assert!(!lister(&s.dossier.path().join("pupitre/agent.sock")).contains("ED25519"));
    drop((ui, tenue));
    arreter(s);
}

#[test]
fn cle_deja_dans_l_agent_pas_rechargee() {
    let Some(_exclusif) = exclusif() else {
        return;
    };
    let t = tempfile::tempdir().unwrap();
    let cle = cle_chiffree(t.path(), "la-bonne");
    // Notre propre agent, où la clé est déjà chargée (phrase fournie par un faux askpass).
    let agent_sock = t.path().join("a.sock");
    let _agent = pupitre_ssh::agent::AgentPupitre::lancer(&agent_sock, &[]).unwrap();
    let faux = t.path().join("faux.sh");
    std::fs::write(&faux, "#!/bin/sh\necho la-bonne\n").unwrap();
    std::fs::set_permissions(&faux, std::fs::Permissions::from_mode(0o700)).unwrap();
    let ok = std::process::Command::new("ssh-add")
        .arg("--")
        .arg(&cle)
        .env("SSH_AUTH_SOCK", &agent_sock)
        .env("SSH_ASKPASS", &faux)
        .env("SSH_ASKPASS_REQUIRE", "force")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap()
        .status
        .success();
    assert!(ok);
    // Une phrase FAUSSE est mémorisée : si le fond rechargeait la clé, elle serait oubliée.
    let coffre = Arc::new(CoffreSecretService::ouvrir());
    coffre.ecrire(&cle, "la-fausse").unwrap();
    let askpass = binaire_askpass();
    let s = demarrer_coffre(
        Duration::from_secs(6),
        |e| {
            e.askpass = Some(askpass);
            e.env_sock = Some(agent_sock.clone());
        },
        coffre.clone(),
    );
    let mut ui = Client::connecter(&s.socket);
    ui.envoyer(ClientMsg::List);
    ui.attendre(|m| matches!(m, ServerMsg::Sessions(_)).then_some(()));
    thread::sleep(Duration::from_secs(3));
    let encore = matches!(coffre.lire(&cle), Ok(Some(_)));
    let _ = coffre.oublier(&cle);
    assert!(encore, "la clé déjà chargée a été rechargée");
    drop(ui);
    arreter(s);
}

/// Coffre verrouillé, pour le message unique.
struct CoffreVerrouille;

impl Coffre for CoffreVerrouille {
    fn etat(&self) -> EtatTrousseau {
        EtatTrousseau::Verrouille
    }
    fn lire(
        &self,
        _: &Path,
    ) -> Result<Option<zeroize::Zeroizing<String>>, pupitre_ssh::trousseau::ErreurTrousseau> {
        Err(pupitre_ssh::trousseau::ErreurTrousseau::Verrouille)
    }
    fn ecrire(&self, _: &Path, _: &str) -> Result<(), pupitre_ssh::trousseau::ErreurTrousseau> {
        Err(pupitre_ssh::trousseau::ErreurTrousseau::Verrouille)
    }
    fn oublier(&self, _: &Path) -> Result<(), pupitre_ssh::trousseau::ErreurTrousseau> {
        Ok(())
    }
    fn cles(&self) -> Vec<PathBuf> {
        Vec::new()
    }
}

#[test]
fn trousseau_verrouille_message_unique() {
    let s = demarrer_coffre(
        Duration::from_secs(2),
        |e| e.askpass = Some("/bin/true".into()),
        Arc::new(CoffreVerrouille),
    );
    let mut ui = Client::connecter(&s.socket);
    let attendu = "Trousseau verrouillé : les clés mémorisées n'ont pas été chargées.";
    // Le message, gardé par le fond tant qu'aucune interface n'avait parlé, arrive à la
    // première requête de l'interface (ou plus tard s'il n'était pas encore émis).
    ui.envoyer(ClientMsg::List);
    ui.attendre(|m| matches!(m, ServerMsg::Message(t) if t == attendu).then_some(()));
    // Un onglet relance le chargement (trousseau toujours verrouillé) sans répéter le message.
    ui.ouvrir(&["sh", "-c", "sleep 2"]);
    // Un deuxième onglet réessaie mais ne répète pas le message.
    ui.ouvrir(&["sh", "-c", "sleep 2"]);
    thread::sleep(Duration::from_secs(1));
    ui.envoyer(ClientMsg::List);
    ui.attendre(|m| {
        assert!(
            !matches!(m, ServerMsg::Message(t) if t == attendu),
            "message répété"
        );
        matches!(m, ServerMsg::Sessions(_)).then_some(())
    });
    drop(ui);
    arreter(s);
}

#[test]
fn message_garde_livre_apres_la_reponse() {
    // Le message gardé pour la première interface arrive après la réponse à sa première
    // requête (`Sessions`), qui crée la première fenêtre : jamais avant.
    let s = demarrer_coffre(
        Duration::from_secs(2),
        |e| e.askpass = Some("/bin/true".into()),
        Arc::new(CoffreVerrouille),
    );
    // Laisse le chargement constater le verrou avant l'arrivée de l'interface.
    thread::sleep(Duration::from_millis(800));
    let mut ui = Client::connecter(&s.socket);
    ui.envoyer(ClientMsg::List);
    let mut sessions_vues = false;
    ui.attendre(|m| match m {
        ServerMsg::Sessions(_) => {
            sessions_vues = true;
            None
        }
        ServerMsg::Message(_) => Some(()),
        _ => None,
    });
    assert!(sessions_vues, "message livré avant la réponse Sessions");
    drop(ui);
    arreter(s);
}

/// Coffre verrouillé tant que `verrouille` est vrai ; compte les appels.
#[derive(Default)]
struct CoffreBascule {
    verrouille: std::sync::atomic::AtomicBool,
    etats: std::sync::atomic::AtomicUsize,
    listes: std::sync::atomic::AtomicUsize,
}

impl Coffre for CoffreBascule {
    fn etat(&self) -> EtatTrousseau {
        use std::sync::atomic::Ordering;
        self.etats.fetch_add(1, Ordering::SeqCst);
        if self.verrouille.load(Ordering::SeqCst) {
            EtatTrousseau::Verrouille
        } else {
            EtatTrousseau::Disponible
        }
    }
    fn lire(
        &self,
        _: &Path,
    ) -> Result<Option<zeroize::Zeroizing<String>>, pupitre_ssh::trousseau::ErreurTrousseau> {
        Ok(None)
    }
    fn ecrire(&self, _: &Path, _: &str) -> Result<(), pupitre_ssh::trousseau::ErreurTrousseau> {
        Ok(())
    }
    fn oublier(&self, _: &Path) -> Result<(), pupitre_ssh::trousseau::ErreurTrousseau> {
        Ok(())
    }
    fn cles(&self) -> Vec<PathBuf> {
        self.listes
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Vec::new()
    }
}

#[test]
fn reessai_garde_apres_un_chargement_ponctuel() {
    use std::sync::atomic::Ordering;
    let coffre = Arc::new(CoffreBascule::default());
    coffre.verrouille.store(true, Ordering::SeqCst);
    let attendre_etats = |n: usize| {
        let limite = Instant::now() + Duration::from_secs(10);
        while coffre.etats.load(Ordering::SeqCst) < n {
            assert!(Instant::now() < limite, "chargement jamais lancé");
            thread::sleep(Duration::from_millis(20));
        }
        thread::sleep(Duration::from_millis(300));
    };
    let s = demarrer_coffre(
        Duration::from_secs(5),
        |e| e.askpass = Some("/bin/true".into()),
        coffre.clone(),
    );
    // Démarrage : trousseau verrouillé, rien de chargé, nouvel essai prévu.
    attendre_etats(1);
    // Le trousseau est déverrouillé, puis une phrase est saisie et mémorisée : chargement
    // de cette seule clé.
    coffre.verrouille.store(false, Ordering::SeqCst);
    let mut ask = demandeur(&s.socket);
    assert_eq!(attendre_reponse(&mut ask), (None, false));
    ask.envoyer(ClientMsg::ChargerCle {
        cle: "/nonexistent/cle".into(),
    });
    attendre_etats(2);
    assert_eq!(coffre.listes.load(Ordering::SeqCst), 0);
    // Le nouvel onglet doit encore réessayer le chargement de TOUTES les clés.
    let mut ui = Client::connecter(&s.socket);
    ui.ouvrir(&["sh", "-c", "sleep 1"]);
    let limite = Instant::now() + Duration::from_secs(5);
    while coffre.listes.load(Ordering::SeqCst) == 0 {
        assert!(
            Instant::now() < limite,
            "le nouvel essai a été effacé par le chargement ponctuel"
        );
        thread::sleep(Duration::from_millis(20));
    }
    drop((ui, ask));
    arreter(s);
}

#[test]
fn garde_du_trousseau_exige_le_pid_du_demon() {
    // L'ancien marqueur « 1 », une valeur quelconque ou absente : refus, sans exception.
    for marque in [
        None,
        Some(""),
        Some("1"),
        Some("oui"),
        Some("0"),
        Some("12 34"),
    ] {
        assert!(
            !trousseau_jetable_verifie(marque.map(str::to_string)),
            "{marque:?}"
        );
    }
}

#[test]
fn charger_cle_demande_par_l_askpass() {
    let Some(_exclusif) = exclusif() else {
        return;
    };
    let t = tempfile::tempdir().unwrap();
    let cle = cle_chiffree(t.path(), "phrase-de-test");
    let coffre = Arc::new(CoffreSecretService::ouvrir());
    // Fond démarré avec un trousseau vide : rien à charger au départ.
    let s = demarrer_reel(&coffre);
    let agent = s.dossier.path().join("pupitre/agent.sock");
    thread::sleep(Duration::from_secs(1));
    assert!(!lister(&agent).contains("ED25519"));
    // L'askpass vient de mémoriser la phrase puis demande le chargement.
    coffre.ecrire(&cle, "phrase-de-test").unwrap();
    let mut ask = demandeur(&s.socket);
    ask.envoyer(ClientMsg::ChargerCle { cle: cle.clone() });
    let chargee = attendre_chargee(&s);
    let _ = coffre.oublier(&cle);
    assert!(chargee);
    drop(ask);
    arreter(s);
}

#[test]
fn phrase_saisie_fausse_pas_memorisee() {
    let Some(_exclusif) = exclusif() else {
        return;
    };
    let t = tempfile::tempdir().unwrap();
    let cle = cle_chiffree(t.path(), "la-bonne");
    let coffre = Arc::new(CoffreSecretService::ouvrir());
    let s = demarrer_reel(&coffre);
    let mut ui = Client::connecter(&s.socket);
    ui.envoyer(ClientMsg::List);
    // Le chargement de démarrage (trousseau vide) doit être fini avant d'écrire l'entrée.
    thread::sleep(Duration::from_secs(2));
    // L'askpass vient de mémoriser une phrase fausse puis demande le chargement.
    coffre.ecrire(&cle, "la-fausse").unwrap();
    let mut ask = demandeur(&s.socket);
    ask.envoyer(ClientMsg::ChargerCle { cle: cle.clone() });
    let texte = ui.attendre(|m| match m {
        ServerMsg::Message(t) if t.contains("n'a pas été mémorisée") => Some(t.clone()),
        _ => None,
    });
    assert_eq!(
        texte,
        format!(
            "La phrase de passe saisie pour {} n'a pas été mémorisée : elle n'ouvre pas la clé.",
            pupitre_ssh::trousseau::chemin_lisible(&cle)
        )
    );
    assert!(matches!(coffre.lire(&cle), Ok(None)));
    drop((ui, ask));
    arreter(s);
}

/// Un client en version 2 ne reçoit ni marque de commande ni état « lent » : le second
/// devient « absent » pour lui, alors qu'une interface en version 3 le reçoit tel quel.
#[test]
fn client_v2_ne_recoit_ni_marque_ni_lent() {
    use pupitre_fond::serveur::adapter_pour;
    use pupitre_proto::MarqueCommande;

    // Les marques sont écartées pour les versions 1 et 2, gardées pour la 3.
    let marque = ServerMsg::Marque {
        id: 1,
        marque: MarqueCommande::Invite,
        rejouee: false,
    };
    assert_eq!(adapter_pour(marque.clone(), 1), None);
    assert_eq!(adapter_pour(marque.clone(), 2), None);
    assert_eq!(adapter_pour(marque.clone(), 3), Some(marque));

    // De bout en bout : la demande avec un trousseau lent.
    let s = demarrer(Duration::from_secs(30));
    let mut v2 = Client::brut(&s.socket);
    v2.envoyer(ClientMsg::Hello { version: 2 });
    assert_eq!(v2.recevoir(), ServerMsg::Welcome { version: 2 });
    v2.envoyer(ClientMsg::List);
    v2.attendre(|m| matches!(m, ServerMsg::Sessions(_)).then_some(()));
    let mut v3 = Client::connecter(&s.socket);
    v3.envoyer(ClientMsg::List);
    v3.attendre(|m| matches!(m, ServerMsg::Sessions(_)).then_some(()));
    let mut askpass = Client::connecter(&s.socket);
    askpass.envoyer(ClientMsg::Demander {
        session: None,
        invite: "Enter passphrase for /tmp/x: ".into(),
        genre: Genre::PhraseCle {
            cle: "/tmp/x".into(),
            nouvel_essai: false,
        },
        trousseau: EtatTrousseau::Lent,
    });
    let etat = |c: &mut Client| {
        c.attendre(|m| match m {
            ServerMsg::Demande { trousseau, .. } => Some(*trousseau),
            _ => None,
        })
    };
    assert_eq!(etat(&mut v2), EtatTrousseau::Absent);
    assert_eq!(etat(&mut v3), EtatTrousseau::Lent);
}

/// `Ouvrir` : les variables du client arrivent dans le shell, mais ne remplacent jamais
/// celles que le fond impose (agent, askpass, session) ; un nom invalide est ignoré.
#[test]
fn ouvrir_applique_les_variables_du_client() {
    let s = demarrer(Duration::from_secs(30));
    let mut c = Client::connecter(&s.socket);
    let script = "printf 'VAL=%s|%s|%s\\n' \"$MA_VARIABLE\" \"$PUPITRE_SESSION\" \"$SSH_AUTH_SOCK\"; sleep 1";
    c.envoyer(ClientMsg::Ouvrir {
        argv: vec!["/bin/sh".into(), "-c".into(), script.into()],
        cwd: None,
        cols: 80,
        rows: 24,
        env: vec![
            ("MA_VARIABLE".into(), "valeur é".into()),
            ("PUPITRE_SESSION".into(), "999".into()),
            ("SSH_AUTH_SOCK".into(), "/faux".into()),
            ("A=B".into(), "x".into()),
            (String::new(), "x".into()),
            ("NUL".into(), "a\0b".into()),
        ],
    });
    let id = c.attendre(|m| match m {
        ServerMsg::Opened { id } => Some(*id),
        ServerMsg::OpenFailed { error } => panic!("{error}"),
        _ => None,
    });
    let mut tout = Vec::new();
    let ligne = c.attendre(|m| match m {
        ServerMsg::Output { id: i, data } if *i == id => {
            tout.extend(data);
            let t = String::from_utf8_lossy(&tout).into_owned();
            let d = t.find("VAL=")? + 4;
            let f = t[d..].find('\n')?;
            Some(t[d..d + f].trim_end().to_string())
        }
        _ => None,
    });
    let champs: Vec<&str> = ligne.split('|').collect();
    assert_eq!(champs[0], "valeur é");
    assert_eq!(champs[1], id.to_string());
    assert_ne!(champs[2], "/faux");
}
