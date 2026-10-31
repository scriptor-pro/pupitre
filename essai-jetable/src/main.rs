// ESSAI JETABLE — mini-terminal GTK4 + VTE pour mesurer ce que VTE 0.70 sait faire.
use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use std::io::Write;
use vte4::prelude::*;

fn rgba(h: &str) -> gdk::RGBA { gdk::RGBA::parse(h).unwrap() }

fn main() -> glib::ExitCode {
    let app = gtk::Application::builder().application_id("fr.pupitre.EssaiJetable")
        .flags(gio::ApplicationFlags::NON_UNIQUE).build();
    app.connect_activate(build);
    app.run_with_args::<&str>(&[])
}

fn build(app: &gtk::Application) {
    let dir = std::env::current_dir().unwrap();
    let out = dir.join("resultats-gtk.txt");
    let win = gtk::ApplicationWindow::builder().application(app).title("Essai VTE")
        .default_width(960).default_height(593).build();
    let term = vte4::Terminal::new();
    term.set_font(Some(&gtk::pango::FontDescription::from_string(&std::env::var("ESSAI_POLICE").unwrap_or("B612 Mono 11".into()))));
    let pal = ["#252630","#f38ba8","#a6e3a1","#f9e2af","#89b4fa","#f5c2e7","#94e2d5","#bac2de",
               "#8a8ea6","#f38ba8","#a6e3a1","#f9e2af","#89b4fa","#f5c2e7","#94e2d5","#b2b8cf"].map(rgba);
    let palr: Vec<&gdk::RGBA> = pal.iter().collect();
    term.set_colors(Some(&rgba("#cdd6f4")), Some(&rgba("#1e1e2e")), &palr);
    term.set_allow_hyperlink(true);
    term.set_cursor_blink_mode(vte4::CursorBlinkMode::Off);
    win.set_child(Some(&term));

    // Signaux propres aux versions « patchées » (Fedora) ou récentes.
    let mut lignes = vec![
        format!("GTK {}.{}.{}", gtk::major_version(), gtk::minor_version(), gtk::micro_version()),
        format!("VTE {}.{}.{}", unsafe { vte4::ffi::vte_get_major_version() }, unsafe { vte4::ffi::vte_get_minor_version() }, unsafe { vte4::ffi::vte_get_micro_version() }),
    ];
    for s in ["notification-received", "shell-precmd", "shell-preexec", "termprop-changed", "termprops-changed", "current-directory-uri-changed", "bell", "hyperlink-hover-uri-changed"] {
        let ok = glib::subclass::signal::SignalId::lookup(s, vte4::Terminal::static_type()).is_some();
        lignes.push(format!("signal {s}: {}", if ok { "OUI" } else { "non" }));
    }
    lignes.push(format!("rôle accessible GTK: {:?}", term.accessible_role()));
    #[cfg(feature = "recent")]
    {
        let seat = gdk::Display::default().unwrap().default_seat().unwrap();
        if let Some(kb) = seat.keyboard() {
            lignes.push(format!("dispositions clavier (API GTK): {:?}, active n°{}", kb.layout_names(), kb.active_layout_index()));
        }
    }
    #[cfg(not(feature = "recent"))]
    lignes.push("dispositions clavier (API GTK): indisponible à la compilation".into());

    // Signaux observés pendant le test
    let cloche = std::rc::Rc::new(std::cell::Cell::new(false));
    { let c = cloche.clone(); term.connect_bell(move |_| c.set(true)); }
    let titre_osc = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
    { let t = titre_osc.clone(); term.connect_window_title_changed(move |tm| { *t.borrow_mut() = tm.window_title().map(|s| s.to_string()).unwrap_or_default(); }); }

    let props = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    #[cfg(feature = "recent")]
    { let pr = props.clone();
      term.connect_local("termprop-changed", false, move |args| {
          if let Ok(n) = args[1].get::<String>() { pr.borrow_mut().push(n); } None }); }
    let script = dir.join("test.sh");
    term.spawn_async(vte4::PtyFlags::DEFAULT, Some(dir.to_str().unwrap()),
        &["/bin/bash", script.to_str().unwrap()], &[], glib::SpawnFlags::DEFAULT,
        || {}, -1, gio::Cancellable::NONE, |r| { if let Err(e) = r { eprintln!("spawn: {e}"); } });

    let win2 = win.clone();
    term.connect_child_exited(move |t, status| {
        let mut l = lignes.clone();
        l.push(format!("statut du script: {status}"));
        l.push(format!("termprops reçues: {:?}", props.borrow()));
        l.push(format!("cloche reçue (signal bell): {}", if cloche.get() { "OUI" } else { "non" }));
        l.push(format!("titre via OSC 0/2: {:?}", titre_osc.borrow()));
        l.push(format!("répertoire courant via OSC 7: {:?}", t.current_directory_uri()));
        // Lien OSC 8 : le script l'a écrit en première ligne visible ; on sonde quelques positions.
        let (cw, ch) = (t.char_width() as f64, t.char_height() as f64);
        let mut lien = None;
        for row in 0..t.row_count() { if let Some(u) = t.check_hyperlink_at(cw * 2.0, ch * (row as f64 + 0.5)) { lien = Some(u.to_string()); break; } }
        l.push(format!("lien OSC 8 détecté: {:?}", lien));
        let mut f = std::fs::File::create(&out).unwrap();
        for x in &l { writeln!(f, "{x}").unwrap(); }
        let w = win2.clone();
        glib::timeout_add_seconds_local_once(1, move || w.close());
    });
    win.present();
}
