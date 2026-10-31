// SPDX-License-Identifier: GPL-3.0-or-later
//! Connexions SSH mémorisées par Pupitre (`connexions.toml`, tableau `[[connexion]]`).

use crate::atomique::ecrire_atomique;
use crate::fichier::{
    dossier_pupitre, erreur_toml, lire_texte, notre, numero_de_ligne, ErreurConfig,
};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};

/// Une connexion mémorisée. Tous les champs sont validés au chargement comme à l'écriture :
/// ils finissent dans la ligne de commande de `ssh`.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Connexion {
    #[serde(deserialize_with = "nom_valide")]
    pub nom: String,
    #[serde(deserialize_with = "hote_valide")]
    pub hote: String,
    #[serde(
        default,
        deserialize_with = "utilisateur_valide",
        skip_serializing_if = "Option::is_none"
    )]
    pub utilisateur: Option<String>,
    #[serde(
        default,
        deserialize_with = "port_valide",
        skip_serializing_if = "Option::is_none"
    )]
    pub port: Option<u16>,
    #[serde(
        default,
        deserialize_with = "cle_valide",
        skip_serializing_if = "Option::is_none"
    )]
    pub cle: Option<PathBuf>,
}

/// Format du fichier : `[[connexion]]` répété.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Fichier<T> {
    #[serde(default = "Vec::new")]
    connexion: Vec<T>,
}

const CLES: &str = "nom, hote, utilisateur, port, cle";

fn verifier_nom(v: &str) -> Result<(), String> {
    if v.trim().is_empty() {
        return Err("nom vide".into());
    }
    if v.chars().any(char::is_control) {
        return Err("le nom ne doit pas contenir de caractère de contrôle".into());
    }
    Ok(())
}

/// Hôte ou utilisateur : mis dans la ligne de commande de ssh, donc jamais pris pour une option.
fn verifier_mot(champ: &str, v: &str) -> Result<(), String> {
    if v.is_empty() {
        return Err(format!("{champ} vide"));
    }
    if v.starts_with('-') {
        return Err(format!(
            "{champ} « {v} » refusé : ne doit pas commencer par « - »"
        ));
    }
    if v.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(format!(
            "{champ} refusé : ni blanc ni caractère de contrôle autorisés"
        ));
    }
    Ok(())
}

fn verifier_port(v: i64) -> Result<u16, String> {
    u16::try_from(v)
        .ok()
        .filter(|&p| p != 0)
        .ok_or_else(|| format!("port {v} hors limites (de 1 à 65535)"))
}

fn verifier_cle(v: &Path) -> Result<(), String> {
    let t = v.to_string_lossy();
    if t.is_empty() {
        return Err("chemin de clé vide".into());
    }
    if t.starts_with('-') {
        return Err("le chemin de la clé ne doit pas commencer par « - »".into());
    }
    if t.chars().any(char::is_control) {
        return Err("le chemin de la clé ne doit pas contenir de caractère de contrôle".into());
    }
    Ok(())
}

fn nom_valide<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let v = String::deserialize(d)?;
    verifier_nom(&v).map_err(notre::<D::Error>)?;
    Ok(v)
}

fn hote_valide<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let v = String::deserialize(d)?;
    verifier_mot("hôte", &v).map_err(notre::<D::Error>)?;
    Ok(v)
}

fn utilisateur_valide<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    let v = Option::<String>::deserialize(d)?;
    if let Some(u) = &v {
        verifier_mot("utilisateur", u).map_err(notre::<D::Error>)?;
    }
    Ok(v)
}

fn port_valide<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u16>, D::Error> {
    // Lu en i64 pour que toute valeur hors limites reçoive le message de `verifier_port`.
    match Option::<i64>::deserialize(d)? {
        Some(v) => verifier_port(v).map(Some).map_err(notre::<D::Error>),
        None => Ok(None),
    }
}

fn cle_valide<'de, D: Deserializer<'de>>(d: D) -> Result<Option<PathBuf>, D::Error> {
    let v = Option::<PathBuf>::deserialize(d)?;
    if let Some(c) = &v {
        verifier_cle(c).map_err(notre::<D::Error>)?;
    }
    Ok(v)
}

/// Mêmes contrôles que le chargement, pour une liste construite en mémoire.
/// Champ d'une connexion, pour montrer l'erreur de validation au bon endroit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChampConnexion {
    Nom,
    Hote,
    Utilisateur,
    Port,
    Cle,
}

/// Première règle violée par une liste de connexions, et le champ fautif.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErreurValidation {
    pub champ: ChampConnexion,
    pub message: String,
}

/// Mêmes contrôles que le chargement, pour une liste construite en mémoire. Appelée par
/// [`enregistrer_connexions`] ; l'interface l'appelle d'abord pour savoir quel champ
/// du formulaire est en cause.
pub fn valider_connexions(v: &[Connexion]) -> Result<(), ErreurValidation> {
    let mut vus = HashSet::new();
    for c in v {
        let erreur = |champ, message| ErreurValidation { champ, message };
        verifier_nom(&c.nom).map_err(|m| erreur(ChampConnexion::Nom, m))?;
        verifier_mot("hôte", &c.hote).map_err(|m| erreur(ChampConnexion::Hote, m))?;
        if let Some(u) = &c.utilisateur {
            verifier_mot("utilisateur", u).map_err(|m| erreur(ChampConnexion::Utilisateur, m))?;
        }
        if let Some(p) = c.port {
            verifier_port(i64::from(p)).map_err(|m| erreur(ChampConnexion::Port, m))?;
        }
        if let Some(k) = &c.cle {
            verifier_cle(k).map_err(|m| erreur(ChampConnexion::Cle, m))?;
        }
        if !vus.insert(c.nom.as_str()) {
            return Err(erreur(
                ChampConnexion::Nom,
                format!("nom « {} » en double", c.nom),
            ));
        }
    }
    Ok(())
}

/// Port saisi à la main : vide = aucun ; sinon un nombre de 1 à 65535.
pub fn analyser_port(texte: &str) -> Result<Option<u16>, String> {
    let t = texte.trim();
    if t.is_empty() {
        return Ok(None);
    }
    let v: i64 = t
        .parse()
        .map_err(|_| format!("port « {t} » invalide : un nombre de 1 à 65535 est attendu"))?;
    verifier_port(v).map(Some)
}

/// `$XDG_CONFIG_HOME/pupitre/connexions.toml`, sinon `~/.config/pupitre/connexions.toml`.
pub fn chemin_connexions() -> PathBuf {
    dossier_pupitre().join("connexions.toml")
}

/// Lit les connexions. Fichier absent : liste vide. Fichier invalide : erreur avec la ligne
/// fautive (celle du champ refusé, ou du second `[[connexion]]` pour un nom en double).
pub fn charger_connexions(chemin: &Path) -> Result<Vec<Connexion>, ErreurConfig> {
    match std::fs::metadata(chemin) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        _ => {}
    }
    let texte = lire_texte(chemin)?;
    let f: Fichier<toml::Spanned<Connexion>> =
        toml::from_str(&texte).map_err(|e| erreur_toml(&texte, &e, CLES))?;
    let mut vus = HashSet::new();
    let mut res = Vec::with_capacity(f.connexion.len());
    for entree in f.connexion {
        let debut = entree.span().start;
        let c = entree.into_inner();
        if !vus.insert(c.nom.clone()) {
            return Err(ErreurConfig {
                ligne: Some(numero_de_ligne(&texte, debut)),
                message: format!("nom « {} » en double (déjà défini plus haut)", c.nom),
            });
        }
        res.push(c);
    }
    Ok(res)
}

/// Écrit la liste (de façon atomique) après les mêmes contrôles qu'au chargement.
pub fn enregistrer_connexions(chemin: &Path, v: &[Connexion]) -> io::Result<()> {
    let invalide = |m: String| io::Error::new(io::ErrorKind::InvalidInput, m);
    valider_connexions(v).map_err(|e| invalide(e.message))?;
    let f = Fichier {
        connexion: v.to_vec(),
    };
    let texte = toml::to_string(&f).map_err(|e| invalide(e.to_string()))?;
    if let Some(dossier) = chemin.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(dossier)?;
    }
    ecrire_atomique(chemin, texte.as_bytes())
}

/// Ligne de commande pour ouvrir la connexion ; `--` protège l'hôte de toute lecture comme option.
pub fn argv_ssh(c: &Connexion) -> Vec<String> {
    let mut v = vec!["ssh".to_owned()];
    if let Some(p) = c.port {
        v.extend(["-p".to_owned(), p.to_string()]);
    }
    if let Some(u) = &c.utilisateur {
        v.extend(["-l".to_owned(), u.clone()]);
    }
    if let Some(k) = &c.cle {
        v.extend(["-i".to_owned(), k.to_string_lossy().into_owned()]);
    }
    v.extend(["--".to_owned(), c.hote.clone()]);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(nom: &str, hote: &str) -> Connexion {
        Connexion {
            nom: nom.into(),
            hote: hote.into(),
            utilisateur: None,
            port: None,
            cle: None,
        }
    }

    fn charge(texte: &str) -> Result<Vec<Connexion>, ErreurConfig> {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("connexions.toml");
        std::fs::write(&p, texte).unwrap();
        charger_connexions(&p)
    }

    #[test]
    fn connexions_aller_retour() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("connexions.toml");
        let v = vec![
            Connexion {
                nom: "Serveur \"prod\"".into(),
                hote: "example.org".into(),
                utilisateur: Some("bob".into()),
                port: Some(2222),
                cle: Some(PathBuf::from("/home/bob/.ssh/id")),
            },
            c("b", "10.0.0.1"),
        ];
        enregistrer_connexions(&p, &v).unwrap();
        assert_eq!(charger_connexions(&p).unwrap(), v);
    }

    #[test]
    fn fichier_absent_vide() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(
            charger_connexions(&d.path().join("rien.toml")).unwrap(),
            vec![]
        );
    }

    #[test]
    fn hote_option_refuse() {
        // Ligne 3 : le champ fautif (le rapport donne la ligne du champ, pas de l'entrée).
        let e = charge("[[connexion]]\nnom = \"a\"\nhote = \"-oProxyCommand=x\"\n").unwrap_err();
        assert_eq!(e.ligne, Some(3), "{e}");
    }

    #[test]
    fn hote_avec_blanc_ou_controle_refuse() {
        assert!(charge("[[connexion]]\nnom = \"a\"\nhote = \"a b\"\n").is_err());
        assert!(charge("[[connexion]]\nnom = \"a\"\nhote = \"a\\u0007\"\n").is_err());
        assert!(
            charge("[[connexion]]\nnom = \"a\"\nhote = \"h\"\nutilisateur = \"-x\"\n").is_err()
        );
        assert!(charge("[[connexion]]\nnom = \"\"\nhote = \"h\"\n").is_err());
        assert!(charge("[[connexion]]\nnom = \"a\"\nhote = \"h\"\ncle = \"\"\n").is_err());
        assert!(charge("[[connexion]]\nnom = \"a\"\nhote = \"h\"\ncle = \"-x\"\n").is_err());
        assert!(charge("[[connexion]]\nnom = \"a\"\nhote = \"h\"\ninconnu = 1\n").is_err());
    }

    #[test]
    fn port_hors_bornes_refuse() {
        for p in ["0", "65536", "-1"] {
            let t = format!("[[connexion]]\nnom = \"a\"\nhote = \"h\"\nport = {p}\n");
            let e = charge(&t).unwrap_err();
            assert_eq!(e.ligne, Some(4), "{p} : {e}");
        }
        assert!(charge("[[connexion]]\nnom = \"a\"\nhote = \"h\"\nport = 65535\n").is_ok());
    }

    #[test]
    fn noms_en_double_refuses() {
        let t = "[[connexion]]\nnom = \"a\"\nhote = \"h\"\n\n[[connexion]]\nnom = \"a\"\nhote = \"i\"\n";
        let e = charge(t).unwrap_err();
        assert_eq!(e.ligne, Some(5), "{e}");
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("c.toml");
        let r = enregistrer_connexions(&p, &[c("a", "h"), c("a", "i")]);
        assert_eq!(r.unwrap_err().kind(), io::ErrorKind::InvalidInput);
        assert!(!p.exists());
    }

    #[test]
    fn enregistrement_invalide_refuse() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("c.toml");
        let r = enregistrer_connexions(&p, &[c("a", "-oProxyCommand=x")]);
        assert_eq!(r.unwrap_err().kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn validation_indique_le_champ() {
        use ChampConnexion as C;
        let champ = |x: Connexion| valider_connexions(&[x]).unwrap_err().champ;
        assert_eq!(champ(c("", "h")), C::Nom);
        assert_eq!(champ(c("a", "-x")), C::Hote);
        let mut x = c("a", "h");
        x.utilisateur = Some("-u".into());
        assert_eq!(champ(x), C::Utilisateur);
        let mut x = c("a", "h");
        x.cle = Some(PathBuf::from("-k"));
        assert_eq!(champ(x), C::Cle);
        let e = valider_connexions(&[c("a", "h"), c("a", "i")]).unwrap_err();
        assert_eq!(e.champ, C::Nom);
        assert!(valider_connexions(&[c("a", "h")]).is_ok());
    }

    #[test]
    fn port_saisi() {
        assert_eq!(analyser_port(""), Ok(None));
        assert_eq!(analyser_port(" 22 "), Ok(Some(22)));
        assert!(analyser_port("0").is_err());
        assert!(analyser_port("65536").is_err());
        assert!(analyser_port("ab").is_err());
        assert!(analyser_port("-1").is_err());
    }

    #[test]
    fn argv_complet() {
        let x = Connexion {
            nom: "n".into(),
            hote: "h".into(),
            utilisateur: Some("u".into()),
            port: Some(2200),
            cle: Some(PathBuf::from("/k")),
        };
        assert_eq!(
            argv_ssh(&x),
            ["ssh", "-p", "2200", "-l", "u", "-i", "/k", "--", "h"]
        );
    }

    #[test]
    fn argv_minimal() {
        assert_eq!(argv_ssh(&c("n", "h")), ["ssh", "--", "h"]);
    }
}
