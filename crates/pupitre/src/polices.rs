// SPDX-License-Identifier: GPL-3.0-or-later
//! Polices embarquées (B612 Mono, Atkinson Hyperlegible Mono, IBM Plex Mono, Nordwand Mono,
//! Ioskeley Mono) : ajoutées à fontconfig pour ce seul processus, sans rien installer chez
//! l'utilisateur. Liste des familles à chasse fixe (`pupitre polices`) et relecture de
//! fontconfig quand une police est installée pendant que Pupitre tourne.

use gtk::glib::translate::from_glib_none;
use gtk::pango;
use gtk::prelude::*;
use pupitre_i18n::{ta, ArgI18n};
use std::ffi::{c_int, c_uchar, c_void, CString};
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

#[link(name = "fontconfig")]
extern "C" {
    /// `FcConfigAppFontAddDir` ; `config` nul = configuration courante. Renvoie FcFalse (0)
    /// si le dossier n'a pas pu être lu.
    fn FcConfigAppFontAddDir(config: *mut c_void, dir: *const c_uchar) -> c_int;
    /// Relit la configuration et les dossiers de polices (nouvelle configuration courante).
    /// Les dossiers ajoutés par `FcConfigAppFontAddDir` sont perdus : à rajouter ensuite.
    fn FcInitReinitialize() -> c_int;
}

#[link(name = "pangocairo-1.0")]
extern "C" {
    /// Table de polices par défaut de Pango (celle de GTK) ; ne demande aucun affichage.
    fn pango_cairo_font_map_get_default() -> *mut c_void;
}

#[link(name = "pangoft2-1.0")]
extern "C" {
    /// Informe la table de polices que la configuration fontconfig a changé.
    fn pango_fc_font_map_config_changed(fontmap: *mut c_void);
}

/// Une police fournie avec Pupitre : famille, début du nom des fichiers
/// (`<fichiers>-Regular.ttf`, `-Bold`, `-Italic`, `-BoldItalic`) et fichier de licence.
pub struct PoliceEmbarquee {
    pub famille: &'static str,
    pub fichiers: &'static str,
    pub licence: &'static str,
}

pub const POLICES_EMBARQUEES: &[PoliceEmbarquee] = &[
    PoliceEmbarquee {
        famille: "B612 Mono",
        fichiers: "B612Mono",
        licence: "OFL-B612.txt",
    },
    PoliceEmbarquee {
        famille: "Atkinson Hyperlegible Mono",
        fichiers: "AtkinsonHyperlegibleMono",
        licence: "OFL-AtkinsonHyperlegibleMono.txt",
    },
    PoliceEmbarquee {
        famille: "IBM Plex Mono",
        fichiers: "IBMPlexMono",
        licence: "OFL-IBMPlexMono.txt",
    },
    PoliceEmbarquee {
        famille: "Nordwand Mono",
        fichiers: "NordwandMono",
        licence: "OFL-NordwandMono.txt",
    },
    PoliceEmbarquee {
        famille: "Ioskeley Mono",
        fichiers: "IoskeleyMono",
        licence: "OFL-IoskeleyMono.txt",
    },
];

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

/// Relit fontconfig (une police installée depuis le démarrage devient visible), rajoute les
/// polices embarquées, puis prévient Pango.
pub fn relire_polices() {
    // SAFETY : fontconfig est utilisé depuis le fil principal, comme partout dans Pupitre.
    unsafe {
        FcInitReinitialize();
    }
    if let Some(d) = dossier_polices() {
        enregistrer_polices_embarquees(&d);
    }
    // SAFETY : le pointeur est la table de polices par défaut de pangocairo, valide pour la
    // durée du processus ; c'est une table fontconfig, que `config_changed` attend.
    unsafe {
        let carte = pango_cairo_font_map_get_default();
        if !carte.is_null() {
            pango_fc_font_map_config_changed(carte);
        }
    }
}

/// Familles à chasse fixe vues par Pango (polices du système et embarquées), triées sans tenir
/// compte de la casse, sans doublon. Les familles embarquées y figurent toujours.
pub fn polices_chasse_fixe() -> Vec<String> {
    let mut noms: Vec<String> = POLICES_EMBARQUEES
        .iter()
        .map(|p| p.famille.to_string())
        .collect();
    // SAFETY : voir `relire_polices` ; `from_glib_none` prend sa propre référence.
    let carte: Option<pango::FontMap> = unsafe {
        let p = pango_cairo_font_map_get_default();
        (!p.is_null()).then(|| from_glib_none(p.cast::<pango::ffi::PangoFontMap>()))
    };
    if let Some(carte) = carte {
        noms.extend(
            carte
                .list_families()
                .iter()
                .filter(|f| f.is_monospace())
                .map(|f| f.name().to_string()),
        );
    }
    noms.sort_by_key(|n| n.to_lowercase());
    noms.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    noms
}

/// `pupitre polices` : une famille à chasse fixe par ligne, les polices fournies avec Pupitre
/// marquées. Code 0.
pub fn executer_polices(sortie: &mut impl Write) -> i32 {
    let mut lignes: Vec<String> = polices_chasse_fixe()
        .into_iter()
        .map(|f| {
            if POLICES_EMBARQUEES
                .iter()
                .any(|p| p.famille.eq_ignore_ascii_case(&f))
            {
                ta("polices-fournie", &[("famille", ArgI18n::from(f.as_str()))])
            } else {
                f
            }
        })
        .collect();
    lignes.sort_by_key(|l| l.to_lowercase());
    for l in lignes {
        let _ = writeln!(sortie, "{l}");
    }
    0
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
    fn polices_embarquees_toutes_presentes() {
        // Les sources, pas un éventuel paquet déjà installé sur la machine.
        let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts");
        assert!(enregistrer_polices_embarquees(&d));
        let familles: Vec<_> = POLICES_EMBARQUEES.iter().map(|p| p.famille).collect();
        assert_eq!(
            familles,
            [
                "B612 Mono",
                "Atkinson Hyperlegible Mono",
                "IBM Plex Mono",
                "Nordwand Mono",
                "Ioskeley Mono"
            ]
        );
        for p in POLICES_EMBARQUEES {
            for style in ["Regular", "Bold", "Italic", "BoldItalic"] {
                let f = d.join(format!("{}-{style}.ttf", p.fichiers));
                assert!(f.is_file(), "{} manquant", f.display());
            }
            let l = d.join(p.licence);
            let texte = std::fs::read_to_string(&l).unwrap();
            assert!(
                texte.contains("SIL OPEN FONT LICENSE Version 1.1"),
                "{} n'est pas la licence OFL 1.1",
                l.display()
            );
        }
    }

    /// Le paquet `.deb` déclare chaque police et sa licence dans `copyright`.
    #[test]
    fn paquet_deb_declare_chaque_licence() {
        let script = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/construire-deb.sh"),
        )
        .unwrap();
        for p in POLICES_EMBARQUEES {
            assert!(
                script.contains(&format!("usr/share/pupitre/fonts/{}-*", p.fichiers)),
                "{} absente de la section copyright",
                p.fichiers
            );
            assert!(
                script.contains(&format!("/usr/share/pupitre/fonts/{}", p.licence)),
                "{} absente de la section copyright",
                p.licence
            );
        }
    }

    #[test]
    fn dossier_des_sources_trouve() {
        let d = dossier_polices().unwrap();
        assert!(d.join("B612Mono-Regular.ttf").is_file());
        assert!(d.join("AtkinsonHyperlegibleMono-Regular.ttf").is_file());
        assert!(enregistrer_polices_embarquees(&d));
    }
}
