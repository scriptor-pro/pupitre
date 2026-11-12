// SPDX-License-Identifier: GPL-3.0-or-later
//! Logique de `pupitre-askpass`, appelé par `ssh`, `ssh-add` ou git à travers `SSH_ASKPASS`.
//!
//! Règle : la sortie standard ne reçoit que la réponse suivie de `\n` (et rien du tout pour
//! une confirmation) ; tout message va sur la sortie d'erreur, préfixé `pupitre-askpass : `.

use crate::invite::classer_invite;
use crate::trousseau::{Coffre, EtatTrousseau};
use pupitre_i18n::{t, ta, ArgI18n};
use pupitre_proto::{read_frame, write_frame, ClientMsg, Genre, ServerMsg};
use std::io::Write;
use std::os::unix::net::UnixStream;
use zeroize::Zeroizing;

/// Message sur la sortie d'erreur (jamais de secret).
fn dire(texte: &str) {
    eprintln!(
        "{}",
        ta("askpass-ligne", &[("texte", ArgI18n::from(texte))])
    );
}

/// Phrase `id` avec l'erreur `e` en argument `erreur`.
fn erreur(id: &str, e: &dyn std::fmt::Display) -> String {
    ta(id, &[("erreur", ArgI18n::from(e.to_string()))])
}

/// Écrit la réponse suivie de `\n` en un seul `write_all`, depuis un tampon effacé ensuite,
/// puis vide la sortie. `sortie` doit être sans tampon (le binaire fournit le descripteur 1
/// tel quel) : aucune copie de la phrase ne reste dans un tampon de ligne.
fn ecrire_reponse(sortie: &mut dyn Write, reponse: &str) -> bool {
    let mut tampon = Zeroizing::new(Vec::with_capacity(reponse.len() + 1));
    tampon.extend_from_slice(reponse.as_bytes());
    tampon.push(b'\n');
    sortie
        .write_all(&tampon)
        .and_then(|()| sortie.flush())
        .is_ok()
}

/// Exécute l'askpass : `args` contient le nom du programme puis l'invite, `env` lit
/// l'environnement, `sortie` est la sortie standard. Rend le code de sortie.
pub fn executer(
    args: &[String],
    env: &dyn Fn(&str) -> Option<String>,
    coffre: &dyn Coffre,
    sortie: &mut dyn Write,
) -> i32 {
    let invite = args.get(1).map(String::as_str).unwrap_or_default();
    let prompt = env("SSH_ASKPASS_PROMPT");
    let genre = classer_invite(invite, prompt.as_deref());
    if env("PUPITRE_ASKPASS_MODE").as_deref() == Some("trousseau") {
        return mode_trousseau(&genre, coffre, sortie);
    }
    mode_normal(invite, genre, env, coffre, sortie)
}

/// Appelé par le chargement des clés mémorisées : ni connexion au fond, ni interface.
fn mode_trousseau(genre: &Genre, coffre: &dyn Coffre, sortie: &mut dyn Write) -> i32 {
    match genre {
        Genre::PhraseCle {
            cle,
            nouvel_essai: false,
        } => match coffre.lire(cle) {
            Ok(Some(phrase)) if ecrire_reponse(sortie, &phrase) => 0,
            Ok(_) => 1,
            Err(e) => {
                dire(&erreur("askpass-memorisee-non-lue", &e));
                1
            }
        },
        // La phrase mémorisée vient d'être refusée : on l'oublie, elle sera redemandée.
        Genre::PhraseCle {
            cle,
            nouvel_essai: true,
        } => {
            if let Err(e) = coffre.oublier(cle) {
                dire(&erreur("askpass-memorisee-non-oubliee", &e));
            }
            1
        }
        _ => 1,
    }
}

fn mode_normal(
    invite: &str,
    genre: Genre,
    env: &dyn Fn(&str) -> Option<String>,
    coffre: &dyn Coffre,
    sortie: &mut dyn Write,
) -> i32 {
    let Some(socket) = env("PUPITRE_SOCKET").filter(|s| !s.is_empty()) else {
        dire(&t("askpass-socket-absent"));
        return 1;
    };
    // Poignée de main : un fond plus ancien (qui tourne encore avec des onglets après une
    // mise à jour) refuse la version courante ; on retente alors en version 2.
    let (mut flux, version) = match poignee(&socket, VERSION_ASKPASS) {
        Ok(r) => r,
        Err(Poignee::Refus) if VERSION_ASKPASS > VERSION_PRECEDENTE => {
            match poignee(&socket, VERSION_PRECEDENTE) {
                Ok(r) => r,
                Err(e) => return e.dire(),
            }
        }
        Err(e) => return e.dire(),
    };
    let trousseau = match genre {
        Genre::PhraseCle { .. } => match coffre.etat() {
            // Un fond en version 2 ne connaît pas l'état « lent ».
            EtatTrousseau::Lent if version < 3 => EtatTrousseau::Absent,
            e => e,
        },
        _ => EtatTrousseau::Absent,
    };
    let session = env("PUPITRE_SESSION").and_then(|s| s.parse().ok());
    let envoi = write_frame(
        &mut flux,
        &ClientMsg::Demander {
            session,
            invite: invite.to_string(),
            genre: genre.clone(),
            trousseau,
        },
    );
    if envoi.is_err() {
        dire(&t("askpass-envoi-echec"));
        return 1;
    }
    // Welcome, Demande, Message… ne nous concernent pas : seule la réponse compte.
    let (reponse, memoriser) = loop {
        match read_frame::<_, ServerMsg>(&mut flux) {
            Ok(ServerMsg::Reponse { reponse, memoriser }) => {
                if genre == Genre::Information {
                    // `ssh` nous tue quand l'information n'a plus lieu d'être affichée.
                    if reponse.is_none() {
                        return 1;
                    }
                    continue;
                }
                break (reponse, memoriser);
            }
            Ok(_) => {}
            Err(_) => {
                dire(&t("askpass-connexion-fermee"));
                return 1;
            }
        }
    };
    let Some(reponse) = reponse else {
        return 1;
    };
    match genre {
        // Aucune sortie sur stdout : pour ssh-agent, « rien + code 0 » signifie « autorisé ».
        Genre::Confirmation => i32::from(reponse.as_str() != "yes"),
        Genre::PhraseCle { cle, .. } => {
            let memorisee = memoriser
                && trousseau == EtatTrousseau::Disponible
                && memoriser_phrase(coffre, &cle, &reponse);
            if !ecrire_reponse(sortie, &reponse) {
                return 1;
            }
            if memorisee {
                // Seulement après la remise de la phrase : le chargement n'est qu'un plus.
                let _ = write_frame(&mut flux, &ClientMsg::ChargerCle { cle });
            }
            0
        }
        _ => {
            if ecrire_reponse(sortie, &reponse) {
                0
            } else {
                1
            }
        }
    }
}

/// Version du protocole que parle l'askpass (la 3 : demandes et état « trousseau lent » ; il
/// n'a que faire des messages de la version 4, qu'un fond en version 3 refuserait donc à tort).
const VERSION_ASKPASS: u32 = 3;
/// Version précédente, où l'askpass sait encore parler.
const VERSION_PRECEDENTE: u32 = 2;

/// Échec de la poignée de main avec le processus de fond.
enum Poignee {
    /// Le fond a refusé la version proposée.
    Refus,
    /// Injoignable, connexion fermée ou réponse illisible.
    Echec(String),
}

impl Poignee {
    /// Dit l'échec sur la sortie d'erreur ; code de sortie de l'askpass.
    fn dire(self) -> i32 {
        match self {
            Poignee::Refus => dire(&t("askpass-connexion-fermee")),
            Poignee::Echec(m) => dire(&m),
        }
        1
    }
}

/// Se connecte au fond et échange `Hello`/`Welcome` en proposant `version`.
fn poignee(socket: &str, version: u32) -> Result<(UnixStream, u32), Poignee> {
    let mut flux = UnixStream::connect(socket)
        .map_err(|e| Poignee::Echec(erreur("askpass-fond-injoignable", &e)))?;
    write_frame(&mut flux, &ClientMsg::Hello { version })
        .map_err(|_| Poignee::Echec(t("askpass-envoi-echec")))?;
    match read_frame::<_, ServerMsg>(&mut flux) {
        Ok(ServerMsg::Welcome { version: v }) => Ok((flux, v.min(version))),
        Ok(ServerMsg::Error(_)) => Err(Poignee::Refus),
        Ok(_) | Err(_) => Err(Poignee::Echec(t("askpass-connexion-fermee"))),
    }
}

/// Mémorise la phrase ; un échec est signalé mais n'empêche pas la connexion.
fn memoriser_phrase(coffre: &dyn Coffre, cle: &std::path::Path, phrase: &str) -> bool {
    match coffre.ecrire(cle, phrase) {
        Ok(()) => true,
        Err(e) => {
            dire(&erreur("askpass-non-memorisee", &e));
            false
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::trousseau::CoffreMemoire;
    use std::os::unix::net::UnixListener;
    use std::path::{Path, PathBuf};
    use std::thread::JoinHandle;
    use zeroize::Zeroizing;

    const CLE: &str = "/home/u/.ssh/id_test";

    fn args(invite: &str) -> Vec<String> {
        vec!["pupitre-askpass".into(), invite.into()]
    }

    fn env_de(vars: Vec<(&'static str, String)>) -> impl Fn(&str) -> Option<String> {
        move |k| vars.iter().find(|(n, _)| *n == k).map(|(_, v)| v.clone())
    }

    /// Faux processus de fond : accueille, glisse des messages à ignorer, répond, puis
    /// rend les messages reçus du client (jusqu'à sa fermeture).
    fn faux_fond(
        dossier: &Path,
        reponse: Option<Option<&str>>,
        memoriser: bool,
    ) -> (PathBuf, JoinHandle<Vec<ClientMsg>>) {
        let socket = dossier.join("fond.sock");
        let l = UnixListener::bind(&socket).unwrap();
        let reponse_absente = reponse.is_none();
        let reponse = reponse.map(|r| r.map(|t| Zeroizing::new(t.to_string())));
        let h = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut recus = Vec::new();
            recus.push(read_frame::<_, ClientMsg>(&mut s).unwrap()); // Hello
            write_frame(
                &mut s,
                &ServerMsg::Welcome {
                    version: VERSION_ASKPASS,
                },
            )
            .unwrap();
            recus.push(read_frame::<_, ClientMsg>(&mut s).unwrap()); // Demander
            write_frame(&mut s, &ServerMsg::Message("à ignorer".into())).unwrap();
            write_frame(&mut s, &ServerMsg::DemandeClose { demande: 77 }).unwrap();
            if let Some(r) = reponse {
                write_frame(
                    &mut s,
                    &ServerMsg::Reponse {
                        reponse: r,
                        memoriser,
                    },
                )
                .unwrap();
            }
            // Sans réponse : on ferme aussitôt la connexion.
            if reponse_absente {
                return recus;
            }
            while let Ok(m) = read_frame::<_, ClientMsg>(&mut s) {
                recus.push(m);
            }
            recus
        });
        (socket, h)
    }

    fn lancer(
        invite: &str,
        prompt: Option<&str>,
        socket: &Path,
        coffre: &CoffreMemoire,
    ) -> (i32, Vec<u8>) {
        let mut vars = vec![
            ("PUPITRE_SOCKET", socket.to_string_lossy().into_owned()),
            ("PUPITRE_SESSION", "5".to_string()),
        ];
        if let Some(p) = prompt {
            vars.push(("SSH_ASKPASS_PROMPT", p.to_string()));
        }
        let mut sortie = Vec::new();
        let code = executer(&args(invite), &env_de(vars), coffre, &mut sortie);
        (code, sortie)
    }

    fn invite_cle() -> String {
        format!("Enter passphrase for {CLE}: ")
    }

    #[test]
    fn sans_socket_code_1() {
        let c = CoffreMemoire::default();
        let mut sortie = Vec::new();
        let code = executer(&args("Password: "), &env_de(vec![]), &c, &mut sortie);
        assert_eq!(code, 1);
        assert!(sortie.is_empty());
        // Socket injoignable.
        let t = tempfile::tempdir().unwrap();
        let (code, sortie) = lancer("Password: ", None, &t.path().join("absent.sock"), &c);
        assert_eq!(code, 1);
        assert!(sortie.is_empty());
    }

    #[test]
    fn refus_rien_sur_stdout() {
        let t = tempfile::tempdir().unwrap();
        let c = CoffreMemoire::default();
        let (socket, h) = faux_fond(t.path(), Some(None), false);
        let (code, sortie) = lancer(&invite_cle(), None, &socket, &c);
        assert_eq!(code, 1);
        assert!(sortie.is_empty());
        h.join().unwrap();
        // Connexion fermée avant toute réponse.
        let t2 = tempfile::tempdir().unwrap();
        let (socket, h) = faux_fond(t2.path(), None, false);
        let (code, sortie) = lancer(&invite_cle(), None, &socket, &c);
        assert_eq!(code, 1);
        assert!(sortie.is_empty());
        let _ = std::fs::remove_file(&socket);
        h.join().unwrap();
    }

    /// Coffre dont le trousseau est lent à répondre.
    struct CoffreLent;
    impl Coffre for CoffreLent {
        fn etat(&self) -> EtatTrousseau {
            EtatTrousseau::Lent
        }
        fn lire(
            &self,
            _: &Path,
        ) -> Result<Option<Zeroizing<String>>, crate::trousseau::ErreurTrousseau> {
            Err(crate::trousseau::ErreurTrousseau::Delai)
        }
        fn ecrire(&self, _: &Path, _: &str) -> Result<(), crate::trousseau::ErreurTrousseau> {
            Err(crate::trousseau::ErreurTrousseau::Delai)
        }
        fn oublier(&self, _: &Path) -> Result<(), crate::trousseau::ErreurTrousseau> {
            Err(crate::trousseau::ErreurTrousseau::Delai)
        }
        fn cles(&self) -> Vec<PathBuf> {
            Vec::new()
        }
    }

    #[test]
    fn trousseau_lent_transmis_a_un_fond_v3_et_jamais_memorise() {
        let t = tempfile::tempdir().unwrap();
        let (socket, h) = faux_fond(t.path(), Some(Some("p")), true);
        let mut vars = vec![("PUPITRE_SOCKET", socket.to_string_lossy().into_owned())];
        vars.push(("PUPITRE_SESSION", "5".to_string()));
        let mut sortie = Vec::new();
        let code = executer(
            &args(&invite_cle()),
            &env_de(vars),
            &CoffreLent,
            &mut sortie,
        );
        assert_eq!(code, 0);
        assert_eq!(sortie, b"p\n");
        let recus = h.join().unwrap();
        assert!(matches!(
            &recus[1],
            ClientMsg::Demander {
                trousseau: EtatTrousseau::Lent,
                ..
            }
        ));
        // Pas de chargement dans l'agent : rien n'a été mémorisé.
        assert_eq!(recus.len(), 2);
    }

    /// Fond en version 2 : refuse `Hello { 3 }` puis accepte `Hello { 2 }` ; il ne doit
    /// jamais voir l'état « lent ».
    #[test]
    fn repli_v2_face_a_un_fond_ancien() {
        let t = tempfile::tempdir().unwrap();
        let socket = t.path().join("fond.sock");
        let l = UnixListener::bind(&socket).unwrap();
        let h = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            assert_eq!(
                read_frame::<_, ClientMsg>(&mut s).unwrap(),
                ClientMsg::Hello { version: 3 }
            );
            write_frame(&mut s, &ServerMsg::Error("version incompatible".into())).unwrap();
            drop(s);
            let (mut s, _) = l.accept().unwrap();
            assert_eq!(
                read_frame::<_, ClientMsg>(&mut s).unwrap(),
                ClientMsg::Hello { version: 2 }
            );
            write_frame(&mut s, &ServerMsg::Welcome { version: 2 }).unwrap();
            let demande = read_frame::<_, ClientMsg>(&mut s).unwrap();
            write_frame(
                &mut s,
                &ServerMsg::Reponse {
                    reponse: Some(Zeroizing::new("p".to_string())),
                    memoriser: false,
                },
            )
            .unwrap();
            demande
        });
        let vars = vec![("PUPITRE_SOCKET", socket.to_string_lossy().into_owned())];
        let mut sortie = Vec::new();
        let code = executer(
            &args(&invite_cle()),
            &env_de(vars),
            &CoffreLent,
            &mut sortie,
        );
        assert_eq!(code, 0);
        assert_eq!(sortie, b"p\n");
        assert!(matches!(
            h.join().unwrap(),
            ClientMsg::Demander {
                trousseau: EtatTrousseau::Absent,
                ..
            }
        ));
    }

    #[test]
    fn phrase_ecrite_et_memorisee() {
        let t = tempfile::tempdir().unwrap();
        let c = CoffreMemoire::default();
        let (socket, h) = faux_fond(t.path(), Some(Some("s3cret")), true);
        let (code, sortie) = lancer(&invite_cle(), None, &socket, &c);
        assert_eq!(code, 0);
        assert_eq!(sortie, b"s3cret\n");
        assert_eq!(
            c.lire(Path::new(CLE))
                .unwrap()
                .as_deref()
                .map(String::as_str),
            Some("s3cret")
        );
        let recus = h.join().unwrap();
        assert_eq!(
            recus[0],
            ClientMsg::Hello {
                version: VERSION_ASKPASS
            }
        );
        assert_eq!(
            recus[1],
            ClientMsg::Demander {
                session: Some(5),
                invite: invite_cle(),
                genre: Genre::PhraseCle {
                    cle: CLE.into(),
                    nouvel_essai: false
                },
                trousseau: EtatTrousseau::Disponible,
            }
        );
        assert_eq!(recus[2], ClientMsg::ChargerCle { cle: CLE.into() });
    }

    #[test]
    fn phrase_non_memorisee_pas_de_chargement() {
        let t = tempfile::tempdir().unwrap();
        let c = CoffreMemoire::default();
        let (socket, h) = faux_fond(t.path(), Some(Some("abc")), false);
        let (code, sortie) = lancer(&invite_cle(), None, &socket, &c);
        assert_eq!((code, sortie.as_slice()), (0, &b"abc\n"[..]));
        assert!(c.lire(Path::new(CLE)).unwrap().is_none());
        assert_eq!(h.join().unwrap().len(), 2, "ni écriture ni ChargerCle");
    }

    #[test]
    fn texte_libre_ecrit_tel_quel() {
        let t = tempfile::tempdir().unwrap();
        let c = CoffreMemoire::default();
        let (socket, h) = faux_fond(t.path(), Some(Some("mdp")), true);
        let (code, sortie) = lancer("user@hote's password: ", None, &socket, &c);
        assert_eq!((code, sortie.as_slice()), (0, &b"mdp\n"[..]));
        let recus = h.join().unwrap();
        assert_eq!(recus.len(), 2, "pas de mémorisation hors phrase de clé");
        assert!(matches!(
            &recus[1],
            ClientMsg::Demander {
                trousseau: EtatTrousseau::Absent,
                ..
            }
        ));
    }

    #[test]
    fn confirmation_codes() {
        let t = tempfile::tempdir().unwrap();
        let c = CoffreMemoire::default();
        for (rep, code_attendu) in [
            (Some(Some("yes")), 0),
            (Some(Some("no")), 1),
            (Some(None), 1),
            (None, 1),
        ] {
            let (socket, h) = faux_fond(t.path(), rep, false);
            let (code, sortie) = lancer("Allow use of key k?", Some("confirm"), &socket, &c);
            assert_eq!(code, code_attendu, "{rep:?}");
            assert!(sortie.is_empty(), "confirmation : rien sur stdout");
            let _ = std::fs::remove_file(&socket);
            h.join().unwrap();
        }
    }

    #[test]
    fn mode_trousseau_rend_la_phrase() {
        let c = CoffreMemoire::default();
        c.ecrire(Path::new(CLE), "enregistree").unwrap();
        let env = env_de(vec![("PUPITRE_ASKPASS_MODE", "trousseau".to_string())]);
        let mut sortie = Vec::new();
        // Aucune variable de socket : jamais de connexion.
        assert_eq!(executer(&args(&invite_cle()), &env, &c, &mut sortie), 0);
        assert_eq!(sortie, b"enregistree\n");
        // Clé sans phrase, et genre autre : code 1.
        let mut sortie = Vec::new();
        let autre = args("Enter passphrase for /home/u/.ssh/autre: ");
        assert_eq!(executer(&autre, &env, &c, &mut sortie), 1);
        assert!(sortie.is_empty());
        assert_eq!(executer(&args("Password: "), &env, &c, &mut sortie), 1);
        assert!(sortie.is_empty());
    }

    #[test]
    fn mode_trousseau_nouvel_essai_oublie() {
        let c = CoffreMemoire::default();
        c.ecrire(Path::new(CLE), "fausse").unwrap();
        let env = env_de(vec![("PUPITRE_ASKPASS_MODE", "trousseau".to_string())]);
        let mut sortie = Vec::new();
        let invite = format!("Bad passphrase, try again for {CLE}: ");
        assert_eq!(executer(&args(&invite), &env, &c, &mut sortie), 1);
        assert!(sortie.is_empty());
        assert!(c.lire(Path::new(CLE)).unwrap().is_none(), "entrée oubliée");
    }
}
