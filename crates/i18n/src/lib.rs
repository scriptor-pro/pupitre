// SPDX-License-Identifier: GPL-3.0-or-later
//! Traduction de Pupitre (Fluent). Sans GTK.
//!
//! Les fichiers `.ftl` sont intégrés à l'exécutable : la traduction est toujours disponible,
//! hors ligne. Repli, par phrase : langue demandée, puis anglais, puis français (langue source,
//! toujours complète). Un identifiant inconnu partout donne l'identifiant lui-même.

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource, FluentValue};
use std::sync::{Arc, RwLock};
pub use unic_langid::LanguageIdentifier;

/// Langues connues de Pupitre, avec leur fichier de traductions intégré.
const LANGUES: &[(&str, &str)] = &[
    ("fr", include_str!("../langues/fr/pupitre.ftl")),
    ("en", include_str!("../langues/en/pupitre.ftl")),
    ("es", include_str!("../langues/es/pupitre.ftl")),
    ("it", include_str!("../langues/it/pupitre.ftl")),
    ("de", include_str!("../langues/de/pupitre.ftl")),
    ("nl", include_str!("../langues/nl/pupitre.ftl")),
];

/// Aides intégrées (`assets/aide/<langue>.md`) ; les langues absentes retombent sur l'anglais
/// puis le français.
const AIDES: &[(&str, &str)] = &[("fr", include_str!("../../../assets/aide/fr.md"))];

/// Argument d'une phrase traduite.
#[derive(Debug, Clone, PartialEq)]
pub enum ArgI18n {
    Texte(String),
    /// Nombre : sert aussi à choisir le pluriel (`[one]`, `*[other]`…).
    Nombre(i64),
}

impl From<&str> for ArgI18n {
    fn from(s: &str) -> Self {
        ArgI18n::Texte(s.to_string())
    }
}

impl From<String> for ArgI18n {
    fn from(s: String) -> Self {
        ArgI18n::Texte(s)
    }
}

impl From<i64> for ArgI18n {
    fn from(n: i64) -> Self {
        ArgI18n::Nombre(n)
    }
}

impl From<usize> for ArgI18n {
    fn from(n: usize) -> Self {
        ArgI18n::Nombre(i64::try_from(n).unwrap_or(i64::MAX))
    }
}

/// Fichiers `.ftl` intégrés : `(code de langue, contenu)`.
pub fn sources_integrees() -> impl Iterator<Item = (&'static str, &'static str)> {
    LANGUES.iter().copied()
}

/// Vérifie qu'un fichier Fluent se lit sans erreur.
pub fn verifier_source(source: &str) -> Result<(), String> {
    FluentResource::try_new(source.to_string())
        .map(|_| ())
        .map_err(|(_, erreurs)| {
            erreurs
                .iter()
                .map(|e| format!("{e:?}"))
                .collect::<Vec<_>>()
                .join(" ; ")
        })
}

fn analyser_langue(brut: &str) -> Vec<LanguageIdentifier> {
    // « de_AT.UTF-8@euro » devient « de-AT », puis « de ».
    let sans_suffixe = brut.split(['.', '@']).next().unwrap_or("").trim();
    if sans_suffixe.is_empty() || sans_suffixe == "C" || sans_suffixe == "POSIX" {
        return Vec::new();
    }
    let Ok(id) = sans_suffixe.replace('_', "-").parse::<LanguageIdentifier>() else {
        return Vec::new();
    };
    let mut v = vec![id.clone()];
    if id.region.is_some() || id.script.is_some() || id.variants().next().is_some() {
        if let Ok(base) = id.language.as_str().parse::<LanguageIdentifier>() {
            v.push(base);
        }
    }
    v
}

/// Langues demandées par l'environnement, de la plus souhaitée à la moins souhaitée :
/// `LANGUAGE` (liste séparée par `:`), sinon `LC_ALL`, `LC_MESSAGES`, `LANG` (le premier non
/// vide). `C` et `POSIX` sont ignorés.
pub fn langues_demandees(env: &dyn Fn(&str) -> Option<String>) -> Vec<LanguageIdentifier> {
    let non_vide = |nom: &str| env(nom).filter(|v| !v.trim().is_empty());
    let brutes: Vec<String> = match non_vide("LANGUAGE") {
        Some(l) => l.split(':').map(String::from).collect(),
        None => ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .find_map(|n| non_vide(n))
            .into_iter()
            .collect(),
    };
    let mut v: Vec<LanguageIdentifier> = Vec::new();
    for b in brutes {
        for id in analyser_langue(&b) {
            if !v.contains(&id) {
                v.push(id);
            }
        }
    }
    v
}

type Paquet = FluentBundle<FluentResource>;

/// Traductions d'une langue, avec leur chaîne de repli.
pub struct Traducteur {
    langue: LanguageIdentifier,
    chaine: Vec<Paquet>,
}

fn id_langue(code: &str) -> LanguageIdentifier {
    code.parse().unwrap_or_default()
}

impl Traducteur {
    /// `chaine` : `(langue, source .ftl)`, de la plus prioritaire à la moins prioritaire.
    pub fn nouveau(langue: &LanguageIdentifier, chaine: &[(LanguageIdentifier, &str)]) -> Self {
        let paquets = chaine
            .iter()
            .map(|(l, source)| {
                // Un fichier en partie invalide garde ses phrases valides.
                let res = match FluentResource::try_new((*source).to_string()) {
                    Ok(r) => r,
                    Err((r, _)) => r,
                };
                let mut p: Paquet = FluentBundle::new_concurrent(vec![l.clone()]);
                // Sans marques d'isolement U+2068/U+2069 : Orca et le terminal les afficheraient.
                p.set_use_isolating(false);
                // Un identifiant en double est ignoré : la première définition reste.
                let _ = p.add_resource(res);
                p
            })
            .collect();
        Traducteur {
            langue: langue.clone(),
            chaine: paquets,
        }
    }

    /// Chaîne pour les langues demandées (voir [`langues_demandees`]) : première langue
    /// connue, puis anglais, puis français. Aucune langue demandée : français.
    pub fn pour_langues(demandees: &[LanguageIdentifier]) -> Self {
        let connue = |l: &LanguageIdentifier| {
            LANGUES
                .iter()
                .find(|(c, _)| *c == l.language.as_str())
                .map(|(c, _)| *c)
        };
        let code = if demandees.is_empty() {
            "fr"
        } else {
            demandees.iter().find_map(connue).unwrap_or("en")
        };
        let mut codes = vec![code];
        for c in ["en", "fr"] {
            if !codes.contains(&c) {
                codes.push(c);
            }
        }
        let source = |c: &str| {
            LANGUES
                .iter()
                .find(|(x, _)| *x == c)
                .map_or("", |(_, s)| *s)
        };
        let chaine: Vec<(LanguageIdentifier, &str)> =
            codes.iter().map(|c| (id_langue(c), source(c))).collect();
        Traducteur::nouveau(&id_langue(code), &chaine)
    }

    pub fn langue(&self) -> &LanguageIdentifier {
        &self.langue
    }

    pub fn t(&self, id: &str) -> String {
        self.ta(id, &[])
    }

    pub fn ta(&self, id: &str, args: &[(&str, ArgI18n)]) -> String {
        let mut fa = FluentArgs::new();
        for (nom, valeur) in args {
            match valeur {
                ArgI18n::Texte(s) => fa.set(*nom, FluentValue::from(s.clone())),
                ArgI18n::Nombre(n) => fa.set(*nom, FluentValue::from(*n)),
            }
        }
        for p in &self.chaine {
            if let Some(motif) = p.get_message(id).and_then(|m| m.value()) {
                let mut erreurs = Vec::new();
                return p
                    .format_pattern(motif, Some(&fa), &mut erreurs)
                    .into_owned();
            }
        }
        id.to_string()
    }
}

static ACTIF: RwLock<Option<Arc<Traducteur>>> = RwLock::new(None);

fn actif() -> Arc<Traducteur> {
    let lecture = ACTIF.read().unwrap_or_else(|e| e.into_inner());
    if let Some(t) = lecture.as_ref() {
        return t.clone();
    }
    drop(lecture);
    let mut ecriture = ACTIF.write().unwrap_or_else(|e| e.into_inner());
    ecriture
        .get_or_insert_with(|| Arc::new(Traducteur::pour_langues(&[])))
        .clone()
}

/// À appeler une fois au démarrage de chaque binaire. Sans appel : français.
pub fn init(langues: &[LanguageIdentifier]) {
    let t = Arc::new(Traducteur::pour_langues(langues));
    *ACTIF.write().unwrap_or_else(|e| e.into_inner()) = Some(t);
}

/// Phrase traduite ; l'identifiant lui-même s'il est inconnu.
pub fn t(id: &str) -> String {
    actif().t(id)
}

/// Phrase traduite avec arguments.
pub fn ta(id: &str, args: &[(&str, ArgI18n)]) -> String {
    actif().ta(id, args)
}

pub fn langue_active() -> LanguageIdentifier {
    actif().langue().clone()
}

/// Aide de la langue active, sinon anglaise, sinon française.
pub fn aide() -> &'static str {
    let code = langue_active().language.as_str().to_string();
    for c in [code.as_str(), "en", "fr"] {
        if let Some((_, a)) = AIDES.iter().find(|(x, _)| *x == c) {
            return a;
        }
    }
    ""
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_de<'a>(paires: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |nom| {
            paires
                .iter()
                .find(|(n, _)| *n == nom)
                .map(|(_, v)| (*v).to_string())
        }
    }

    fn codes(l: &[LanguageIdentifier]) -> Vec<String> {
        l.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn langues_par_variables() {
        let e = env_de(&[("LANGUAGE", "nl:de"), ("LANG", "fr_BE.UTF-8")]);
        assert_eq!(codes(&langues_demandees(&e)), ["nl", "de"]);
        let e = env_de(&[("LANG", "de_AT.UTF-8")]);
        assert_eq!(codes(&langues_demandees(&e)), ["de-AT", "de"]);
        let e = env_de(&[("LANG", "C")]);
        assert!(langues_demandees(&e).is_empty());
        let e = env_de(&[("LC_ALL", "POSIX"), ("LANG", "fr_FR.UTF-8")]);
        assert!(langues_demandees(&e).is_empty());
        let e = env_de(&[("LC_MESSAGES", "es_ES@euro"), ("LANG", "fr_FR.UTF-8")]);
        assert_eq!(codes(&langues_demandees(&e)), ["es-ES", "es"]);
        let e = env_de(&[("LANGUAGE", ""), ("LANG", "it_IT.UTF-8")]);
        assert_eq!(codes(&langues_demandees(&e)), ["it-IT", "it"]);
        assert!(langues_demandees(&env_de(&[])).is_empty());
    }

    fn lid(s: &str) -> LanguageIdentifier {
        s.parse().unwrap()
    }

    #[test]
    fn repli_par_phrase() {
        let sources = [
            (lid("de"), "seulement-fr = x\n"),
            (lid("en"), "avec-en = anglais\n"),
            (lid("fr"), "seulement-fr = français\navec-en = français\n"),
        ];
        let t = Traducteur::nouveau(
            &lid("de"),
            &[sources[0].clone(), sources[1].clone(), sources[2].clone()],
        );
        assert_eq!(t.t("seulement-fr"), "x");
        assert_eq!(t.t("avec-en"), "anglais");
        let t = Traducteur::nouveau(&lid("nl"), &[sources[1].clone(), sources[2].clone()]);
        assert_eq!(t.t("avec-en"), "anglais");
        assert_eq!(t.t("seulement-fr"), "français");
    }

    #[test]
    fn identifiant_inconnu_sans_plantage() {
        let t = Traducteur::nouveau(&lid("fr"), &[(lid("fr"), "a = b\n")]);
        assert_eq!(t.t("n-existe-pas"), "n-existe-pas");
        assert_eq!(
            t.ta("n-existe-pas", &[("x", ArgI18n::Nombre(1))]),
            "n-existe-pas"
        );
        assert_eq!(t.ta("a", &[]), "b");
    }

    const PLURIEL: &str =
        "pb = { $n ->\n    [one] { $n } problème\n    *[other] { $n } problèmes\n}\n";

    #[test]
    fn pluriel() {
        let t = Traducteur::nouveau(&lid("fr"), &[(lid("fr"), PLURIEL)]);
        assert_eq!(t.ta("pb", &[("n", ArgI18n::Nombre(1))]), "1 problème");
        assert_eq!(t.ta("pb", &[("n", ArgI18n::Nombre(3))]), "3 problèmes");
    }

    #[test]
    fn sans_marques_d_isolement() {
        let t = Traducteur::nouveau(&lid("fr"), &[(lid("fr"), "salut = Bonjour { $nom } !\n")]);
        let s = t.ta("salut", &[("nom", ArgI18n::Texte("Marie".into()))]);
        assert_eq!(s, "Bonjour Marie !");
        assert!(!s.contains('\u{2068}') && !s.contains('\u{2069}'));
    }

    #[test]
    fn tous_les_ftl_valides() {
        for (code, source) in sources_integrees() {
            if let Err(e) = verifier_source(source) {
                panic!("{code} : {e}");
            }
        }
    }

    #[test]
    fn globale_sans_init_est_en_francais() {
        // Aucun test de cette crate n'appelle `init` : le français est la langue par défaut.
        assert_eq!(langue_active().to_string(), "fr");
        assert_eq!(
            t("identifiant-qui-n-existe-pas"),
            "identifiant-qui-n-existe-pas"
        );
        assert!(aide().contains('#'));
    }

    #[test]
    fn langue_active_selon_demande() {
        let t = Traducteur::pour_langues(&[lid("de-AT"), lid("de")]);
        assert_eq!(t.langue().to_string(), "de");
        let t = Traducteur::pour_langues(&[lid("xx")]);
        assert_eq!(t.langue().to_string(), "en");
        let t = Traducteur::pour_langues(&[]);
        assert_eq!(t.langue().to_string(), "fr");
    }
}
