// SPDX-License-Identifier: GPL-3.0-or-later
//! Binaire pupitre-fond : possède les shells, lancé à la demande par l'interface.
//! Usage : `pupitre-fond [--socket CHEMIN]`.

use pupitre_fond::serveur::{run, socket_path, ARRET_SI_INACTIF};
use pupitre_i18n::{init, langues_demandees, t, ta, ArgI18n};
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    init(&langues_demandees(&|n| std::env::var(n).ok()));
    let mut usage = t("fond-usage");
    let mut args = std::env::args_os().skip(1);
    let mut socket = None;
    while let Some(a) = args.next() {
        match a.to_str() {
            Some("--socket") => match args.next() {
                Some(p) => socket = Some(PathBuf::from(p)),
                None => {
                    eprintln!("{}\n{usage}", t("fond-socket-attend-chemin"));
                    return ExitCode::from(2);
                }
            },
            Some("--langue") => match args.next().and_then(|l| l.into_string().ok()) {
                Some(l) => {
                    if let Ok(id) = l.parse() {
                        init(&[id]);
                        usage = t("fond-usage");
                    }
                }
                None => {
                    eprintln!("{}\n{usage}", t("fond-langue-attend-code"));
                    return ExitCode::from(2);
                }
            },
            Some("--help" | "-h") => {
                println!("{usage}");
                return ExitCode::SUCCESS;
            }
            _ => {
                eprintln!(
                    "{}\n{usage}",
                    ta(
                        "fond-option-inconnue",
                        &[("option", ArgI18n::from(format!("{a:?}")))]
                    )
                );
                return ExitCode::from(2);
            }
        }
    }
    let socket = socket.unwrap_or_else(socket_path);
    match run(&socket, ARRET_SI_INACTIF) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!(
                "{}",
                ta(
                    "fond-erreur",
                    &[
                        ("socket", ArgI18n::from(socket.display().to_string())),
                        ("erreur", ArgI18n::from(e.to_string()))
                    ]
                )
            );
            ExitCode::FAILURE
        }
    }
}
