// SPDX-License-Identifier: GPL-3.0-or-later
//! Programme appelé par `ssh`, `ssh-add` ou git pour demander une phrase de passe.
//! Toute la logique est dans `pupitre_ssh::askpass`.

use pupitre_ssh::askpass::executer;
use pupitre_ssh::trousseau::CoffreSecretService;
use std::os::fd::AsFd;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let coffre = CoffreSecretService::ouvrir();
    // Sortie sans tampon : un double du descripteur 1 (fichier), pas `stdout()` qui garde
    // les lignes dans un tampon.
    let mut sortie = match std::io::stdout().as_fd().try_clone_to_owned() {
        Ok(fd) => std::fs::File::from(fd),
        Err(e) => {
            eprintln!("pupitre-askpass : sortie standard inutilisable : {e}");
            std::process::exit(1);
        }
    };
    let code = executer(&args, &|nom| std::env::var(nom).ok(), &coffre, &mut sortie);
    std::process::exit(code);
}
