// SPDX-License-Identifier: GPL-3.0-or-later
//! Cohérence entre le code, les fichiers de traduction et le lanceur du bureau.
#![allow(clippy::unwrap_used)]

use fluent_syntax::ast::{Entry, Expression, InlineExpression, Pattern, PatternElement, Resource};
use fluent_syntax::parser;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

fn racine() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fichiers_rs(dossier: &Path, sortie: &mut Vec<PathBuf>) {
    let Ok(entrees) = fs::read_dir(dossier) else {
        return;
    };
    for e in entrees.flatten() {
        let p = e.path();
        if p.is_dir() {
            let nom = e.file_name();
            if nom != "target" && nom != "fuzz" {
                fichiers_rs(&p, sortie);
            }
        } else if p.extension().is_some_and(|x| x == "rs") {
            sortie.push(p);
        }
    }
}

/// Sources de tous les crates (code et tests), sauf ce fichier et le code de `pupitre-i18n`
/// lui-même (ses tests emploient de faux identifiants).
fn sources() -> Vec<(PathBuf, String)> {
    let mut v = Vec::new();
    fichiers_rs(&racine().join("crates"), &mut v);
    v.into_iter()
        .filter(|p| !p.ends_with("i18n/tests/sources.rs") && !p.ends_with("i18n/src/lib.rs"))
        .map(|p| {
            let s = fs::read_to_string(&p).unwrap();
            (p, s)
        })
        .collect()
}

fn ftl(langue: &str) -> String {
    fs::read_to_string(racine().join(format!("crates/i18n/langues/{langue}/pupitre.ftl"))).unwrap()
}

fn analyser(source: &str) -> Resource<&str> {
    match parser::parse(source) {
        Ok(r) => r,
        Err((r, _)) => r,
    }
}

fn identifiants(source: &str) -> BTreeSet<String> {
    analyser(source)
        .body
        .iter()
        .filter_map(|e| match e {
            Entry::Message(m) => Some(m.id.name.to_string()),
            _ => None,
        })
        .collect()
}

fn variables_motif(p: &Pattern<&str>, sortie: &mut BTreeSet<String>) {
    for e in &p.elements {
        if let PatternElement::Placeable { expression } = e {
            variables_expression(expression, sortie);
        }
    }
}

fn variables_inline(e: &InlineExpression<&str>, sortie: &mut BTreeSet<String>) {
    match e {
        InlineExpression::VariableReference { id } => {
            sortie.insert(id.name.to_string());
        }
        InlineExpression::Placeable { expression } => variables_expression(expression, sortie),
        InlineExpression::FunctionReference { arguments, .. }
        | InlineExpression::TermReference {
            arguments: Some(arguments),
            ..
        } => {
            for a in &arguments.positional {
                variables_inline(a, sortie);
            }
            for a in &arguments.named {
                variables_inline(&a.value, sortie);
            }
        }
        _ => {}
    }
}

fn variables_expression(e: &Expression<&str>, sortie: &mut BTreeSet<String>) {
    match e {
        Expression::Inline(i) => variables_inline(i, sortie),
        Expression::Select { selector, variants } => {
            variables_inline(selector, sortie);
            for v in variants {
                variables_motif(&v.value, sortie);
            }
        }
    }
}

fn arguments_par_message(source: &str) -> BTreeMap<String, BTreeSet<String>> {
    let mut m = BTreeMap::new();
    for e in &analyser(source).body {
        if let Entry::Message(msg) = e {
            let mut v = BTreeSet::new();
            if let Some(p) = &msg.value {
                variables_motif(p, &mut v);
            }
            m.insert(msg.id.name.to_string(), v);
        }
    }
    m
}

/// Identifiants passés en littéral à `t(`, `tr(` ou `ta(`.
fn identifiants_appeles(code: &str) -> Vec<String> {
    let octets = code.as_bytes();
    let mut trouves = Vec::new();
    for nom in ["t(", "tr(", "ta("] {
        let mut debut = 0;
        while let Some(i) = code[debut..].find(nom) {
            let pos = debut + i;
            debut = pos + nom.len();
            let avant = if pos == 0 { b' ' } else { octets[pos - 1] };
            if avant.is_ascii_alphanumeric() || avant == b'_' || avant == b'.' {
                continue;
            }
            let reste = code[debut..].trim_start();
            if let Some(litteral) = reste.strip_prefix('"') {
                if let Some(fin) = litteral.find('"') {
                    trouves.push(litteral[..fin].to_string());
                }
            }
        }
    }
    trouves
}

#[test]
fn identifiants_utilises_existent_en_fr() {
    let connus = identifiants(&ftl("fr"));
    let mut manquants = Vec::new();
    let mut appels = 0;
    for (chemin, code) in sources() {
        for id in identifiants_appeles(&code) {
            appels += 1;
            if !connus.contains(&id) {
                manquants.push(format!("{} : {id}", chemin.display()));
            }
        }
    }
    assert!(
        appels > 50,
        "la recherche ne trouve presque rien : {appels}"
    );
    assert!(manquants.is_empty(), "absents de fr.ftl : {manquants:#?}");
}

#[test]
fn aucun_identifiant_orphelin() {
    // Le test du lanceur du bureau (ce fichier) emploie les identifiants `bureau-*`.
    let mut tout: String = sources().into_iter().map(|(_, s)| s).collect();
    tout.push_str(include_str!("sources.rs"));
    let orphelins: Vec<String> = identifiants(&ftl("fr"))
        .into_iter()
        .filter(|id| !tout.contains(&format!("\"{id}\"")))
        .collect();
    assert!(orphelins.is_empty(), "jamais utilisés : {orphelins:#?}");
}

#[test]
fn memes_arguments_partout() {
    let fr = arguments_par_message(&ftl("fr"));
    for langue in ["en", "es", "it", "de", "nl"] {
        for (id, vars) in arguments_par_message(&ftl(langue)) {
            let Some(attendus) = fr.get(&id) else {
                panic!("{langue} : « {id} » n'existe pas en français");
            };
            assert!(
                vars.is_subset(attendus),
                "{langue} : « {id} » utilise {vars:?}, la phrase française n'a que {attendus:?}"
            );
        }
    }
}

fn valeur_desktop(contenu: &str, groupe: &str, cle: &str) -> Option<String> {
    let mut dans = false;
    for l in contenu.lines() {
        if l.starts_with('[') {
            dans = l == format!("[{groupe}]");
        } else if dans {
            if let Some(v) = l.strip_prefix(&format!("{cle}=")) {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Le lanceur reste un fichier (les bureaux lisent `Name[de]`…) : ses entrées françaises
/// doivent dire la même chose que `fr.ftl`.
#[test]
fn lanceur_du_bureau_concorde_avec_fr_ftl() {
    let d = fs::read_to_string(racine().join("assets/fr.pupitre.Pupitre.desktop")).unwrap();
    let source = ftl("fr");
    let res = analyser(&source);
    let texte = |id: &str| -> String {
        res.body
            .iter()
            .find_map(|e| match e {
                Entry::Message(m) if m.id.name == id => m.value.as_ref(),
                _ => None,
            })
            .map(|p| {
                p.elements
                    .iter()
                    .map(|e| match e {
                        PatternElement::TextElement { value } => (*value).to_string(),
                        PatternElement::Placeable { .. } => String::new(),
                    })
                    .collect()
            })
            .unwrap_or_else(|| panic!("« {id} » absent de fr.ftl"))
    };
    let entree = "Desktop Entry";
    for (cle, id) in [
        ("Name[fr]", "bureau-nom"),
        ("GenericName[fr]", "bureau-nom-generique"),
        ("Comment[fr]", "bureau-commentaire"),
        ("Keywords[fr]", "bureau-mots-cles"),
    ] {
        assert_eq!(
            valeur_desktop(&d, entree, cle).as_deref(),
            Some(texte(id).as_str()),
            "{cle}"
        );
    }
    assert_eq!(
        valeur_desktop(&d, "Desktop Action nouvelle-fenetre", "Name[fr]").as_deref(),
        Some(texte("bureau-nouvelle-fenetre").as_str())
    );
}

/// Chaque phrase française se formate sans erreur Fluent (variable manquante, référence
/// inconnue…) quand tous ses arguments sont fournis.
#[test]
fn phrases_francaises_se_formatent_sans_erreur() {
    use fluent_bundle::{FluentArgs, FluentBundle, FluentResource};
    let source = ftl("fr");
    let ressource = FluentResource::try_new(source.clone()).unwrap();
    let mut paquet = FluentBundle::new(vec!["fr".parse().unwrap()]);
    paquet.set_use_isolating(false);
    paquet.add_resource(ressource).unwrap();
    for (id, vars) in arguments_par_message(&source) {
        let mut args = FluentArgs::new();
        for v in &vars {
            args.set(v.as_str(), 2);
        }
        let motif = paquet.get_message(&id).unwrap().value().unwrap();
        let mut erreurs = Vec::new();
        let texte = paquet.format_pattern(motif, Some(&args), &mut erreurs);
        assert!(erreurs.is_empty(), "{id} : {erreurs:?}");
        assert!(!texte.is_empty(), "{id} est vide");
    }
}
