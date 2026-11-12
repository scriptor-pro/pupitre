// SPDX-License-Identifier: GPL-3.0-or-later
// Essai jetable : modifier `theme` avec toml_edit garde-t-il le reste octet pour octet ?
// Deux méthodes comparées : `doc["theme"] = value(..)` (naïve) et la même avec la décoration
// (espaces et commentaire de fin de ligne) recopiée sur la nouvelle valeur.
use std::{env, fs, process::ExitCode};
use toml_edit::{value, DocumentMut};

/// Premier octet où deux textes diffèrent (None s'ils sont identiques).
fn premiere_diff(a: &str, b: &str) -> Option<String> {
    if a == b {
        return None;
    }
    let i = a
        .bytes()
        .zip(b.bytes())
        .position(|(x, y)| x != y)
        .unwrap_or_else(|| a.len().min(b.len()));
    Some(format!(
        "octet {i} : {:?} / {:?}",
        &a[a.floor_char_boundary(i)..a.len().min(a.floor_char_boundary(i) + 30)],
        &b[b.floor_char_boundary(i)..b.len().min(b.floor_char_boundary(i) + 30)]
    ))
}

fn main() -> ExitCode {
    let mut ok = true;
    for chemin in env::args().skip(1) {
        let Ok(src) = fs::read_to_string(&chemin) else {
            println!("{chemin}: illisible");
            ok = false;
            continue;
        };
        let doc: DocumentMut = match src.parse() {
            Ok(d) => d,
            Err(e) => {
                println!("{chemin}: erreur d'analyse {e}");
                ok = false;
                continue;
            }
        };
        println!("== {chemin}");
        let rt = premiere_diff(&doc.to_string(), &src);
        println!("  aller-retour sans modification : {}", rt.as_deref().unwrap_or("identique"));
        ok &= rt.is_none();

        let Some(avant) = doc.get("theme").and_then(|v| v.as_str()).map(String::from) else {
            println!("  (pas de clé theme)");
            continue;
        };
        // Texte attendu : on remplace à la main, sur la ligne qui commence par `theme`.
        let attendu: String = src
            .split_inclusive('\n')
            .map(|l| {
                if l.trim_start().starts_with("theme") && l.contains('=') {
                    l.replacen(&format!("\"{avant}\""), "\"sombre\"", 1)
                } else {
                    l.to_string()
                }
            })
            .collect();

        // Méthode naïve.
        let mut d1 = doc.clone();
        d1["theme"] = value("sombre");
        let r1 = premiere_diff(&d1.to_string(), &attendu);
        println!("  naïve  `doc[\"theme\"] = value(..)` : {}", r1.as_deref().unwrap_or("seule la valeur change"));

        // Méthode avec décoration recopiée.
        let mut d2 = doc.clone();
        let deco = d2["theme"].as_value().map(|v| v.decor().clone());
        d2["theme"] = value("sombre");
        if let (Some(deco), Some(v)) = (deco, d2["theme"].as_value_mut()) {
            *v.decor_mut() = deco;
        }
        let r2 = premiere_diff(&d2.to_string(), &attendu);
        println!("  décor recopié                     : {}", r2.as_deref().unwrap_or("seule la valeur change"));
        ok &= r2.is_none();

        // Retour à la valeur d'origine (même méthode).
        let deco = d2["theme"].as_value().map(|v| v.decor().clone());
        d2["theme"] = value(avant.as_str());
        if let (Some(deco), Some(v)) = (deco, d2["theme"].as_value_mut()) {
            *v.decor_mut() = deco;
        }
        let r3 = premiere_diff(&d2.to_string(), &src);
        println!("  retour à l'origine (décor recopié) : {}", r3.as_deref().unwrap_or("identique"));
        ok &= r3.is_none();
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
