// SPDX-License-Identifier: GPL-3.0-or-later
//! Tests graphiques : rechargement à chaud de la configuration, messages, police de secours
//! (Task 10). Lancés par `scripts/test-x11.sh` et `scripts/test-wayland.sh` ; ignorés sinon.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod commun;

use commun::*;
use gtk::glib;
use gtk::prelude::*;
use pupitre::fenetre::{FenetrePupitre, Pupitre};
use pupitre::messages::BandeauMessages;
use pupitre::polices::{dossier_polices, enregistrer_polices_embarquees};
use pupitre_config::{charger, theme_clair, theme_sombre, Rgb};
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};
use vte4::prelude::*;

const NOM_APPLI: &str = "pupitre-test-configuration";
/// Délai maximal d'application d'un changement de fichier (plan : < 1 s).
const DELAI: Duration = Duration::from_secs(1);

fn main() {
    if !actives() {
        return;
    }
    bus_prive();
    compiler();
    let perso = isoler_dossiers();
    glib::set_prgname(Some(NOM_APPLI));
    glib::set_application_name(NOM_APPLI);
    // Police d'essai : un vrai TTF libre du système (DejaVu Sans Mono), repéré avant d'isoler
    // fontconfig. Pour que Pupitre ne la voie qu'une fois « installée » par le test, ce
    // processus n'utilise que le dossier de polices de l'utilisateur (vide au départ) et les
    // polices embarquées.
    let source_police = chemin_police_libre();
    let polices_perso = perso.path().join(".local/share/fonts");
    std::fs::create_dir_all(&polices_perso).unwrap();
    let conf = perso.path().join("fonts.conf");
    std::fs::write(
        &conf,
        format!(
            "<?xml version=\"1.0\"?>\n<fontconfig>\n<dir>{}</dir>\n<cachedir>{}</cachedir>\n</fontconfig>\n",
            polices_perso.display(),
            perso.path().join(".cache/fontconfig").display()
        ),
    )
    .unwrap();
    std::env::set_var("FONTCONFIG_FILE", &conf);
    assert!(enregistrer_polices_embarquees(&dossier_polices().unwrap()));
    gtk::init().unwrap();

    let mut ex = Executeur::new();
    ex.lancer("bandeau_temporaire_et_persistant", bandeau);

    let config = perso.path().join(".config/pupitre/pupitre.toml");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::fs::write(&config, "theme = \"sombre\"\n").unwrap();
    let dossier = tempfile::tempdir().unwrap();
    let app = gtk::Application::builder()
        .application_id("fr.pupitre.TestConfiguration")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gtk::gio::Cancellable::NONE).unwrap();
    let p = Pupitre::demarrer(
        &app,
        dossier.path().join("pupitre/fond.sock"),
        charger(&config).unwrap(),
    );
    p.surveiller_config(config.clone());
    let pret = attendre(Duration::from_secs(10), || {
        p.fenetres().first().is_some_and(|f| {
            f.onglets()
                .first()
                .is_some_and(|o| o.terminal().terminal().is_mapped())
        })
    });
    assert!(pret, "pas de fenêtre au démarrage");
    let f = p.fenetres()[0].clone();

    ex.lancer("rechargement_change_les_couleurs", || {
        rechargement(&f, &config)
    });
    ex.lancer("remplacement_du_fichier_suivi", || {
        remplacement(&f, &config)
    });
    ex.lancer("config_invalide_garde_l_ancienne", || invalide(&f, &config));
    ex.lancer("police_inconnue_replie_sur_b612", || police(&f, &config));
    ex.lancer("nouvelle_police_sans_redemarrer", || {
        nouvelle_police(&f, &config, &source_police, &polices_perso)
    });
    ex.lancer("theme_fichier_avec_probleme_applique_et_signale", || {
        theme_fichier(&f, &config, perso.path())
    });
    ex.lancer("nouvelle_fenetre_herite_des_messages", || {
        nouvelle_fenetre(&p, &config)
    });
    fermer_sessions(&dossier.path().join("pupitre/fond.sock"));
    ex.conclure();
}

fn fond(f: &FenetrePupitre) -> Rgb {
    let c = f.onglets()[0]
        .terminal()
        .terminal()
        .color_background_for_draw();
    let v = |x: f32| (x * 255.0).round() as u8;
    Rgb {
        r: v(c.red()),
        g: v(c.green()),
        b: v(c.blue()),
    }
}

/// Écrit le fichier puis mesure le temps jusqu'à ce que `cond` soit vraie.
fn ecrire_et_attendre(
    chemin: &Path,
    contenu: &str,
    cond: impl FnMut() -> bool,
) -> Option<Duration> {
    let debut = Instant::now();
    std::fs::write(chemin, contenu).unwrap();
    attendre(Duration::from_secs(3), cond).then(|| debut.elapsed())
}

fn famille_effective(f: &FenetrePupitre) -> String {
    let t = f.onglets()[0].terminal().terminal().clone();
    t.font()
        .and_then(|d| t.pango_context().load_font(&d))
        .and_then(|p| p.describe().family())
        .map(|f| f.to_string())
        .unwrap_or_default()
}

fn bandeau() -> Resultat {
    let b = BandeauMessages::new();
    let fenetre = gtk::Window::new();
    fenetre.set_child(Some(b.widget()));
    fenetre.present();
    b.afficher("éphémère", Some(Duration::from_millis(300)));
    b.afficher("durable", None);
    b.afficher("durable", None); // pas de doublon
    verifier(b.textes() == ["éphémère", "durable"], || {
        format!("{:?}", b.textes())
    })?;
    let ok = attendre(Duration::from_secs(2), || b.textes() == ["durable"]);
    verifier(ok, || {
        format!("message temporaire resté : {:?}", b.textes())
    })?;
    // Un message persistant a un bouton « Fermer ».
    let fermer = trouver_bouton(b.widget().upcast_ref(), "Fermer").ok_or("pas de bouton Fermer")?;
    fermer.emit_clicked();
    patienter(Duration::from_millis(100));
    verifier(b.textes().is_empty(), || format!("{:?}", b.textes()))?;
    // Un message à clé est remplacé, puis retiré.
    b.poser("config", Some("premier"));
    b.poser("config", Some("second"));
    verifier(b.textes() == ["second"], || format!("{:?}", b.textes()))?;
    b.poser("config", None);
    verifier(b.textes().is_empty(), || format!("{:?}", b.textes()))?;
    fenetre.destroy();
    Ok(())
}

fn etiquettes(racine: &gtk::Widget) -> Vec<gtk::Label> {
    let mut v = Vec::new();
    let mut pile = vec![racine.clone()];
    while let Some(w) = pile.pop() {
        if let Ok(l) = w.clone().downcast::<gtk::Label>() {
            v.push(l);
        }
        let mut enfant = w.first_child();
        while let Some(e) = enfant {
            enfant = e.next_sibling();
            pile.push(e);
        }
    }
    v
}

fn trouver_bouton(racine: &gtk::Widget, etiquette: &str) -> Option<gtk::Button> {
    let mut pile = vec![racine.clone()];
    while let Some(w) = pile.pop() {
        if let Ok(b) = w.clone().downcast::<gtk::Button>() {
            if b.label().is_some_and(|l| l == etiquette) {
                return Some(b);
            }
        }
        let mut enfant = w.first_child();
        while let Some(e) = enfant {
            enfant = e.next_sibling();
            pile.push(e);
        }
    }
    None
}

fn rechargement(f: &FenetrePupitre, config: &Path) -> Resultat {
    verifier(fond(f) == theme_sombre().fond, || {
        format!("fond initial {}", fond(f))
    })?;
    let clair = theme_clair().fond;
    let sombre = theme_sombre().fond;
    // Échauffement non chronométré : sans écran et avec un cache vide, les premiers
    // affichages peuvent occuper la boucle GTK plus d'une seconde (vu une fois : 1,8 s).
    ecrire_et_attendre(config, "theme = \"clair\"\n", || fond(f) == clair)
        .ok_or("échauffement : thème clair non appliqué")?;
    ecrire_et_attendre(config, "theme = \"sombre\"\n", || fond(f) == sombre)
        .ok_or("échauffement : thème sombre non appliqué")?;
    patienter(Duration::from_millis(500));
    let d = ecrire_et_attendre(config, "theme = \"clair\"\n", || fond(f) == clair)
        .ok_or_else(|| format!("fond {} au lieu de {clair}", fond(f)))?;
    println!("  appliqué en {} ms", d.as_millis());
    verifier(d < DELAI, || format!("trop lent : {} ms", d.as_millis()))
}

/// Les éditeurs enregistrent souvent en écrivant un fichier voisin puis en le renommant.
fn remplacement(f: &FenetrePupitre, config: &Path) -> Resultat {
    let voisin = config.with_extension("toml.tmp");
    std::fs::write(&voisin, "theme = \"sombre\"\n").unwrap();
    let debut = Instant::now();
    std::fs::rename(&voisin, config).unwrap();
    let sombre = theme_sombre().fond;
    let ok = attendre(Duration::from_secs(3), || fond(f) == sombre);
    verifier(ok && debut.elapsed() < DELAI, || {
        format!(
            "après renommage : fond {} en {} ms",
            fond(f),
            debut.elapsed().as_millis()
        )
    })?;
    // Et la surveillance continue après le remplacement.
    let clair = theme_clair().fond;
    ecrire_et_attendre(config, "theme = \"clair\"\n", || fond(f) == clair)
        .map(|_| ())
        .ok_or_else(|| "surveillance perdue après le renommage".to_string())
}

fn invalide(f: &FenetrePupitre, config: &Path) -> Resultat {
    let avant = fond(f);
    let ok = ecrire_et_attendre(config, "historique_lignes = \"x\"\n", || {
        f.messages().iter().any(|m| m.contains("ligne 1"))
    });
    verifier(ok.is_some(), || {
        format!("pas de message : {:?}", f.messages())
    })?;
    let m = f.messages().join(" | ");
    verifier(m.contains("Configuration invalide, ligne 1"), || m.clone())?;
    patienter(Duration::from_millis(300));
    verifier(fond(f) == avant, || format!("fond changé : {}", fond(f)))?;
    let (ok, arbre) = sonder_jusqu_a(NOM_APPLI, Duration::from_secs(10), |a| {
        a.contains("Configuration invalide, ligne 1")
    });
    verifier(ok, || {
        format!("message absent de l'arbre AT-SPI :\n{arbre}")
    })?;
    // Fichier corrigé : le message disparaît et la configuration s'applique.
    let sombre = theme_sombre().fond;
    let ok = ecrire_et_attendre(config, "theme = \"sombre\"\n", || {
        fond(f) == sombre && f.messages().is_empty()
    });
    verifier(ok.is_some(), || {
        format!("après correction : {:?}", f.messages())
    })
}

fn police(f: &FenetrePupitre, config: &Path) -> Resultat {
    let ok = ecrire_et_attendre(config, "police = \"Nexiste Pas 13\"\n", || {
        f.messages().iter().any(|m| m.contains("Nexiste Pas"))
    });
    verifier(ok.is_some(), || {
        format!("pas de message : {:?}", f.messages())
    })?;
    let famille = famille_effective(f);
    verifier(famille == "B612 Mono", || {
        format!("famille effective « {famille} »")
    })?;
    let taille = f.onglets()[0]
        .terminal()
        .terminal()
        .font()
        .map(|d| d.size() / gtk::pango::SCALE)
        .unwrap_or(0);
    verifier(taille == 13, || format!("taille {taille} au lieu de 13"))?;
    let ok = ecrire_et_attendre(config, "police = \"B612 Mono 11\"\n", || {
        f.messages().is_empty()
    });
    verifier(ok.is_some(), || {
        format!("message resté : {:?}", f.messages())
    })
}

/// Fichier TTF de DejaVu Sans Mono (libre, très répandue), hors polices embarquées. Rien n'est
/// téléchargé : le test échoue clairement si le système ne l'a pas.
fn chemin_police_libre() -> std::path::PathBuf {
    let o = std::process::Command::new("fc-match")
        .args(["-f", "%{file}", "DejaVu Sans Mono"])
        .output()
        .expect("fc-match");
    let chemin = std::path::PathBuf::from(String::from_utf8_lossy(&o.stdout).into_owned());
    assert!(
        chemin
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("DejaVuSansMono")),
        "DejaVu Sans Mono absente du système ({})",
        chemin.display()
    );
    chemin
}

/// Une police installée pendant que Pupitre tourne est prise au rechargement : fontconfig est
/// relu une fois, sans repli sur B612 Mono et sans redémarrer.
fn nouvelle_police(
    f: &FenetrePupitre,
    config: &Path,
    source: &Path,
    polices_perso: &Path,
) -> Resultat {
    let demande = "police = \"DejaVu Sans Mono 12\"\n";
    // Avant l'installation : introuvable, repli signalé.
    ecrire_et_attendre(config, demande, || {
        f.messages().iter().any(|m| m.contains("DejaVu Sans Mono"))
    })
    .ok_or("police absente non signalée avant l'installation")?;
    let famille = famille_effective(f);
    verifier(famille == "B612 Mono", || {
        format!("pas de repli : famille « {famille} »")
    })?;
    // Installation, puis rechargement de la configuration : la police est prise, sans repli.
    std::fs::copy(source, polices_perso.join("DejaVuSansMono.ttf")).unwrap();
    let ok = ecrire_et_attendre(config, &format!("{demande}# rechargé\n"), || {
        f.messages().is_empty()
    });
    verifier(ok.is_some(), || {
        format!("police installée non prise : {:?}", f.messages())
    })?;
    let famille = famille_effective(f);
    verifier(famille == "DejaVu Sans Mono", || {
        format!("famille effective « {famille} »")
    })
}

fn theme_fichier(f: &FenetrePupitre, config: &Path, perso: &Path) -> Resultat {
    let theme = perso.join("themes/mocha.toml");
    std::fs::create_dir_all(theme.parent().unwrap()).unwrap();
    std::fs::write(&theme, "fond = \"#101010\"\n[couleurs]\n8 = \"#585b70\"\n").unwrap();
    let voulu: Rgb = "#101010".parse().unwrap();
    let ok = ecrire_et_attendre(
        config,
        &format!("theme = \"{}\"\n", theme.display()),
        || {
            fond(f) == voulu
                && f.messages()
                    .iter()
                    .any(|m| m.contains("couleur 8 (#585b70)"))
        },
    );
    verifier(ok.is_some(), || {
        format!("fond {} ; messages {:?}", fond(f), f.messages())
    })?;
    // Le fichier de thème lui-même est surveillé.
    let nouveau: Rgb = "#1e1e2e".parse().unwrap();
    let ok = ecrire_et_attendre(&theme, "fond = \"#1e1e2e\"\n", || {
        fond(f) == nouveau && f.messages().is_empty()
    });
    verifier(ok.is_some(), || {
        format!(
            "thème modifié non appliqué : fond {} ; {:?}",
            fond(f),
            f.messages()
        )
    })?;
    // Thème illisible : le précédent est gardé, message avec la ligne.
    let ok = ecrire_et_attendre(&theme, "fond = \"rouge\"\n", || {
        f.messages().iter().any(|m| m.contains("ligne 1"))
    });
    verifier(ok.is_some(), || format!("{:?}", f.messages()))?;
    verifier(fond(f) == nouveau, || format!("fond changé : {}", fond(f)))
}

fn nouvelle_fenetre(p: &Rc<Pupitre>, config: &Path) -> Resultat {
    std::fs::write(config, "historique_lignes = \"x\"\n").unwrap();
    let premiere = p.fenetres()[0].clone();
    let vu = attendre(Duration::from_secs(3), || {
        premiere
            .messages()
            .iter()
            .any(|m| m.contains("Configuration invalide"))
    });
    verifier(vu, || "première fenêtre sans le message".into())?;
    let g = p.nouvelle_fenetre();
    let m = g.messages().join(" | ");
    let res = verifier(m.contains("Configuration invalide"), || {
        format!("nouvelle fenêtre sans le message : {m:?}")
    })
    .and_then(|()| {
        // Le clavier va au terminal, pas au texte d'un message.
        let ok = attendre(Duration::from_secs(10), || {
            g.onglets().first().is_some_and(|o| {
                o.terminal().terminal().is_mapped()
                    && GtkWindowExt::focus(g.fenetre()).as_ref()
                        == Some(o.terminal().terminal().upcast_ref::<gtk::Widget>())
            })
        });
        // Et il y reste une fois la fenêtre affichée et activée.
        patienter(Duration::from_secs(1));
        let ok = ok
            && g.onglets().first().is_some_and(|o| {
                GtkWindowExt::focus(g.fenetre()).as_ref()
                    == Some(o.terminal().terminal().upcast_ref::<gtk::Widget>())
            });
        // Aucun texte de message ne doit être sélectionné (surbrillance trompeuse).
        let selection = etiquettes(g.fenetre().upcast_ref())
            .iter()
            .filter(|l| l.selection_bounds().is_some())
            .map(|l| l.text().to_string())
            .collect::<Vec<_>>();
        verifier(selection.is_empty(), || {
            format!("texte sélectionné : {selection:?}")
        })?;
        verifier(ok, || {
            format!(
                "focus sur {:?} au lieu du terminal",
                GtkWindowExt::focus(g.fenetre()).map(|w| w.type_().name())
            )
        })
    });
    g.fenetre().close();
    res
}
