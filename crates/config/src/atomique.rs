// SPDX-License-Identifier: GPL-3.0-or-later
//! Écriture de fichier sans risque de perte : jamais de fichier à moitié écrit.

use pupitre_i18n::{t, ta, ArgI18n};
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Remplace le contenu de `chemin` de façon atomique : écriture dans un fichier temporaire
/// du même dossier, `fsync`, copie de l'ancien contenu en `<nom>.precedent`, puis `rename`.
/// En cas d'échec, l'ancien fichier est intact et le fichier temporaire est supprimé.
/// Un lien symbolique est suivi (le lien reste un lien) ; les droits de l'ancien fichier sont gardés.
pub fn ecrire_atomique(chemin: &Path, contenu: &[u8]) -> io::Result<()> {
    let cible = match fs::symlink_metadata(chemin) {
        Ok(m) if m.file_type().is_symlink() => fs::canonicalize(chemin)?,
        _ => chemin.to_path_buf(),
    };
    let nom = cible
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, t("fichier-sans-nom")))?
        .to_owned();
    let dossier = match cible.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let ancien = match fs::metadata(&cible) {
        Ok(m) if !m.is_file() => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                ta(
                    "fichier-pas-ordinaire",
                    &[("chemin", ArgI18n::from(cible.display().to_string()))],
                ),
            ))
        }
        Ok(m) => Some(m),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };

    let tmp = dossier.join(nom_temporaire(&nom));
    let resultat = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        if ancien.is_some() {
            // Contenu jamais lisible par d'autres pendant l'écriture ; droits rétablis ensuite.
            options.mode(0o600);
        }
        let mut f = options.open(&tmp)?;
        f.write_all(contenu)?;
        if let Some(m) = &ancien {
            f.set_permissions(m.permissions())?;
        }
        f.sync_all()?;
        drop(f);
        if ancien.is_some() {
            garder_precedent(&cible, &dossier, &nom)?;
        }
        fs::rename(&tmp, &cible)
    })();
    match resultat {
        Ok(()) => {
            // Rend le rename durable ; un échec ici ne remet pas en cause l'écriture.
            if let Ok(d) = File::open(&dossier) {
                let _ = d.sync_all();
            }
            Ok(())
        }
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// Copie l'ancien contenu en `<nom>.precedent`, lui aussi de façon atomique.
fn garder_precedent(cible: &Path, dossier: &Path, nom: &std::ffi::OsStr) -> io::Result<()> {
    let mut nom_precedent = nom.to_owned();
    nom_precedent.push(".precedent");
    let tmp = dossier.join(nom_temporaire(&nom_precedent));
    let resultat = (|| {
        fs::copy(cible, &tmp)?;
        File::open(&tmp)?.sync_all()?;
        fs::rename(&tmp, dossier.join(&nom_precedent))
    })();
    if resultat.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    resultat
}

/// `.<nom>.<pid>.<n>.tmp` : caché, unique dans le processus et entre processus.
fn nom_temporaire(nom: &std::ffi::OsStr) -> OsString {
    static COMPTEUR: AtomicU64 = AtomicU64::new(0);
    let n = COMPTEUR.fetch_add(1, Ordering::Relaxed);
    let mut s = OsString::from(".");
    s.push(nom);
    s.push(format!(".{}.{n}.tmp", std::process::id()));
    s
}
