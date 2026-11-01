// SPDX-License-Identifier: GPL-3.0-or-later
//! Fichier de configuration TOML : valeurs par défaut, chargement, erreurs avec numéro de ligne.

use crate::atomique::ecrire_atomique;
use pupitre_i18n::{langue_active, t, ta, ArgI18n};
use serde::{de, Deserialize, Deserializer};
use std::fmt;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

/// Fichier créé pour l'utilisateur au premier lancement ; donne exactement `Config::default()`.
pub const CONFIG_DEFAUT: &str = include_str!("../../../assets/config/pupitre.toml");

/// Fichiers par défaut par langue : le même réglage partout, seuls les commentaires diffèrent.
/// Une langue absente de cette liste reçoit le fichier français.
const CONFIGS_DEFAUT: &[(&str, &str)] = &[("fr", CONFIG_DEFAUT)];

/// Valeurs acceptées pour `langue` (vide : langue du système).
pub const LANGUES_CONFIG: &[&str] = &["fr", "en", "es", "it", "de", "nl"];

/// Fichier par défaut de la langue donnée (code de langue, ex. « de »), sinon le français.
pub fn config_defaut(langue: &str) -> &'static str {
    CONFIGS_DEFAUT
        .iter()
        .find(|(l, _)| *l == langue)
        .map_or(CONFIG_DEFAUT, |(_, c)| c)
}

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
    /// Langue de l'interface, appliquée au prochain lancement ; vide : langue du système.
    #[serde(deserialize_with = "langue_valide")]
    pub langue: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            police: "B612 Mono 11".into(),
            theme: ThemeChoix::Systeme,
            curseur_clignote: false,
            historique_lignes: 10_000,
            zoom_max_pourcent: 200,
            langue: String::new(),
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
            Some(l) => f.write_str(&ta(
                "config-ligne",
                &[
                    ("ligne", ArgI18n::from(l)),
                    ("message", ArgI18n::from(self.message.as_str())),
                ],
            )),
            None => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for ErreurConfig {}

/// Lit et valide le fichier. Ne panique jamais ; en cas d'erreur, l'appelant garde
/// la configuration précédente (ou les défauts) et affiche le message.
pub fn charger(chemin: &Path) -> Result<Config, ErreurConfig> {
    lire_toml(chemin)
}

/// Lit un fichier TOML (taille bornée, UTF-8) et le désérialise ; les erreurs portent le
/// numéro de ligne et un message en français.
pub(crate) fn lire_toml<T: de::DeserializeOwned>(chemin: &Path) -> Result<T, ErreurConfig> {
    lire_toml_avec(chemin, CLES_CONFIG)
}

/// Comme `lire_toml`, avec la liste des clés citée dans le message « clé inconnue ».
pub(crate) fn lire_toml_avec<T: de::DeserializeOwned>(
    chemin: &Path,
    cles: &str,
) -> Result<T, ErreurConfig> {
    let texte = lire_texte(chemin)?;
    toml::from_str(&texte).map_err(|e| erreur_toml(&texte, &e, cles))
}

const CLES_CONFIG: &str =
    "police, theme, curseur_clignote, historique_lignes, zoom_max_pourcent, langue";

/// Lit un fichier (ordinaire, taille bornée, UTF-8).
pub(crate) fn lire_texte(chemin: &Path) -> Result<String, ErreurConfig> {
    let lire = || -> io::Result<String> {
        let f = std::fs::File::open(chemin)?;
        if !f.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                t("config-pas-ordinaire"),
            ));
        }
        let mut octets = Vec::new();
        f.take(TAILLE_MAX + 1).read_to_end(&mut octets)?;
        if octets.len() as u64 > TAILLE_MAX {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                t("config-trop-gros"),
            ));
        }
        String::from_utf8(octets).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, t("config-pas-utf8"))
        })
    };
    lire().map_err(|e| ErreurConfig {
        ligne: None,
        message: ta(
            "config-lecture-impossible",
            &[
                ("chemin", ArgI18n::from(chemin.display().to_string())),
                ("erreur", ArgI18n::from(e.to_string())),
            ],
        ),
    })
}

/// Convertit une erreur toml en erreur française avec numéro de ligne.
pub(crate) fn erreur_toml(texte: &str, e: &toml::de::Error, cles: &str) -> ErreurConfig {
    ErreurConfig {
        ligne: e.span().map(|s| numero_de_ligne(texte, s.start)),
        message: traduire(e.message().trim(), cles),
    }
}

/// Traduit en français les messages courants de toml/serde ; les autres sont donnés tels quels,
/// précédés de « syntaxe TOML incorrecte ».
fn traduire(m: &str, cles: &str) -> String {
    if let Some(message) = m.strip_prefix(MARQUE) {
        return message.to_owned();
    }
    if let Some(reste) = m.strip_prefix("unknown field `") {
        let cle = reste.split('`').next().unwrap_or(reste);
        return ta(
            "config-cle-inconnue",
            &[("cle", ArgI18n::from(cle)), ("cles", ArgI18n::from(cles))],
        );
    }
    for (prefixe, debut) in [
        ("invalid type: ", "config-type-incorrect"),
        ("invalid value: ", "config-valeur-incorrecte"),
    ] {
        if let Some((recu, attendu)) = m
            .strip_prefix(prefixe)
            .and_then(|r| r.split_once(", expected "))
        {
            return ta(
                debut,
                &[
                    ("recu", ArgI18n::from(traduire_recu(recu))),
                    ("attendu", ArgI18n::from(traduire_attendu(attendu))),
                ],
            );
        }
    }
    match m {
        "duplicate key" => t("config-cle-double"),
        _ => ta("config-syntaxe-toml", &[("detail", ArgI18n::from(m))]),
    }
}

/// Préfixe des messages écrits par Pupitre (déjà en français), retiré par `traduire`.
pub(crate) const MARQUE: &str = "pupitre: ";

pub(crate) fn notre<E: de::Error>(message: String) -> E {
    E::custom(format!("{MARQUE}{message}"))
}

fn traduire_recu(r: &str) -> String {
    let valeur = |p: &str| r.strip_prefix(p).map(|v| v.trim_matches('`').to_owned());
    let avec = |id: &str, v: String| ta(id, &[("v", ArgI18n::from(v))]);
    if let Some(v) = valeur("integer ") {
        avec("config-recu-entier", v)
    } else if let Some(v) = valeur("floating point ") {
        avec("config-recu-decimal", v)
    } else if let Some(v) = valeur("string ") {
        avec("config-recu-texte", v)
    } else if let Some(v) = valeur("boolean ") {
        avec("config-recu-booleen", v)
    } else {
        r.to_owned()
    }
}

fn traduire_attendu(a: &str) -> String {
    match a {
        "a boolean" => t("config-attendu-booleen"),
        "a string" => t("config-attendu-texte"),
        "i64" | "u64" | "u32" | "u16" => t("config-attendu-entier"),
        _ => a.to_owned(),
    }
}

pub(crate) fn numero_de_ligne(texte: &str, octet: usize) -> usize {
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
    dossier_pupitre().join("pupitre.toml")
}

/// `$XDG_CONFIG_HOME/pupitre`, sinon `~/.config/pupitre`.
pub(crate) fn dossier_pupitre() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| dossier_personnel().join(".config"));
    base.join("pupitre")
}

pub(crate) fn dossier_personnel() -> PathBuf {
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
    let langue = langue_active();
    ecrire_atomique(chemin, config_defaut(langue.language.as_str()).as_bytes())?;
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
            _ => Err(notre(ta(
                "config-theme-inconnu",
                &[("v", ArgI18n::from(v))],
            ))),
        }
    }
}

fn police_valide<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let v = String::deserialize(d)?;
    if v.trim().is_empty() {
        return Err(notre(t("config-police-vide")));
    }
    Ok(v.trim().to_owned())
}

fn historique_valide<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    // Lu en i64 pour que toute valeur hors limites reçoive le message ci-dessous.
    let v = i64::deserialize(d)?;
    if !(0..=i64::from(HISTORIQUE_MAX)).contains(&v) {
        return Err(notre(ta(
            "config-historique-limites",
            &[
                ("v", ArgI18n::Nombre(v)),
                ("max", ArgI18n::Nombre(i64::from(HISTORIQUE_MAX))),
            ],
        )));
    }
    u32::try_from(v).map_err(de::Error::custom)
}

fn zoom_valide<'de, D: Deserializer<'de>>(d: D) -> Result<u16, D::Error> {
    let v = i64::deserialize(d)?;
    if !(i64::from(ZOOM_MIN)..=i64::from(ZOOM_MAX)).contains(&v) {
        return Err(notre(ta(
            "config-zoom-limites",
            &[
                ("v", ArgI18n::Nombre(v)),
                ("min", ArgI18n::Nombre(i64::from(ZOOM_MIN))),
                ("max", ArgI18n::Nombre(i64::from(ZOOM_MAX))),
            ],
        )));
    }
    u16::try_from(v).map_err(de::Error::custom)
}

fn langue_valide<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let v = String::deserialize(d)?;
    if v.is_empty() || LANGUES_CONFIG.contains(&v.as_str()) {
        Ok(v)
    } else {
        Err(notre(ta(
            "config-langue-inconnue",
            &[("v", ArgI18n::from(v))],
        )))
    }
}

/// Réglage `langue` du fichier, lu avant tout le reste (pour choisir la langue des messages,
/// y compris ceux de l'erreur). Tolérant : fichier absent, illisible, invalide ou langue
/// inconnue donnent `None` (langue du système).
pub fn langue_configuree(chemin: &Path) -> Option<String> {
    let texte = lire_texte(chemin).ok()?;
    let table: toml::Table = toml::from_str(&texte).ok()?;
    let v = table.get("langue")?.as_str()?;
    LANGUES_CONFIG.contains(&v).then(|| v.to_string())
}
