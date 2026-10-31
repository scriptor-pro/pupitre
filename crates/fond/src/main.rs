// SPDX-License-Identifier: GPL-3.0-or-later
//! Binaire pupitre-fond : possède les shells, lancé à la demande par l'interface.
//! Usage : `pupitre-fond [--socket CHEMIN]`.

use pupitre_fond::serveur::{run, socket_path, ARRET_SI_INACTIF};
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage : pupitre-fond [--socket CHEMIN]";

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let mut socket = None;
    while let Some(a) = args.next() {
        match a.to_str() {
            Some("--socket") => match args.next() {
                Some(p) => socket = Some(PathBuf::from(p)),
                None => {
                    eprintln!("pupitre-fond : --socket attend un chemin\n{USAGE}");
                    return ExitCode::from(2);
                }
            },
            Some("--help" | "-h") => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            _ => {
                eprintln!("pupitre-fond : option inconnue {a:?}\n{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let socket = socket.unwrap_or_else(socket_path);
    match run(&socket, ARRET_SI_INACTIF) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("pupitre-fond : {} : {e}", socket.display());
            ExitCode::FAILURE
        }
    }
}
