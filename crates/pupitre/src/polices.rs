// SPDX-License-Identifier: GPL-3.0-or-later
//! Polices embarquées (B612 Mono) : ajoutées à fontconfig pour ce seul processus,
//! sans rien installer chez l'utilisateur.

use std::ffi::{c_int, c_uchar, c_void, CString};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

#[link(name = "fontconfig")]
extern "C" {
    /// `FcConfigAppFontAddDir` ; `config` nul = configuration courante. Renvoie FcFalse (0)
    /// si le dossier n'a pas pu être lu.
    fn FcConfigAppFontAddDir(config: *mut c_void, dir: *const c_uchar) -> c_int;
}

/// Dossier des polices embarquées : `$PUPITRE_ASSETS/fonts`, sinon
/// `/usr/share/pupitre/fonts` (paquet installé), sinon `assets/fonts` des sources.
pub fn dossier_polices() -> Option<PathBuf> {
    let candidats = [
        std::env::var_os("PUPITRE_ASSETS")
            .filter(|v| !v.is_empty())
            .map(|v| PathBuf::from(v).join("fonts")),
        Some(PathBuf::from("/usr/share/pupitre/fonts")),
        Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts")),
    ];
    candidats.into_iter().flatten().find(|d| d.is_dir())
}

/// Ajoute les polices de `dossier` à fontconfig. À appeler avant que GTK ne charge une
/// police. Renvoie `false` (sans planter) si le dossier est absent ou illisible.
pub fn enregistrer_polices_embarquees(dossier: &Path) -> bool {
    if !dossier.is_dir() {
        return false;
    }
    let Ok(chemin) = CString::new(dossier.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY : `chemin` est une chaîne C valide qui vit pendant l'appel ; fontconfig la
    // copie. Un pointeur de configuration nul désigne la configuration courante.
    unsafe { FcConfigAppFontAddDir(std::ptr::null_mut(), chemin.as_ptr().cast()) != 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dossier_absent_refuse_sans_planter() {
        assert!(!enregistrer_polices_embarquees(Path::new(
            "/nulle/part/pupitre"
        )));
    }

    #[test]
    fn dossier_des_sources_trouve() {
        let d = dossier_polices().unwrap();
        assert!(d.join("B612Mono-Regular.ttf").is_file());
        assert!(enregistrer_polices_embarquees(&d));
    }
}
