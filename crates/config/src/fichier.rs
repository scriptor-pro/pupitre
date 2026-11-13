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
    /// Shell des nouveaux onglets : nom ou chemin ; vide : shell de connexion.
    pub shell: String,
    /// Qui dessine l'invite de commande.
    pub invite: Invite,
    /// Quand la fin d'une commande est annoncée.
    pub annonce_fin_commande: AnnonceFin,
    /// Texte sélectionné à la souris copié dans le presse-papiers au relâchement du bouton.
    pub copie_selection: bool,
    /// Bandeau de disposition du clavier : nom complet, code court ou masqué.
    pub bandeau_clavier: BandeauClavier,
    /// Commandes proposées dans « Nouvel onglet avec » (lancées telles quelles).
    pub shells: Vec<ShellDeclare>,
    /// Section `[notifications]` : ce qui signale un événement, et par quel canal.
    pub notifications: ReglagesNotifications,
    /// Section `[raccourcis]` : une entrée par action dont le raccourci est changé (texte brut ;
    /// l'analyse et la validation se font dans Pupitre, qui garde le défaut en cas d'erreur).
    #[serde(deserialize_with = "raccourcis_lus")]
    pub raccourcis: Vec<ReglageRaccourci>,
}

/// Actions dont le raccourci se change dans `[raccourcis]`, dans l'ordre du fichier par défaut.
pub const ACTIONS_RACCOURCIS: &[&str] = &[
    "nouvel_onglet",
    "fermer_onglet",
    "nouvelle_fenetre",
    "connexions",
    "copier",
    "coller",
    "rechercher",
    "onglet_suivant",
    "onglet_precedent",
    "plein_ecran",
    "zoom_plus",
    "zoom_moins",
    "zoom_normal",
    "commande_precedente",
    "commande_suivante",
    "aide",
];

/// Une ligne de `[raccourcis]` : l'action, la combinaison écrite (vide : aucun raccourci) et
/// la ligne du fichier (absente si la configuration ne vient pas d'un fichier).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReglageRaccourci {
    pub cle: String,
    pub valeur: String,
    pub ligne: Option<usize>,
}

/// Section `[notifications]` : chaque case est réglable, tout est actif par défaut.
/// Aucun son, jamais ; une bulle du bureau est toujours doublée d'un indicateur d'onglet.
#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct ReglagesNotifications {
    /// ✓ ou ✗ sur l'onglet quand une commande finit dans un onglet non affiché.
    pub fin_commande_onglet: bool,
    /// Bulle du bureau pour une commande longue finie quand aucune fenêtre n'est active.
    pub fin_commande_bulle: bool,
    /// ● sur l'onglet quand un programme envoie une notification.
    pub programme_onglet: bool,
    /// Bulle du bureau pour la notification d'un programme.
    pub programme_bulle: bool,
    /// Copie dans le presse-papiers demandée par un programme (OSC 52), signalée.
    pub presse_papiers: bool,
    /// ● sur l'onglet non affiché où la cloche a sonné.
    pub cloche_onglet: bool,
}

impl Default for ReglagesNotifications {
    fn default() -> Self {
        ReglagesNotifications {
            fin_commande_onglet: true,
            fin_commande_bulle: true,
            programme_onglet: true,
            programme_bulle: true,
            presse_papiers: true,
            cloche_onglet: true,
        }
    }
}

/// Réglage `invite`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Invite {
    /// Invite Starship gérée par Pupitre (si Starship est installé).
    Starship,
    /// Invite habituelle du shell, laissée telle quelle.
    Shell,
}

/// Réglage `annonce_fin_commande`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnonceFin {
    Longues,
    Toutes,
    Aucune,
}

/// Réglage `bandeau_clavier`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BandeauClavier {
    /// Nom complet de la disposition, tel que donné par le système (« French (Dvorak) »).
    Nom,
    /// Code court (« fr dvorak »).
    Code,
    /// Pas de bandeau ; les changements restent annoncés.
    Masque,
}

/// Entrée `[[shells]]` : un nom affiché et la commande (argv) à lancer.
#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ShellDeclare {
    #[serde(deserialize_with = "nom_shell_valide")]
    pub nom: String,
    #[serde(deserialize_with = "commande_shell_valide")]
    pub commande: Vec<String>,
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
            shell: String::new(),
            invite: Invite::Starship,
            annonce_fin_commande: AnnonceFin::Longues,
            copie_selection: true,
            bandeau_clavier: BandeauClavier::Nom,
            shells: Vec::new(),
            notifications: ReglagesNotifications::default(),
            raccourcis: Vec::new(),
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
    let texte = lire_texte(chemin)?;
    let mut config: Config =
        toml::from_str(&texte).map_err(|e| erreur_toml(&texte, &e, CLES_CONFIG))?;
    for r in &mut config.raccourcis {
        r.ligne = ligne_de_cle(&texte, "[raccourcis]", &r.cle);
    }
    Ok(config)
}

/// Ligne (à partir de 1) de la clé `cle` dans la table `section` (« [raccourcis] »).
fn ligne_de_cle(texte: &str, section: &str, cle: &str) -> Option<usize> {
    let mut courante = "";
    for (i, l) in texte.lines().enumerate() {
        let l = l.trim();
        if l.starts_with('[') {
            courante = l.split('#').next().unwrap_or(l).trim_end();
        } else if courante == section
            && l.strip_prefix(cle)
                .is_some_and(|r| r.trim_start().starts_with('='))
        {
            return Some(i + 1);
        }
    }
    None
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
    "police, theme, curseur_clignote, historique_lignes, zoom_max_pourcent, langue, shell, invite, annonce_fin_commande, copie_selection, bandeau_clavier, shells, notifications";

const CLES_NOTIFICATIONS: &str =
    "fin_commande_onglet, fin_commande_bulle, programme_onglet, programme_bulle, presse_papiers, cloche_onglet";

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
        String::from_utf8(octets)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, t("config-pas-utf8")))
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
    // Une clé inconnue de la section [notifications] cite les clés de cette section.
    let cles = match e.span() {
        Some(s) if derniere_section(texte, s.start) == Some("[notifications]") => {
            CLES_NOTIFICATIONS
        }
        _ => cles,
    };
    ErreurConfig {
        ligne: e.span().map(|s| numero_de_ligne(texte, s.start)),
        message: traduire(e.message().trim(), cles),
    }
}

/// En-tête de table (`[x]`, sans `[[x]]`) le plus récent avant l'octet `octet`.
fn derniere_section(texte: &str, octet: usize) -> Option<&str> {
    let fin = octet.min(texte.len());
    let avant = texte.get(..fin)?;
    avant
        .lines()
        .map(str::trim)
        .rfind(|l| l.starts_with('['))
        .map(|l| l.split('#').next().unwrap_or(l).trim_end())
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

/// `$XDG_STATE_HOME/pupitre`, sinon `~/.local/state/pupitre` : état et journaux (une valeur
/// relative de `XDG_STATE_HOME` est ignorée).
pub fn dossier_etat() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| dossier_personnel().join(".local/state"));
    base.join("pupitre")
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

/// Table `[raccourcis]` : clés et valeurs gardées telles quelles, sans rien refuser ici (une clé
/// inconnue, une valeur invalide ou qui n'est pas du texte sont signalées avec leur ligne par
/// Pupitre, sans invalider le reste ; un texte vide supprime le raccourci).
fn raccourcis_lus<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<ReglageRaccourci>, D::Error> {
    let table = std::collections::BTreeMap::<String, toml::Value>::deserialize(d)?;
    let mut lus = Vec::new();
    for (cle, v) in table {
        let valeur = match v {
            toml::Value::String(s) => s,
            autre => autre.to_string(),
        };
        lus.push(ReglageRaccourci {
            cle,
            valeur,
            ligne: None,
        });
    }
    Ok(lus)
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

impl<'de> Deserialize<'de> for Invite {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = String::deserialize(d)?;
        match v.as_str() {
            "starship" => Ok(Invite::Starship),
            "shell" => Ok(Invite::Shell),
            _ => Err(notre(ta(
                "config-invite-inconnue",
                &[("v", ArgI18n::from(v))],
            ))),
        }
    }
}

impl<'de> Deserialize<'de> for AnnonceFin {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = String::deserialize(d)?;
        match v.as_str() {
            "longues" => Ok(AnnonceFin::Longues),
            "toutes" => Ok(AnnonceFin::Toutes),
            "aucune" => Ok(AnnonceFin::Aucune),
            _ => Err(notre(ta(
                "config-annonce-inconnue",
                &[("v", ArgI18n::from(v))],
            ))),
        }
    }
}

impl<'de> Deserialize<'de> for BandeauClavier {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = String::deserialize(d)?;
        match v.as_str() {
            "nom" => Ok(BandeauClavier::Nom),
            "code" => Ok(BandeauClavier::Code),
            "masque" => Ok(BandeauClavier::Masque),
            _ => Err(notre(ta(
                "config-bandeau-clavier-inconnu",
                &[("v", ArgI18n::from(v))],
            ))),
        }
    }
}

fn nom_shell_valide<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let v = String::deserialize(d)?;
    if v.trim().is_empty() {
        return Err(notre(t("config-shell-nom-vide")));
    }
    Ok(v.trim().to_owned())
}

/// Premier élément non vide et sans `-` initial (il ne doit pas pouvoir passer pour une option).
fn commande_shell_valide<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    let v = Vec::<String>::deserialize(d)?;
    match v.first().map(|p| p.trim()) {
        None | Some("") => Err(notre(t("config-shell-commande-vide"))),
        Some(p) if p.starts_with('-') => Err(notre(ta(
            "config-shell-commande-option",
            &[("v", ArgI18n::from(p))],
        ))),
        Some(_) => Ok(v),
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
