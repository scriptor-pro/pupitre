// SPDX-License-Identifier: GPL-3.0-or-later
//! Fichier de configuration TOML : valeurs par défaut, chargement, erreurs avec numéro de ligne.

use crate::atomique::ecrire_atomique;
use serde::{de, Deserialize, Deserializer};
use std::fmt;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

/// Fichier créé pour l'utilisateur au premier lancement ; donne exactement `Config::default()`.
pub const CONFIG_DEFAUT: &str = include_str!("../../../assets/config/pupitre.toml");

/// Au-delà, le fichier est refusé (protection contre un fichier énorme ou un périphérique).
const TAILLE_MAX: u64 = 1024 * 1024;
const HISTORIQUE_MAX: u32 = 1_000_000;
const ZOOM_MIN: u16 = 100;
const ZOOM_MAX: u16 = 500;

#[derive(Debug, Clone, PartialEq)]
pub enum ThemeChoix {
    Sombre,
    Clair,
    /// Suit la préférence clair/sombre du bureau.
    Systeme,
    Fichier(PathBuf),
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    #[serde(deserialize_with = "police_valide")]
    pub police: String,
    pub theme: ThemeChoix,
    pub curseur_clignote: bool,
    #[serde(deserialize_with = "historique_valide")]
    pub historique_lignes: u32,
    #[serde(deserialize_with = "zoom_valide")]
    pub zoom_max_pourcent: u16,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            police: "B612 Mono 11".into(),
            theme: ThemeChoix::Systeme,
            curseur_clignote: false,
            historique_lignes: 10_000,
            zoom_max_pourcent: 200,
        }
    }
}

/// Configuration refusée. `ligne` (à partir de 1) est absente si le fichier n'a pas pu être lu.
#[derive(Debug, Clone, PartialEq)]
pub struct ErreurConfig {
    pub ligne: Option<usize>,
    pub message: String,
}

impl fmt::Display for ErreurConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.ligne {
            Some(l) => write!(f, "ligne {l} : {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for ErreurConfig {}

/// Lit et valide le fichier. Ne panique jamais ; en cas d'erreur, l'appelant garde
/// la configuration précédente (ou les défauts) et affiche le message.
pub fn charger(chemin: &Path) -> Result<Config, ErreurConfig> {
    let sans_ligne = |message: String| ErreurConfig {
        ligne: None,
        message,
    };
    let lire = || -> io::Result<String> {
        let f = std::fs::File::open(chemin)?;
        if !f.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "ce n'est pas un fichier ordinaire",
            ));
        }
        let mut octets = Vec::new();
        f.take(TAILLE_MAX + 1).read_to_end(&mut octets)?;
        if octets.len() as u64 > TAILLE_MAX {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "fichier trop gros (plus de 1 Mio)",
            ));
        }
        String::from_utf8(octets).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "le fichier n'est pas en UTF-8")
        })
    };
    let texte =
        lire().map_err(|e| sans_ligne(format!("impossible de lire {} : {e}", chemin.display())))?;
    toml::from_str(&texte).map_err(|e| ErreurConfig {
        ligne: e.span().map(|s| numero_de_ligne(&texte, s.start)),
        message: traduire(e.message().trim()),
    })
}

/// Traduit en français les messages courants de toml/serde ; les autres sont donnés tels quels,
/// précédés de « syntaxe TOML incorrecte ».
fn traduire(m: &str) -> String {
    if let Some(message) = m.strip_prefix(MARQUE) {
        return message.to_owned();
    }
    if let Some(reste) = m.strip_prefix("unknown field `") {
        let cle = reste.split('`').next().unwrap_or(reste);
        return format!(
            "clé inconnue « {cle} » ; clés possibles : police, theme, curseur_clignote, \
             historique_lignes, zoom_max_pourcent"
        );
    }
    for (prefixe, debut) in [
        ("invalid type: ", "type incorrect"),
        ("invalid value: ", "valeur incorrecte"),
    ] {
        if let Some((recu, attendu)) = m
            .strip_prefix(prefixe)
            .and_then(|r| r.split_once(", expected "))
        {
            return format!(
                "{debut} : {} au lieu de {}",
                traduire_recu(recu),
                traduire_attendu(attendu)
            );
        }
    }
    match m {
        "duplicate key" => "clé en double (déjà définie plus haut)".into(),
        _ => format!("syntaxe TOML incorrecte ({m})"),
    }
}

/// Préfixe des messages écrits par Pupitre (déjà en français), retiré par `traduire`.
const MARQUE: &str = "pupitre: ";

fn notre<E: de::Error>(message: String) -> E {
    E::custom(format!("{MARQUE}{message}"))
}

fn traduire_recu(r: &str) -> String {
    let valeur = |p: &str| r.strip_prefix(p).map(|v| v.trim_matches('`').to_owned());
    if let Some(v) = valeur("integer ") {
        format!("le nombre {v}")
    } else if let Some(v) = valeur("floating point ") {
        format!("le nombre décimal {v}")
    } else if let Some(v) = valeur("string ") {
        format!("le texte {v}")
    } else if let Some(v) = valeur("boolean ") {
        format!("la valeur {v}")
    } else {
        r.to_owned()
    }
}

fn traduire_attendu(a: &str) -> String {
    match a {
        "a boolean" => "true ou false".into(),
        "a string" => "un texte entre guillemets".into(),
        "i64" | "u64" | "u32" | "u16" => "un nombre entier".into(),
        _ => a.to_owned(),
    }
}

fn numero_de_ligne(texte: &str, octet: usize) -> usize {
    let fin = octet.min(texte.len());
    texte.as_bytes()[..fin]
        .iter()
        .filter(|&&c| c == b'\n')
        .count()
        + 1
}

/// `$XDG_CONFIG_HOME/pupitre/pupitre.toml`, sinon `~/.config/pupitre/pupitre.toml`
/// (une valeur relative de `XDG_CONFIG_HOME` est ignorée, comme le veut la norme XDG).
pub fn chemin_config() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| dossier_personnel().join(".config"));
    base.join("pupitre").join("pupitre.toml")
}

fn dossier_personnel() -> PathBuf {
    std::env::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

/// Crée le fichier commenté par défaut (et ses dossiers) s'il n'existe pas.
/// Renvoie `true` s'il a été créé.
pub fn creer_defaut_si_absent(chemin: &Path) -> io::Result<bool> {
    match std::fs::symlink_metadata(chemin) {
        Ok(_) => return Ok(false),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    if let Some(dossier) = chemin.parent() {
        std::fs::create_dir_all(dossier)?;
    }
    ecrire_atomique(chemin, CONFIG_DEFAUT.as_bytes())?;
    Ok(true)
}

impl<'de> Deserialize<'de> for ThemeChoix {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = String::deserialize(d)?;
        match v.as_str() {
            "sombre" => Ok(ThemeChoix::Sombre),
            "clair" => Ok(ThemeChoix::Clair),
            "systeme" | "système" => Ok(ThemeChoix::Systeme),
            _ if v.starts_with('/') => Ok(ThemeChoix::Fichier(PathBuf::from(v))),
            _ if v.starts_with("~/") => Ok(ThemeChoix::Fichier(dossier_personnel().join(&v[2..]))),
            _ => Err(notre(format!(
                "thème « {v} » inconnu : valeurs possibles « sombre », « clair », « systeme », \
                 ou le chemin d'un fichier de thème commençant par / ou ~/"
            ))),
        }
    }
}

fn police_valide<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let v = String::deserialize(d)?;
    if v.trim().is_empty() {
        return Err(notre(String::from(
            "police vide : indiquer une famille et une taille, ex. « B612 Mono 11 »",
        )));
    }
    Ok(v.trim().to_owned())
}

fn historique_valide<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    // Lu en i64 pour que toute valeur hors limites reçoive le message ci-dessous.
    let v = i64::deserialize(d)?;
    if !(0..=i64::from(HISTORIQUE_MAX)).contains(&v) {
        return Err(notre(format!(
            "historique de {v} lignes hors limites (de 0 à {HISTORIQUE_MAX})"
        )));
    }
    u32::try_from(v).map_err(de::Error::custom)
}

fn zoom_valide<'de, D: Deserializer<'de>>(d: D) -> Result<u16, D::Error> {
    let v = i64::deserialize(d)?;
    if !(i64::from(ZOOM_MIN)..=i64::from(ZOOM_MAX)).contains(&v) {
        return Err(notre(format!(
            "zoom maximum {v} % hors limites (de {ZOOM_MIN} à {ZOOM_MAX})"
        )));
    }
    u16::try_from(v).map_err(de::Error::custom)
}
