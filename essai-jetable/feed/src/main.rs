// ESSAI JETABLE (Task 0) — un vte4::Terminal sans spawn : le programme possède le PTY,
// passe les octets lus à feed() et renvoie le signal commit vers le PTY.
use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::cell::RefCell;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use vte4::prelude::*;
use gtk::subclass::prelude::ObjectSubclassIsExt;

// Conteneur autour de vte4::Terminal : VTE n'a pas de propriété « column-count » à surveiller,
// et dériver vte4::Terminal fait planter vte4-rs 0.9/0.10 (double libération dans le
// trampoline de commit). Le conteneur intercepte donc size_allocate pour son enfant.
mod imp {
    use gtk::glib;
    use std::cell::{Cell, OnceCell, RefCell};
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;

    #[derive(Default)]
    pub struct CadreTerminal {
        pub term: OnceCell<vte4::Terminal>,
        pub grille: Cell<(i64, i64)>,
        pub sur_taille: RefCell<Option<Box<dyn Fn(u16, u16)>>>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for CadreTerminal {
        const NAME: &'static str = "EssaiCadreTerminal";
        type Type = super::CadreTerminal;
        type ParentType = gtk::Widget;
    }
    impl ObjectImpl for CadreTerminal {
        fn constructed(&self) {
            self.parent_constructed();
            let t = vte4::Terminal::new();
            t.set_parent(&*self.obj());
            let _ = self.term.set(t);
        }
        fn dispose(&self) {
            if let Some(t) = self.term.get() { t.unparent(); }
        }
    }
    impl WidgetImpl for CadreTerminal {
        fn measure(&self, o: gtk::Orientation, f: i32) -> (i32, i32, i32, i32) {
            self.term.get().map(|t| t.measure(o, f)).unwrap_or((0, 0, -1, -1))
        }
        fn size_allocate(&self, w: i32, h: i32, b: i32) {
            let Some(t) = self.term.get() else { return };
            t.allocate(w, h, b, None);
            use vte4::prelude::*;
            let g = (t.column_count(), t.row_count());
            if g != self.grille.replace(g) {
                if let Some(f) = self.sur_taille.borrow().as_ref() {
                    f(g.0 as u16, g.1 as u16);
                }
            }
        }
    }
}
glib::wrapper! {
    pub struct CadreTerminal(ObjectSubclass<imp::CadreTerminal>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

fn main() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id("fr.pupitre.EssaiFeed")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(build);
    app.run_with_args::<&str>(&[])
}

fn ajouter(fichier: &Path, ligne: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(fichier) {
        let _ = writeln!(f, "{ligne}");
    }
}

fn build(app: &gtk::Application) {
    let dir = std::env::current_dir().unwrap();
    let sortie: PathBuf = dir.join("sortie");
    let win = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Essai feed")
        .default_width(960)
        .default_height(593)
        .build();
    let tf: CadreTerminal = glib::Object::new();
    let term: vte4::Terminal = tf.imp().term.get().unwrap().clone();
    term.set_font(Some(&gtk::pango::FontDescription::from_string("B612 Mono 11")));
    term.set_cursor_blink_mode(vte4::CursorBlinkMode::Off);
    win.set_child(Some(&tf));

    // PTY géré par le programme, pas par VTE.
    let pair = native_pty_system()
        .openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 })
        .unwrap();
    let mut cmd = CommandBuilder::new("/bin/bash");
    cmd.arg(dir.join("enfant.sh"));
    cmd.cwd(&dir);
    cmd.env("TERM", "xterm-256color");
    let _enfant = pair.slave.spawn_command(cmd).unwrap();
    drop(pair.slave);
    let mut lecteur = pair.master.try_clone_reader().unwrap();
    let ecrivain = Rc::new(RefCell::new(pair.master.take_writer().unwrap()));
    let maitre = Rc::new(pair.master);

    // PTY → feed()
    let (tx, rx) = async_channel::unbounded::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut buf = vec![0u8; 65536];
        loop {
            match lecteur.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send_blocking(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    {
        let t = term.clone();
        let w = win.clone();
        let s = sortie.clone();
        glib::spawn_future_local(async move {
            while let Ok(d) = rx.recv().await {
                t.feed(&d);
            }
            ajouter(&s.join("app.txt"), "fin du PTY (EOF)");
            glib::timeout_add_seconds_local_once(1, move || w.close());
        });
    }

    // commit → PTY (saisie clavier, collage, réponses aux requêtes, souris)
    {
        let e = ecrivain.clone();
        let s = sortie.clone();
        term.connect_commit(move |_, texte, _| {
            ajouter(&s.join("commit.txt"), &format!("{:?}", texte));
            let mut w = e.borrow_mut();
            let _ = w.write_all(texte.as_bytes());
            let _ = w.flush();
        });
    }

    // Taille : VTE calcule colonnes/lignes, on les reporte au PTY.
    {
        let m = maitre.clone();
        let s = sortie.clone();
        *tf.imp().sur_taille.borrow_mut() = Some(Box::new(move |c, r| {
            let res = m.resize(PtySize { rows: r, cols: c, pixel_width: 0, pixel_height: 0 });
            ajouter(&s.join("taille.txt"), &format!("{c}x{r} resize={:?}", res.is_ok()));
        }));
    }

    // Coller : Ctrl+Maj+V (VTE ne fournit pas de raccourci lui-même).
    let touches = gtk::EventControllerKey::new();
    touches.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let t = term.clone();
        touches.connect_key_pressed(move |_, k, _, m| {
            let cs = gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::SHIFT_MASK;
            if m.contains(cs) && (k == gdk::Key::V || k == gdk::Key::v) {
                t.paste_clipboard();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
    }
    term.add_controller(touches);

    // Commandes du pilote : fichier sortie/commande (« ecran » → contenu visible).
    {
        let t = term.clone();
        let s = sortie.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
            let c = s.join("commande");
            if let Ok(txt) = std::fs::read_to_string(&c) {
                let _ = std::fs::remove_file(&c);
                if txt.trim() == "ecran" {
                    let contenu = t.text_format(vte4::Format::Text).unwrap_or_default();
                    let _ = std::fs::write(
                        s.join("ecran.txt"),
                        format!("{}x{}\n{}", t.column_count(), t.row_count(), contenu),
                    );
                }
            }
            glib::ControlFlow::Continue
        });
    }
    win.present();
}
