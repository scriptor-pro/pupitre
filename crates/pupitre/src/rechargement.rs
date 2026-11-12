// SPDX-License-Identifier: GPL-3.0-or-later
//! Surveillance de fichiers (configuration, thème) pour le rechargement à chaud.
//!
//! Le dossier parent est surveillé, pas le fichier : les éditeurs qui enregistrent en
//! écrivant un fichier voisin puis en le renommant sont ainsi suivis, et la surveillance
//! survit au remplacement. Les événements sont regroupés sur 200 ms.

use gtk::glib;
use notify::event::{AccessKind, AccessMode};
use notify::{EventKind, RecursiveMode, Watcher};
use pupitre_config::{charger, Config, ErreurConfig};
use pupitre_i18n::{ta, ArgI18n};
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
            if change_le_contenu(&e.kind) && e.paths.iter().any(|p| concerne(p, &filtre)) {
                let _ = tx.send_blocking(());
            }
        }
    })
    .map_err(|e| {
        ta(
            "surveillance-impossible",
            &[("erreur", ArgI18n::from(e.to_string()))],
        )
    })?;
    let mut dossiers: Vec<&Path> = cibles.iter().filter_map(|c| c.parent()).collect();
    dossiers.sort();
    dossiers.dedup();
    for d in dossiers {
        observateur
            .watch(d, RecursiveMode::NonRecursive)
            .map_err(|e| {
                ta(
                    "surveillance-dossier-impossible",
                    &[
                        ("dossier", ArgI18n::from(d.display().to_string())),
                        ("erreur", ArgI18n::from(e.to_string())),
                    ],
                )
            })?;
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

/// Faux pour une simple lecture (ouverture, fermeture sans écriture) : notify les signale
/// aussi, et relire le fichier après un changement en produirait une sans fin.
fn change_le_contenu(k: &EventKind) -> bool {
    match k {
        EventKind::Access(AccessKind::Close(AccessMode::Write)) => true,
        EventKind::Access(_) => false,
        _ => true,
    }
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

    /// Fait tourner la boucle glib pendant `duree`.
    fn tourner(ctx: &glib::MainContext, duree: Duration) {
        let fin = std::time::Instant::now() + duree;
        while std::time::Instant::now() < fin {
            ctx.iteration(false);
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Relire le fichier dans le rappel (comme `charger`) ne doit pas relancer le rappel :
    /// sinon la configuration est relue sans fin et le menu « Nouvel onglet avec » est
    /// reconstruit en boucle (surbrillance perdue, clic ignoré).
    #[test]
    fn une_lecture_ne_relance_pas() {
        let ctx = glib::MainContext::new();
        ctx.with_thread_default(|| {
            let d = tempfile::tempdir().unwrap();
            let f = d.path().join("pupitre.toml");
            std::fs::write(&f, "theme = \"sombre\"\n").unwrap();
            let appels = std::rc::Rc::new(std::cell::Cell::new(0));
            let (a, lu) = (appels.clone(), f.clone());
            let _s = surveiller_fichiers(std::slice::from_ref(&f), move || {
                let _ = std::fs::read_to_string(&lu);
                a.set(a.get() + 1);
            })
            .unwrap();
            tourner(&ctx, Duration::from_millis(100));
            std::fs::write(&f, "theme = \"clair\"\n").unwrap();
            tourner(&ctx, Duration::from_millis(1500));
            assert_eq!(appels.get(), 1, "rappels après une seule écriture");
            let _ = std::fs::read_to_string(&f);
            tourner(&ctx, Duration::from_millis(600));
            assert_eq!(appels.get(), 1, "une simple lecture a relancé le rappel");
        })
        .unwrap();
    }
}
