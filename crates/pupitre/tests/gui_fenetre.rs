// SPDX-License-Identifier: GPL-3.0-or-later
//! Tests graphiques de la fenêtre (Task 8). Lancés par `scripts/test-x11.sh` et
//! `scripts/test-wayland.sh` (option `--include-ignored`) ; ignorés sinon, pour ne pas
//! ouvrir de fenêtre sur le bureau de l'utilisateur.
//!
//! Un seul processus GTK : les tests s'exécutent l'un après l'autre dans `main`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod commun;

use gtk::glib;
use gtk::prelude::*;
use pupitre::apparence::{suivre_preference_systeme, theme_systeme, Preference};
use pupitre::client::{connect_or_spawn_at, Client};
use pupitre::polices::{dossier_polices, enregistrer_polices_embarquees};
use pupitre::terminal::TerminalOnglet;
use pupitre_config::{contrast_ratio, theme_clair, theme_sombre, Config, Rgb, Theme};
use pupitre_proto::{ClientMsg, ServerMsg, SessionId};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use std::time::{Duration, Instant};
use vte4::prelude::*;

type Resultat = Result<(), String>;

const PORTAIL_NOM: &str = "org.freedesktop.portal.Desktop";
const PORTAIL_CHEMIN: &str = "/org/freedesktop/portal/desktop";
const PORTAIL_INTERFACE: &str = "org.freedesktop.portal.Settings";

fn main() {
    if !std::env::args().any(|a| a == "--include-ignored") {
        println!(
            "tests graphiques ignorés (lancer scripts/test-x11.sh ou scripts/test-wayland.sh)"
        );
        return;
    }
    commun::bus_prive();
    let racine = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    compiler(&racine);

    // Dossier personnel vide : seules les polices du système et celles embarquées par
    // Pupitre sont visibles (la B612 Mono de l'utilisateur ne doit pas fausser le test).
    let perso = tempfile::tempdir().unwrap();
    for (var, sous) in [
        ("HOME", ""),
        ("XDG_DATA_HOME", ".local/share"),
        ("XDG_CONFIG_HOME", ".config"),
        ("XDG_CACHE_HOME", ".cache"),
        ("XDG_STATE_HOME", ".local/state"),
    ] {
        let d = perso.path().join(sous);
        std::fs::create_dir_all(&d).unwrap();
        std::env::set_var(var, &d);
    }
    commun::isoler_ssh(perso.path());

    // Faux portail réservé avant GTK, qui interroge lui aussi le portail au démarrage.
    let portail = FauxPortail::installer();

    let dossier = dossier_polices().expect("dossier des polices embarquées");
    assert!(
        enregistrer_polices_embarquees(&dossier),
        "polices embarquées"
    );
    gtk::init().unwrap();

    let mut echecs = 0;
    let mut lancer = |nom: &str, r: Resultat| match r {
        Ok(()) => println!("test {nom} ... ok"),
        Err(e) => {
            println!("test {nom} ... ÉCHEC : {e}");
            echecs += 1;
        }
    };
    let banc = Banc::nouveau(&racine);
    lancer("police_b612_utilisee", police_b612_utilisee(&banc));
    lancer("curseur_fixe_par_defaut", curseur_fixe_par_defaut(&banc));
    lancer(
        "saisie_et_sortie_passent_par_le_fond",
        saisie_et_sortie(&banc),
    );
    lancer(
        "theme_suit_le_systeme",
        theme_suit_le_systeme(&banc, &portail),
    );
    lancer("texte_attenue_lisible", texte_attenue_lisible(&banc));
    lancer(
        "fenetre_s_ouvre_et_affiche_le_shell",
        fenetre_s_ouvre_et_affiche_le_shell(&racine),
    );
    banc.fermer();
    if echecs > 0 {
        println!("{echecs} test(s) graphique(s) en échec");
        std::process::exit(1);
    }
    println!("tests graphiques : tous réussis");
}

fn compiler(racine: &Path) {
    let ok = Command::new(env!("CARGO"))
        .args([
            "build",
            "-q",
            "-p",
            "pupitre-fond",
            "-p",
            "pupitre",
            "--bins",
        ])
        .current_dir(racine)
        .status()
        .unwrap()
        .success();
    assert!(ok, "compilation des binaires");
    std::env::set_var("PUPITRE_FOND", commun::fond_sans_trousseau());
}

/// Fait tourner la boucle GTK jusqu'à ce que `cond` soit vraie (ou délai écoulé).
fn attendre(delai: Duration, mut cond: impl FnMut() -> bool) -> bool {
    let ctx = glib::MainContext::default();
    let limite = Instant::now() + delai;
    while Instant::now() < limite {
        while ctx.iteration(false) {}
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

/// Processus de fond isolé, un client, et une fenêtre par terminal ouvert.
struct Banc {
    _dossier: tempfile::TempDir,
    client: Rc<Client>,
    recus: Rc<RefCell<Vec<ServerMsg>>>,
}

impl Banc {
    fn nouveau(_racine: &Path) -> Self {
        let dossier = tempfile::tempdir().unwrap();
        let socket = dossier.path().join("pupitre/fond.sock");
        let (client, rx) = connect_or_spawn_at(&socket).unwrap();
        let recus = Rc::new(RefCell::new(Vec::new()));
        let r = recus.clone();
        glib::spawn_future_local(async move {
            while let Ok(m) = rx.recv().await {
                r.borrow_mut().push(m);
            }
        });
        Banc {
            _dossier: dossier,
            client: Rc::new(client),
            recus,
        }
    }

    /// Ouvre une session `/bin/sh` et l'affiche dans une fenêtre ; la sortie est relayée.
    fn ouvrir(&self, theme: &Theme) -> (TerminalOnglet, gtk::Window) {
        self.client
            .send(ClientMsg::Open {
                argv: Some(vec!["/bin/sh".into()]),
                cwd: None,
                cols: 80,
                rows: 24,
            })
            .unwrap();
        let mut id: Option<SessionId> = None;
        assert!(attendre(Duration::from_secs(10), || {
            let mut r = self.recus.borrow_mut();
            if let Some(p) = r.iter().position(|m| matches!(m, ServerMsg::Opened { .. })) {
                if let ServerMsg::Opened { id: i } = r.remove(p) {
                    id = Some(i);
                }
            }
            id.is_some()
        }));
        let id = id.unwrap();
        let onglet = TerminalOnglet::new(self.client.clone(), id, &Config::default(), theme);
        let fenetre = gtk::Window::new();
        fenetre.set_default_size(800, 500);
        fenetre.set_child(Some(onglet.widget()));
        fenetre.present();
        let t = onglet.terminal().clone();
        assert!(attendre(Duration::from_secs(5), || t.is_mapped()));
        (onglet, fenetre)
    }

    /// Relaie vers `onglet` la sortie reçue pour sa session.
    fn relayer(&self, onglet: &TerminalOnglet, id: SessionId) {
        let mut r = self.recus.borrow_mut();
        r.retain(|m| match m {
            ServerMsg::Output { id: i, data } if *i == id => {
                onglet.on_output(data);
                false
            }
            _ => true,
        });
    }

    fn fermer(&self) {
        let _ = self.client.send(ClientMsg::List);
        let mut ids = Vec::new();
        attendre(Duration::from_secs(5), || {
            let r = self.recus.borrow();
            match r.iter().rev().find(|m| matches!(m, ServerMsg::Sessions(_))) {
                Some(ServerMsg::Sessions(v)) => {
                    ids = v.iter().map(|s| s.id).collect();
                    true
                }
                _ => false,
            }
        });
        for id in ids {
            let _ = self.client.send(ClientMsg::Close { id });
        }
        attendre(Duration::from_millis(300), || false);
    }
}

fn texte(t: &vte4::Terminal) -> String {
    t.text_format(vte4::Format::Text)
        .map(|s| s.to_string())
        .unwrap_or_default()
}

fn police_b612_utilisee(banc: &Banc) -> Resultat {
    let (onglet, fenetre) = banc.ouvrir(&theme_sombre());
    let t = onglet.terminal();
    let demandee = t.font().ok_or("aucune police réglée")?;
    let police = t
        .pango_context()
        .load_font(&demandee)
        .ok_or("pango ne charge aucune police")?;
    let famille = police
        .describe()
        .family()
        .map(|f| f.to_string())
        .unwrap_or_default();
    fenetre.destroy();
    if famille == "B612 Mono" {
        Ok(())
    } else {
        Err(format!("police effective « {famille} »"))
    }
}

fn curseur_fixe_par_defaut(banc: &Banc) -> Resultat {
    let (onglet, fenetre) = banc.ouvrir(&theme_sombre());
    let mode = onglet.terminal().cursor_blink_mode();
    fenetre.destroy();
    if mode == vte4::CursorBlinkMode::Off {
        Ok(())
    } else {
        Err(format!("mode de clignotement {mode:?}"))
    }
}

/// Une frappe passe par `commit` → fond → shell, et la sortie revient par `feed`.
/// Le changement de taille de la fenêtre est transmis au PTY.
fn saisie_et_sortie(banc: &Banc) -> Resultat {
    let (onglet, fenetre) = banc.ouvrir(&theme_sombre());
    let t = onglet.terminal().clone();
    let id = onglet.id();
    t.feed_child(b"echo \"$((6*7))-OK\"; stty size\n");
    let ok = attendre(Duration::from_secs(10), || {
        banc.relayer(&onglet, id);
        texte(&t).contains("42-OK")
    });
    if !ok {
        fenetre.destroy();
        return Err(format!("sortie attendue absente : {:?}", texte(&t)));
    }
    // Le compositeur peut encore ajuster la fenêtre juste après son affichage : on
    // redemande la taille au shell jusqu'à ce qu'elle corresponde à la grille du moment.
    let mut attendu = String::new();
    let ok = (0..10).any(|n| {
        attendu = format!("{} {}", t.row_count(), t.column_count());
        t.feed_child(format!("echo T{n}=$(stty size)\n").as_bytes());
        let marque = format!("T{n}={attendu}");
        attendre(Duration::from_secs(2), || {
            banc.relayer(&onglet, id);
            texte(&t).contains(&marque)
        })
    });
    let ecran = texte(&t);
    fenetre.destroy();
    if ok {
        Ok(())
    } else {
        Err(format!("stty size ≠ « {attendu} » : {ecran:?}"))
    }
}

fn rgb_de(c: &gtk::gdk::RGBA) -> Rgb {
    let v = |x: f32| (x * 255.0).round() as u8;
    Rgb {
        r: v(c.red()),
        g: v(c.green()),
        b: v(c.blue()),
    }
}

fn theme_suit_le_systeme(banc: &Banc, portail: &FauxPortail) -> Resultat {
    portail.regler(2); // clair
    let (onglet, fenetre) = banc.ouvrir(&theme_sombre());
    let onglet = Rc::new(onglet);
    let o = onglet.clone();
    let vues = Rc::new(RefCell::new(Vec::new()));
    let v = vues.clone();
    suivre_preference_systeme(move |p| {
        v.borrow_mut().push(p);
        o.appliquer(&Config::default(), &theme_systeme(p));
    });
    let t = onglet.terminal().clone();
    let clair = theme_clair().fond;
    if !attendre(Duration::from_secs(5), || {
        rgb_de(&t.color_background_for_draw()) == clair
    }) {
        fenetre.destroy();
        return Err(format!(
            "pas de thème clair ; préférences vues {:?}",
            vues.borrow()
        ));
    }
    portail.changer(1); // sombre
    let sombre = theme_sombre().fond;
    let ok = attendre(Duration::from_secs(5), || {
        rgb_de(&t.color_background_for_draw()) == sombre
    });
    fenetre.destroy();
    if ok && vues.borrow().as_slice() == [Preference::Clair, Preference::Sombre] {
        Ok(())
    } else {
        Err(format!(
            "après SettingChanged : préférences vues {:?}",
            vues.borrow()
        ))
    }
}

/// Rend le terminal dans une texture et renvoie ses pixels (RGBA, largeur, hauteur).
fn capture(t: &vte4::Terminal) -> Option<(Vec<u8>, usize, usize)> {
    let (w, h) = (t.width(), t.height());
    let paintable = gtk::WidgetPaintable::new(Some(t));
    let snapshot = gtk::Snapshot::new();
    paintable.snapshot(&snapshot, f64::from(w), f64::from(h));
    let noeud = snapshot.to_node()?;
    let renderer = t.native()?.renderer()?;
    let texture = renderer.render_texture(
        &noeud,
        Some(&gtk::graphene::Rect::new(0.0, 0.0, w as f32, h as f32)),
    );
    let mut dl = gtk::gdk::TextureDownloader::new(&texture);
    dl.set_format(gtk::gdk::MemoryFormat::R8g8b8a8);
    let (octets, pas) = dl.download_bytes();
    let (lw, lh) = (texture.width() as usize, texture.height() as usize);
    let mut px = Vec::with_capacity(lw * lh * 4);
    for y in 0..lh {
        px.extend_from_slice(&octets[y * pas..y * pas + lw * 4]);
    }
    Some((px, lw, lh))
}

/// Le texte atténué (SGR 2) garde un contraste ≥ 4,5:1 avec chaque thème, pour la couleur
/// du texte et pour chaque couleur de la palette qui sert au texte. Le pavé plein (U+2588)
/// est dessiné par VTE en aplat de la couleur du texte : sa couleur rendue est exacte.
fn texte_attenue_lisible(banc: &Banc) -> Resultat {
    let mut erreurs = Vec::new();
    for theme in [theme_sombre(), theme_clair()] {
        let (onglet, fenetre) = banc.ouvrir(&theme);
        let t = onglet.terminal().clone();
        // Ligne 0 : couleur du texte ; ligne 1 + i : couleur i de la palette.
        let mut lignes: Vec<(String, Rgb)> = vec![("texte".into(), theme.texte)];
        let mut flux = b"\x1b[2J\x1b[H\x1b[?25l\x1b[2m\xe2\x96\x88\xe2\x96\x88\xe2\x96\x88\xe2\x96\x88 texte\x1b[0m\r\n".to_vec();
        for (i, &c) in theme.palette.iter().enumerate() {
            let sgr = if i < 8 { 30 + i } else { 90 + i - 8 };
            lignes.push((format!("couleur {i}"), c));
            flux.extend(
                format!("\x1b[2;{sgr}m\u{2588}\u{2588}\u{2588}\u{2588} c{i}\x1b[0m\r\n").bytes(),
            );
        }
        // Par le chemin normal de la sortie d'une session.
        onglet.on_output(&flux);
        // VTE traite la sortie et redessine plus tard : attendre que tout soit là.
        let affiche = attendre(Duration::from_secs(5), || texte(&t).contains("c15"));
        attendre(Duration::from_millis(300), || false);
        let (cw, ch) = (t.char_width() as usize, t.char_height() as usize);
        let image = capture(&t);
        fenetre.destroy();
        if !affiche {
            erreurs.push(format!("{} : texte jamais affiché", theme.nom));
            continue;
        }
        let Some((px, w, h)) = image else {
            erreurs.push(format!("{} : capture impossible", theme.nom));
            continue;
        };
        for (n, (nom, couleur)) in lignes.iter().enumerate() {
            // Une couleur de rôle « fond » ne sert pas au texte : pas d'exigence.
            if n > 0 && theme.roles[n - 1] == pupitre_config::Role::Fond {
                continue;
            }
            // Intérieur des 4 cellules du pavé (sans les bords, lissés éventuellement).
            let mut compte: std::collections::HashMap<(u8, u8, u8), usize> = Default::default();
            for y in (n * ch + 1)..((n + 1) * ch).saturating_sub(1).min(h) {
                for x in 1..(4 * cw).saturating_sub(1).min(w) {
                    let i = (y * w + x) * 4;
                    *compte.entry((px[i], px[i + 1], px[i + 2])).or_default() += 1;
                }
            }
            let Some(((r, g, b), _)) = compte.into_iter().max_by_key(|&(_, k)| k) else {
                erreurs.push(format!("{} {nom} : rien mesuré", theme.nom));
                continue;
            };
            let rendu = Rgb { r, g, b };
            let ratio = contrast_ratio(rendu, theme.fond);
            println!(
                "  {} {nom} : #{:02x}{:02x}{:02x} atténué en #{r:02x}{g:02x}{b:02x}, {ratio:.2}:1",
                theme.nom, couleur.r, couleur.g, couleur.b
            );
            if ratio < 4.5 {
                erreurs.push(format!("{} {nom} : {ratio:.2}:1", theme.nom));
            }
        }
    }
    if erreurs.is_empty() {
        Ok(())
    } else {
        Err(erreurs.join(" ; "))
    }
}

/// Bout en bout : le vrai binaire `pupitre` s'ouvre, lance le shell de connexion et le
/// lecteur d'écran (sonde AT-SPI) lit l'invite dans un rôle « terminal ».
fn fenetre_s_ouvre_et_affiche_le_shell(racine: &Path) -> Resultat {
    let runtime = tempfile::tempdir().unwrap();
    let mut cmd = Command::new(racine.join("target/debug/pupitre"));
    cmd.env("XDG_RUNTIME_DIR", runtime.path())
        .stdout(Stdio::null());
    // Le socket Wayland est relatif à l'ancien XDG_RUNTIME_DIR : le rendre absolu.
    if let (Some(rt), Some(wd)) = (
        std::env::var_os("XDG_RUNTIME_DIR"),
        std::env::var_os("WAYLAND_DISPLAY"),
    ) {
        let wd = PathBuf::from(wd);
        if wd.is_relative() {
            cmd.env("WAYLAND_DISPLAY", PathBuf::from(rt).join(wd));
        }
    }
    let mut appli = cmd.spawn().map_err(|e| e.to_string())?;
    let hote = std::fs::read_to_string("/proc/sys/kernel/hostname")
        .unwrap_or_default()
        .trim()
        .to_string();
    let sonde = commun::sonde_a11y();
    let mut sortie = String::new();
    let limite = Instant::now() + Duration::from_secs(20);
    let mut trouve = false;
    while Instant::now() < limite && !trouve {
        if let Ok(o) = Command::new("python3")
            .arg(&sonde)
            .args(["--app", "pupitre"])
            .output()
        {
            sortie = String::from_utf8_lossy(&o.stdout).into_owned();
            trouve = o.status.success() && sortie.contains(&hote) && sortie.contains('$');
        }
        if !trouve {
            std::thread::sleep(Duration::from_millis(300));
        }
    }
    arreter(&mut appli);
    // Ferme les sessions laissées dans le processus de fond isolé (il s'arrête ensuite).
    if let Ok((c, rx)) = connect_or_spawn_at(&runtime.path().join("pupitre/fond.sock")) {
        let _ = c.send(ClientMsg::List);
        let limite = Instant::now() + Duration::from_secs(3);
        while Instant::now() < limite {
            match rx.try_recv() {
                Ok(ServerMsg::Sessions(v)) => {
                    for s in v {
                        let _ = c.send(ClientMsg::Close { id: s.id });
                    }
                    std::thread::sleep(Duration::from_millis(200));
                    break;
                }
                _ => std::thread::sleep(Duration::from_millis(20)),
            }
        }
    }
    if trouve {
        Ok(())
    } else {
        Err(format!(
            "invite (« {hote} », « $ ») non lue par la sonde : {sortie}"
        ))
    }
}

fn arreter(appli: &mut Child) {
    let _ = appli.kill();
    let _ = appli.wait();
}

/// Faux `org.freedesktop.portal.Settings` sur le bus de session de test.
struct FauxPortail {
    connexion: gtk::gio::DBusConnection,
    valeur: Rc<std::cell::Cell<u32>>,
}

impl FauxPortail {
    fn installer() -> Self {
        let connexion =
            gtk::gio::bus_get_sync(gtk::gio::BusType::Session, gtk::gio::Cancellable::NONE)
                .expect("bus de session (lancer sous dbus-run-session)");
        // Réservation synchrone du nom, avant que GTK ne démarre le vrai portail.
        let r = connexion
            .call_sync(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
                "RequestName",
                Some(&(PORTAIL_NOM, 4u32).to_variant()), // DO_NOT_QUEUE
                Some(glib::VariantTy::new("(u)").unwrap()),
                gtk::gio::DBusCallFlags::NONE,
                -1,
                gtk::gio::Cancellable::NONE,
            )
            .expect("RequestName");
        assert_eq!(r.get::<(u32,)>(), Some((1,)), "nom du portail déjà pris");
        let xml = r#"<node><interface name="org.freedesktop.portal.Settings">
            <method name="ReadOne"><arg type="s" direction="in"/><arg type="s" direction="in"/>
              <arg type="v" direction="out"/></method>
            <signal name="SettingChanged"><arg type="s"/><arg type="s"/><arg type="v"/></signal>
            <property name="version" type="u" access="read"/>
          </interface></node>"#;
        let info = gtk::gio::DBusNodeInfo::for_xml(xml).unwrap();
        let iface = info.lookup_interface(PORTAIL_INTERFACE).unwrap();
        let valeur = Rc::new(std::cell::Cell::new(0u32));
        let v = valeur.clone();
        connexion
            .register_object(PORTAIL_CHEMIN, &iface)
            .method_call(move |_, _, _, _, methode, params, invocation| {
                let (ns, cle) = params.get::<(String, String)>().unwrap_or_default();
                if methode == "ReadOne"
                    && ns == "org.freedesktop.appearance"
                    && cle == "color-scheme"
                {
                    let rep = glib::Variant::tuple_from_iter([v.get().to_variant().to_variant()]);
                    invocation.return_value(Some(&rep));
                } else {
                    invocation.return_dbus_error(
                        "org.freedesktop.portal.Error.NotFound",
                        "réglage inconnu",
                    );
                }
            })
            .property(|_, _, _, _, _| 2u32.to_variant())
            .build()
            .unwrap();
        FauxPortail { connexion, valeur }
    }

    fn regler(&self, v: u32) {
        self.valeur.set(v);
    }

    fn changer(&self, v: u32) {
        self.valeur.set(v);
        let _ = self.connexion.emit_signal(
            None,
            PORTAIL_CHEMIN,
            PORTAIL_INTERFACE,
            "SettingChanged",
            Some(&("org.freedesktop.appearance", "color-scheme", v.to_variant()).to_variant()),
        );
    }
}
