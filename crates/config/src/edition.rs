// SPDX-License-Identifier: GPL-3.0-or-later
//! Modification d'une valeur de `pupitre.toml` en gardant le reste du fichier : commentaires,
//! ordre des clés, clés inconnues. Utilisé par la fenêtre de réglages.
//!
//! Limites de `toml_edit` 0.25, vérifiées à l'essai : remplacer une valeur efface le
//! commentaire de fin de ligne (on recopie donc la décoration de l'ancienne valeur) ; un
//! fichier aux fins de ligne CRLF est réécrit avec LF, et un retour à la ligne final est ajouté
//! s'il manquait. Aucun autre octet ne change.

use crate::atomique::ecrire_atomique;
use crate::fichier::{charger, chemin_config};
use pupitre_i18n::{t, ta, ArgI18n};
use std::fmt;
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item, Table, Value};

/// Pourquoi un réglage n'a pas été enregistré.
#[derive(Debug, Clone, PartialEq)]
pub enum ErreurEdition {
    /// Le fichier est invalide (ligne si connue) : on ne l'écrase jamais.
    Invalide(Option<usize>),
    /// Le texte n'a pas pu être analysé pour modification.
    Syntaxe(String),
    /// Un maillon du chemin existe mais n'est pas une table.
    PasUneTable(String),
    /// Écriture impossible.
    Ecriture(String),
}

impl fmt::Display for ErreurEdition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            ErreurEdition::Invalide(Some(l)) => {
                ta("edition-invalide-ligne", &[("ligne", ArgI18n::from(*l))])
            }
            ErreurEdition::Invalide(None) => t("edition-invalide"),
            ErreurEdition::Syntaxe(d) => {
                ta("edition-syntaxe", &[("detail", ArgI18n::from(d.as_str()))])
            }
            ErreurEdition::PasUneTable(c) => ta(
                "edition-pas-une-table",
                &[("cle", ArgI18n::from(c.as_str()))],
            ),
            ErreurEdition::Ecriture(e) => {
                ta("edition-ecriture", &[("erreur", ArgI18n::from(e.as_str()))])
            }
        };
        f.write_str(&s)
    }
}

impl std::error::Error for ErreurEdition {}

/// Remplace la valeur de `chemin` (tables puis clé, ex. `["notifications", "cloche_onglet"]`)
/// dans `texte`. La clé est ajoutée en fin de table si elle manque, et la table est créée.
pub fn modifier_cle(
    texte: &str,
    chemin: &[&str],
    mut valeur: Value,
) -> Result<String, ErreurEdition> {
    let Some((cle, tables)) = chemin.split_last() else {
        return Err(ErreurEdition::Syntaxe(String::new()));
    };
    let mut doc: DocumentMut = texte
        .parse()
        .map_err(|e: toml_edit::TomlError| ErreurEdition::Syntaxe(e.message().to_string()))?;
    let mut table: &mut Table = doc.as_table_mut();
    for nom in tables {
        if !table.contains_key(nom) {
            table.insert(nom, Item::Table(Table::new()));
        }
        table = table
            .get_mut(nom)
            .and_then(Item::as_table_mut)
            .ok_or_else(|| ErreurEdition::PasUneTable((*nom).to_string()))?;
    }
    // Clé existante : seule la valeur change (la clé garde les commentaires qui la précèdent,
    // la valeur garde sa décoration, dont le commentaire de fin de ligne).
    if let Some(item) = table.get_mut(cle) {
        if let Some(ancienne) = item.as_value() {
            *valeur.decor_mut() = ancienne.decor().clone();
        }
        *item = Item::Value(valeur);
    } else {
        table.insert(cle, Item::Value(valeur));
    }
    Ok(doc.to_string())
}

/// Enregistre un réglage dans `pupitre.toml` (voir `enregistrer_reglage_dans`).
pub fn enregistrer_reglage(chemin: &[&str], valeur: Value) -> Result<(), ErreurEdition> {
    enregistrer_reglage_dans(&chemin_config(), chemin, valeur)
}

/// Lit `fichier`, refuse s'il est invalide, copie l'ancien contenu dans `<fichier>.bak`, puis
/// écrit le nouveau de façon atomique.
pub fn enregistrer_reglage_dans(
    fichier: &Path,
    chemin: &[&str],
    valeur: Value,
) -> Result<(), ErreurEdition> {
    if let Err(e) = charger(fichier) {
        return Err(ErreurEdition::Invalide(e.ligne));
    }
    let ancien = std::fs::read(fichier).map_err(|e| ErreurEdition::Ecriture(e.to_string()))?;
    let texte = String::from_utf8(ancien.clone()).map_err(|_| ErreurEdition::Invalide(None))?;
    let nouveau = modifier_cle(&texte, chemin, valeur)?;
    if nouveau == texte {
        return Ok(());
    }
    ecrire_atomique(&chemin_bak(fichier), &ancien)
        .map_err(|e| ErreurEdition::Ecriture(e.to_string()))?;
    ecrire_atomique(fichier, nouveau.as_bytes()).map_err(|e| ErreurEdition::Ecriture(e.to_string()))
}

fn chemin_bak(fichier: &Path) -> PathBuf {
    let mut nom = fichier.as_os_str().to_owned();
    nom.push(".bak");
    PathBuf::from(nom)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fichier::CONFIG_DEFAUT;

    fn val(s: &str) -> Value {
        Value::from(s)
    }

    #[test]
    fn reglage_garde_les_commentaires() {
        let sortie = modifier_cle(CONFIG_DEFAUT, &["theme"], val("sombre")).unwrap();
        let a: Vec<&str> = CONFIG_DEFAUT.lines().collect();
        let b: Vec<&str> = sortie.lines().collect();
        assert_eq!(a.len(), b.len());
        let diff: Vec<usize> = (0..a.len()).filter(|&i| a[i] != b[i]).collect();
        assert_eq!(diff.len(), 1, "{diff:?}");
        assert!(b[diff[0]].starts_with("theme"));
        assert!(b[diff[0]].contains("\"sombre\""));
    }

    #[test]
    fn commentaire_de_fin_de_ligne_garde() {
        let s = modifier_cle("theme = \"clair\" # mon choix\n", &["theme"], val("sombre"));
        assert_eq!(s.unwrap(), "theme = \"sombre\" # mon choix\n");
    }

    #[test]
    fn cle_absente_ajoutee() {
        let s = modifier_cle("# début\npolice = \"A 10\"\n", &["theme"], val("clair")).unwrap();
        assert_eq!(s, "# début\npolice = \"A 10\"\ntheme = \"clair\"\n");
    }

    #[test]
    fn table_notifications_creee() {
        let s = modifier_cle(
            "theme = \"clair\"\n",
            &["notifications", "cloche_onglet"],
            Value::from(false),
        )
        .unwrap();
        assert!(s.contains("[notifications]\ncloche_onglet = false"), "{s}");
        assert!(toml::from_str::<crate::Config>(&s).is_ok());
    }

    #[test]
    fn cles_inconnues_gardees() {
        let src = "inconnue = 3 # à moi\n[ailleurs]\nx = 1\n";
        let s = modifier_cle(src, &["theme"], val("clair")).unwrap();
        assert!(s.contains("inconnue = 3 # à moi"));
        assert!(s.contains("[ailleurs]\nx = 1"));
    }

    #[test]
    fn maillon_pas_une_table() {
        let r = modifier_cle("notifications = 3\n", &["notifications", "x"], val("a"));
        assert!(matches!(r, Err(ErreurEdition::PasUneTable(_))));
    }

    #[test]
    fn fichier_invalide_pas_d_ecriture() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("pupitre.toml");
        let src = "theme = \"clair\"\ncouleur_inconnue = 1\n";
        std::fs::write(&f, src).unwrap();
        let r = enregistrer_reglage_dans(&f, &["theme"], val("sombre"));
        assert_eq!(r, Err(ErreurEdition::Invalide(Some(2))));
        assert_eq!(std::fs::read_to_string(&f).unwrap(), src);
        assert!(!d.path().join("pupitre.toml.bak").exists());
    }

    #[test]
    fn sauvegarde_bak() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("pupitre.toml");
        std::fs::write(&f, CONFIG_DEFAUT).unwrap();
        enregistrer_reglage_dans(&f, &["theme"], val("sombre")).unwrap();
        assert_eq!(
            std::fs::read_to_string(d.path().join("pupitre.toml.bak")).unwrap(),
            CONFIG_DEFAUT
        );
        assert!(std::fs::read_to_string(&f).unwrap().contains("\"sombre\""));
        let c = charger(&f).unwrap();
        assert_eq!(c.theme, crate::ThemeChoix::Sombre);
    }
}
