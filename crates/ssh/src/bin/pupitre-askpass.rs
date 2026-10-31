// SPDX-License-Identifier: GPL-3.0-or-later
//! Programme appelé par `ssh`, `ssh-add` ou git pour demander une phrase de passe.
//! Toute la logique est dans `pupitre_ssh::askpass`.

use pupitre_ssh::askpass::executer;
use pupitre_ssh::trousseau::CoffreSecretService;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let coffre = CoffreSecretService::ouvrir();
    let code = executer(
        &args,
        &|nom| std::env::var(nom).ok(),
        &coffre,
        &mut std::io::stdout().lock(),
    );
    std::process::exit(code);
}
