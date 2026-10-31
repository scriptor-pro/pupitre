// SPDX-License-Identifier: GPL-3.0-or-later
//! Alias d'hôtes déclarés dans `~/.ssh/config` (lignes `Host`, `Include` suivis).

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Taille maximale lue par fichier : au-delà, le reste est ignoré.
const TAILLE_MAX: u64 = 1024 * 1024;
/// Profondeur maximale d'`Include` imbriqués.
const PROFONDEUR_MAX: usize = 8;
/// Nombre maximal de fichiers lus au total (protège des inclusions démesurées).
const FICHIERS_MAX: usize = 512;

struct Etat {
    /// Dossier des chemins `Include` relatifs (celui du fichier de départ, normalement `~/.ssh`).
    base: PathBuf,
    hotes: Vec<String>,
    vus: HashSet<String>,
    /// Fichiers en cours de lecture (chemins canoniques) : détecte les boucles.
    en_cours: HashSet<PathBuf>,
    lus: usize,
}

/// Alias des lignes `Host` du fichier et de ses `Include`, sans motif (`*`, `?`, `!`), sans
/// alias commençant par `-`, sans doublon, dans l'ordre. Fichier absent ou illisible : vide.
pub fn hotes_ssh_config(fichier: &Path) -> Vec<String> {
    let base = match fichier.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let mut etat = Etat {
        base,
        hotes: Vec::new(),
        vus: HashSet::new(),
        en_cours: HashSet::new(),
        lus: 0,
    };
    lire_fichier(fichier, 0, &mut etat);
    etat.hotes
}

fn lire_fichier(chemin: &Path, profondeur: usize, etat: &mut Etat) {
    if profondeur > PROFONDEUR_MAX || etat.lus >= FICHIERS_MAX {
        return;
    }
    let Ok(canon) = std::fs::canonicalize(chemin) else {
        return;
    };
    if etat.en_cours.contains(&canon) {
        return;
    }
    let Some(texte) = lire_borne(&canon) else {
        return;
    };
    etat.lus += 1;
    etat.en_cours.insert(canon.clone());
    for ligne in texte.lines() {
        let Some((mot, args)) = analyser(ligne) else {
            continue;
        };
        if mot.eq_ignore_ascii_case("host") {
            for a in args {
                ajouter(&a, etat);
            }
        } else if mot.eq_ignore_ascii_case("include") {
            for a in args {
                inclure(&a, profondeur, etat);
            }
        }
    }
    etat.en_cours.remove(&canon);
}

/// Contenu du fichier ordinaire (borné, décodé sans échec), ou `None`.
fn lire_borne(chemin: &Path) -> Option<String> {
    let f = std::fs::File::open(chemin).ok()?;
    if !f.metadata().ok()?.is_file() {
        return None;
    }
    let mut octets = Vec::new();
    f.take(TAILLE_MAX).read_to_end(&mut octets).ok()?;
    Some(String::from_utf8_lossy(&octets).into_owned())
}

fn ajouter(alias: &str, etat: &mut Etat) {
    let ok = !alias.is_empty()
        && !alias.starts_with('-')
        && !alias.contains(['*', '?', '!'])
        && !alias.chars().any(char::is_control);
    if ok && etat.vus.insert(alias.to_owned()) {
        etat.hotes.push(alias.to_owned());
    }
}

fn inclure(motif: &str, profondeur: usize, etat: &mut Etat) {
    let complet = if let Some(reste) = motif.strip_prefix("~/") {
        match std::env::home_dir() {
            Some(h) => h.join(reste),
            None => return,
        }
    } else if Path::new(motif).is_absolute() {
        PathBuf::from(motif)
    } else {
        etat.base.join(motif)
    };
    let Some(motif) = complet.to_str() else {
        return;
    };
    let Ok(chemins) = glob::glob(motif) else {
        return;
    };
    // `take` borne aussi un motif qui correspondrait à une multitude de fichiers.
    for chemin in chemins.take(FICHIERS_MAX).flatten() {
        lire_fichier(&chemin, profondeur + 1, etat);
    }
}

/// Sépare une ligne en mot-clé et arguments (`Mot val val`, `Mot=val`, `Mot = "val"`).
/// `None` pour une ligne vide ou en commentaire.
fn analyser(ligne: &str) -> Option<(&str, Vec<String>)> {
    let ligne = ligne.trim_start();
    if ligne.is_empty() || ligne.starts_with('#') {
        return None;
    }
    let fin = ligne
        .find(|c: char| c.is_whitespace() || c == '=')
        .unwrap_or(ligne.len());
    let (mot, reste) = ligne.split_at(fin);
    let reste = reste.trim_start();
    let reste = reste.strip_prefix('=').unwrap_or(reste);
    Some((mot, arguments(reste)))
}

/// Découpe en arguments séparés par des blancs ; les guillemets doubles groupent ; un argument
/// non quoté commençant par `#` termine la ligne.
fn arguments(s: &str) -> Vec<String> {
    let mut res = Vec::new();
    let mut courant = String::new();
    let mut en_cours = false;
    let mut guillemets = false;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if guillemets {
            match c {
                '"' => guillemets = false,
                '\\' => match chars.next() {
                    Some(s @ ('"' | '\\')) => courant.push(s),
                    Some(autre) => {
                        courant.push('\\');
                        courant.push(autre);
                    }
                    None => courant.push('\\'),
                },
                _ => courant.push(c),
            }
        } else if c == '"' {
            guillemets = true;
            en_cours = true;
        } else if c.is_whitespace() {
            if en_cours {
                res.push(std::mem::take(&mut courant));
                en_cours = false;
            }
        } else if c == '#' && !en_cours {
            return res;
        } else {
            courant.push(c);
            en_cours = true;
        }
    }
    if en_cours {
        res.push(courant);
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hotes(texte: &str) -> Vec<String> {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config");
        std::fs::write(&p, texte).unwrap();
        hotes_ssh_config(&p)
    }

    #[test]
    fn hotes_simples() {
        assert_eq!(hotes("Host a b\n  HostName x\nHost c\n"), ["a", "b", "c"]);
    }

    #[test]
    fn motifs_ignores() {
        assert_eq!(
            hotes("Host *\nHost web-* !bad ok\nHost a? b\n# Host cache\n"),
            ["ok", "b"]
        );
    }

    #[test]
    fn alias_avec_tiret_ignore() {
        assert_eq!(hotes("Host -oProxyCommand=x bon\n"), ["bon"]);
    }

    #[test]
    fn host_egal() {
        assert_eq!(
            hotes("host=a\nHOST = b\nHost\t\"c\" a\n   Host =d # fin\n"),
            ["a", "b", "c", "d"]
        );
    }

    #[test]
    fn include_relatif_et_glob() {
        let d = tempfile::tempdir().unwrap();
        let ssh = d.path().join("ssh");
        std::fs::create_dir_all(ssh.join("conf.d")).unwrap();
        std::fs::write(
            ssh.join("config"),
            "Host un\nInclude conf.d/*.conf extra\nHost fin\n",
        )
        .unwrap();
        std::fs::write(ssh.join("conf.d/a.conf"), "Host deux\n").unwrap();
        std::fs::write(ssh.join("conf.d/b.conf"), "Host trois un\n").unwrap();
        std::fs::write(ssh.join("conf.d/ignore.txt"), "Host non\n").unwrap();
        std::fs::write(ssh.join("extra"), "Host quatre\n").unwrap();
        assert_eq!(
            hotes_ssh_config(&ssh.join("config")),
            ["un", "deux", "trois", "quatre", "fin"]
        );
    }

    #[test]
    fn include_en_boucle_termine() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("config"), "Host a\nInclude autre\n").unwrap();
        std::fs::write(
            d.path().join("autre"),
            "Host b\nInclude config\nInclude autre\n",
        )
        .unwrap();
        assert_eq!(hotes_ssh_config(&d.path().join("config")), ["a", "b"]);
    }

    #[test]
    fn include_trop_profond_borne() {
        let d = tempfile::tempdir().unwrap();
        for i in 0..20 {
            std::fs::write(
                d.path().join(format!("f{i}")),
                format!("Host h{i}\nInclude f{}\n", i + 1),
            )
            .unwrap();
        }
        let v = hotes_ssh_config(&d.path().join("f0"));
        assert_eq!(v.len(), PROFONDEUR_MAX + 1);
    }

    #[test]
    fn absent_ou_illisible_vide() {
        let d = tempfile::tempdir().unwrap();
        assert!(hotes_ssh_config(&d.path().join("rien")).is_empty());
        assert!(hotes_ssh_config(d.path()).is_empty());
    }

    #[test]
    fn entrees_bizarres_sans_panique() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config");
        let mut octets = b"Host a\n\xff\xfe\x00Host b\n\"\n=\n Host\n Include \"\n".to_vec();
        octets.extend(std::iter::repeat_n(b'x', 3 * 1024 * 1024));
        octets.extend(b"\nHost apres\n");
        std::fs::write(&p, octets).unwrap();
        let v = hotes_ssh_config(&p);
        assert!(v.contains(&"a".to_owned()));
        assert!(!v.contains(&"apres".to_owned()));
        assert!(hotes("Host \"ouvert\nInclude [\n\\").len() <= 1);
        assert!(hotes("Host a\x07b ok\n").contains(&"ok".to_owned()));
    }
}
