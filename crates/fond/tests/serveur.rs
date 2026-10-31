// SPDX-License-Identifier: GPL-3.0-or-later
//! Tests d'intégration du serveur de `pupitre-fond` : un vrai socket, de vrais shells.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pupitre_fond::serveur::{preparer_dossier, run_avec, socket_path};
use pupitre_fond::ssh::EntreesSsh;
use pupitre_proto::{read_frame, write_frame, ClientMsg, ServerMsg, SessionId, VERSION};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

struct Serveur {
    dossier: tempfile::TempDir,
    socket: PathBuf,
    fin: mpsc::Receiver<std::io::Result<()>>,
}

/// Entrées SSH isolées : ni l'agent ni le `/tmp` de l'utilisateur ne sont consultés.
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
        let _ = tx.send(run_avec(&s, idle, entrees));
    });
    Serveur {
        dossier,
        socket,
        fin,
    }
}

/// Faux agent SSH : répond à chaque connexion par une liste d'identités vide.
fn faux_agent(chemin: &Path) {
    use std::io::{Read, Write};
    std::fs::create_dir_all(chemin.parent().unwrap()).unwrap();
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
    c.envoyer(ClientMsg::Hello { version: 3 });
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
