// SPDX-License-Identifier: GPL-3.0-or-later
//! Surveillance de fichiers (configuration, thème) pour le rechargement à chaud.
//!
//! Le dossier parent est surveillé, pas le fichier : les éditeurs qui enregistrent en
//! écrivant un fichier voisin puis en le renommant sont ainsi suivis, et la surveillance
//! survit au remplacement. Les événements sont regroupés sur 200 ms.

use gtk::glib;
use notify::{RecursiveMode, Watcher};
use pupitre_config::{charger, Config, ErreurConfig};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Durée de regroupement des événements d'un même enregistrement.
pub const REGROUPEMENT: Duration = Duration::from_millis(200);

/// Surveillance active tant que cette valeur existe.
pub struct Surveillance {
    _observateur: notify::RecommendedWatcher,
}

/// Rappelle `rappel` avec la configuration relue (ou l'erreur) après chaque changement.
pub fn surveiller(
    chemin: &Path,
    rappel: impl Fn(Result<Config, ErreurConfig>) + 'static,
) -> Result<Surveillance, String> {
    let c = chemin.to_path_buf();
    surveiller_fichiers(&[chemin.to_path_buf()], move || rappel(charger(&c)))
}

/// Rappelle `rappel` (sur la boucle GTK) après tout changement d'un des fichiers.
pub fn surveiller_fichiers(
    chemins: &[PathBuf],
    rappel: impl Fn() + 'static,
) -> Result<Surveillance, String> {
    let cibles: Vec<PathBuf> = chemins.to_vec();
    let (tx, rx) = async_channel::unbounded::<()>();
    let filtre = cibles.clone();
    let mut observateur = notify::recommended_watcher(move |e: notify::Result<notify::Event>| {
        if let Ok(e) = e {
            if e.paths.iter().any(|p| concerne(p, &filtre)) {
                let _ = tx.send_blocking(());
            }
        }
    })
    .map_err(|e| format!("surveillance des fichiers impossible : {e}"))?;
    let mut dossiers: Vec<&Path> = cibles.iter().filter_map(|c| c.parent()).collect();
    dossiers.sort();
    dossiers.dedup();
    for d in dossiers {
        observateur
            .watch(d, RecursiveMode::NonRecursive)
            .map_err(|e| format!("surveillance de {} impossible : {e}", d.display()))?;
    }
    glib::spawn_future_local(async move {
        // S'arrête quand l'observateur (donc l'émetteur) disparaît.
        while rx.recv().await.is_ok() {
            glib::timeout_future(REGROUPEMENT).await;
            while rx.try_recv().is_ok() {}
            rappel();
        }
    });
    Ok(Surveillance {
        _observateur: observateur,
    })
}

/// Vrai si l'événement porte sur l'un des fichiers surveillés.
fn concerne(p: &Path, cibles: &[PathBuf]) -> bool {
    cibles
        .iter()
        .any(|c| p == c || (p.file_name() == c.file_name() && p.parent() == c.parent()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filtre_les_fichiers() {
        let c = vec![PathBuf::from("/a/b/pupitre.toml")];
        assert!(concerne(Path::new("/a/b/pupitre.toml"), &c));
        assert!(!concerne(Path::new("/a/b/autre.toml"), &c));
        assert!(!concerne(Path::new("/a/pupitre.toml"), &c));
    }
}
