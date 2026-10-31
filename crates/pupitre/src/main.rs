// SPDX-License-Identifier: GPL-3.0-or-later
//! Binaire pupitre : la fenêtre GTK.

// Vérifie que la paire gtk4/vte4 se lie correctement.
use vte4 as _;

fn main() -> gtk::glib::ExitCode {
    gtk::glib::ExitCode::SUCCESS
}
