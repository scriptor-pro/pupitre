// SPDX-License-Identifier: GPL-3.0-or-later
//! Phrases de passe des clés SSH dans le trousseau du système (Secret Service).
//!
//! Règles : on ne déverrouille jamais le trousseau, on ne crée jamais de
//! collection, et toute erreur D-Bus se traduit par « absent » (jamais de
//! panique). Le service est cherché à chaque appel : il peut apparaître ou
//! disparaître pendant la vie de Pupitre.
//!
//! Temps borné : la bibliothèque n'offre pas de délai ; on s'appuie sur les
//! échecs rapides mesurés (~5 ms sans bus, ~150 ms si le service n'existe pas),
//! voir `essai-jetable/ssh/RESULTATS.md`.

use secret_service::blocking::{Collection, SecretService};
use secret_service::EncryptionType;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Mutex};
use std::time::Duration;
use zeroize::Zeroizing;

pub use pupitre_proto::ssh::EtatTrousseau;

const APPLICATION: &str = "pupitre";
const TYPE: &str = "phrase-cle-ssh";

/// Échec d'une écriture ou d'une suppression dans le trousseau.
#[derive(Debug, Clone, PartialEq)]
pub enum ErreurTrousseau {
    Verrouille,
    Absent,
    Autre(String),
}

impl fmt::Display for ErreurTrousseau {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Verrouille => write!(f, "le trousseau est verrouillé"),
            Self::Absent => write!(f, "aucun trousseau disponible"),
            Self::Autre(m) => write!(f, "erreur du trousseau : {m}"),
        }
    }
}

impl std::error::Error for ErreurTrousseau {}

/// Dépôt des phrases de passe, indexées par le chemin absolu de la clé.
pub trait Coffre {
    /// État du trousseau, sans jamais le déverrouiller.
    fn etat(&self) -> EtatTrousseau;
    /// Phrase enregistrée pour `cle` ; `None` si absente ou trousseau verrouillé.
    fn lire(&self, cle: &Path) -> Option<Zeroizing<String>>;
    fn ecrire(&self, cle: &Path, phrase: &str) -> Result<(), ErreurTrousseau>;
    /// Efface la phrase de `cle` ; une phrase inexistante n'est pas une erreur.
    fn oublier(&self, cle: &Path) -> Result<(), ErreurTrousseau>;
    /// Chemins des clés dont une phrase est enregistrée (triés).
    fn cles(&self) -> Vec<PathBuf>;
}

/// Remplace le dossier personnel par `~` pour l'affichage.
pub fn chemin_lisible(p: &Path) -> String {
    if let Some(home) = std::env::home_dir() {
        if let Ok(reste) = p.strip_prefix(&home) {
            return if reste.as_os_str().is_empty() {
                "~".to_string()
            } else {
                format!("~/{}", reste.display())
            };
        }
    }
    p.display().to_string()
}

/// Coffre en mémoire, pour les tests.
#[derive(Default)]
pub struct CoffreMemoire {
    phrases: Mutex<BTreeMap<PathBuf, Zeroizing<String>>>,
}

impl Coffre for CoffreMemoire {
    fn etat(&self) -> EtatTrousseau {
        EtatTrousseau::Disponible
    }
    fn lire(&self, cle: &Path) -> Option<Zeroizing<String>> {
        let m = self.phrases.lock().ok()?;
        m.get(cle).map(|p| Zeroizing::new(p.to_string()))
    }
    fn ecrire(&self, cle: &Path, phrase: &str) -> Result<(), ErreurTrousseau> {
        let mut m = self.phrases.lock().map_err(|_| ErreurTrousseau::Absent)?;
        // L'ancienne valeur éventuelle est effacée à sa destruction.
        m.insert(cle.to_path_buf(), Zeroizing::new(phrase.to_string()));
        Ok(())
    }
    fn oublier(&self, cle: &Path) -> Result<(), ErreurTrousseau> {
        let mut m = self.phrases.lock().map_err(|_| ErreurTrousseau::Absent)?;
        m.remove(cle);
        Ok(())
    }
    fn cles(&self) -> Vec<PathBuf> {
        self.phrases
            .lock()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    }
}

/// Délai maximal accordé à une opération du Secret Service.
pub const DELAI_TROUSSEAU: Duration = Duration::from_secs(2);

/// Coffre adossé au Secret Service (collection par défaut).
pub struct CoffreSecretService {
    _prive: (),
}

/// Résultat de la recherche de la collection par défaut.
enum Acces<'a> {
    Pret(Collection<'a>),
    Verrouille,
    Absent,
}

impl CoffreSecretService {
    /// N'échoue jamais : le service est cherché à chaque appel.
    pub fn ouvrir() -> Self {
        Self { _prive: () }
    }

    /// Exécute `f` sur un fil à part et attend au plus `DELAI_TROUSSEAU` ;
    /// passé ce délai (ou si le fil plante), rend `defaut`. Le fil en retard
    /// est abandonné : il finira de lui-même.
    fn avec_collection<T: Send + 'static>(
        &self,
        defaut: T,
        f: impl FnOnce(&Acces<'_>) -> T + Send + 'static,
    ) -> T {
        let (tx, rx) = mpsc::channel();
        let lance = std::thread::Builder::new()
            .name("pupitre-trousseau".into())
            .spawn(move || {
                let _ = tx.send(Self::acceder(f));
            });
        if lance.is_err() {
            return defaut;
        }
        rx.recv_timeout(DELAI_TROUSSEAU).unwrap_or(defaut)
    }

    fn acceder<T>(f: impl FnOnce(&Acces<'_>) -> T) -> T {
        let Ok(service) = SecretService::connect(EncryptionType::Dh) else {
            return f(&Acces::Absent);
        };
        // Pas de collection par défaut : jamais créée par Pupitre.
        let Ok(col) = service.get_default_collection() else {
            return f(&Acces::Absent);
        };
        match col.is_locked() {
            Ok(false) => f(&Acces::Pret(col)),
            Ok(true) => f(&Acces::Verrouille),
            Err(_) => f(&Acces::Absent),
        }
    }
}

fn attributs(cle: &str) -> HashMap<&str, &str> {
    HashMap::from([("application", APPLICATION), ("type", TYPE), ("cle", cle)])
}

impl Coffre for CoffreSecretService {
    fn etat(&self) -> EtatTrousseau {
        self.avec_collection(EtatTrousseau::Absent, |a| match a {
            Acces::Pret(_) => EtatTrousseau::Disponible,
            Acces::Verrouille => EtatTrousseau::Verrouille,
            Acces::Absent => EtatTrousseau::Absent,
        })
    }

    fn lire(&self, cle: &Path) -> Option<Zeroizing<String>> {
        let cle = cle.to_str()?.to_string();
        self.avec_collection(None, move |a| {
            let Acces::Pret(col) = a else { return None };
            let items = col.search_items(attributs(&cle)).ok()?;
            let secret = Zeroizing::new(items.first()?.get_secret().ok()?);
            // UTF-8 invalide : traité comme absent (octets effacés).
            match String::from_utf8(secret.to_vec()) {
                Ok(s) => Some(Zeroizing::new(s)),
                Err(e) => {
                    drop(Zeroizing::new(e.into_bytes()));
                    None
                }
            }
        })
    }

    fn ecrire(&self, cle: &Path, phrase: &str) -> Result<(), ErreurTrousseau> {
        let chemin = cle
            .to_str()
            .ok_or_else(|| ErreurTrousseau::Autre("chemin non UTF-8".into()))?
            .to_string();
        let libelle = format!("Pupitre — phrase de passe de {}", chemin_lisible(cle));
        let phrase = Zeroizing::new(phrase.to_string());
        self.avec_collection(Err(ErreurTrousseau::Absent), move |a| match a {
            Acces::Absent => Err(ErreurTrousseau::Absent),
            Acces::Verrouille => Err(ErreurTrousseau::Verrouille),
            Acces::Pret(col) => col
                .create_item(
                    &libelle,
                    attributs(&chemin),
                    phrase.as_bytes(),
                    true,
                    "text/plain",
                )
                .map(|_| ())
                .map_err(|e| ErreurTrousseau::Autre(e.to_string())),
        })
    }

    fn oublier(&self, cle: &Path) -> Result<(), ErreurTrousseau> {
        let chemin = cle
            .to_str()
            .ok_or_else(|| ErreurTrousseau::Autre("chemin non UTF-8".into()))?
            .to_string();
        self.avec_collection(Err(ErreurTrousseau::Absent), move |a| match a {
            Acces::Absent => Err(ErreurTrousseau::Absent),
            Acces::Verrouille => Err(ErreurTrousseau::Verrouille),
            Acces::Pret(col) => {
                let items = col
                    .search_items(attributs(&chemin))
                    .map_err(|e| ErreurTrousseau::Autre(e.to_string()))?;
                for item in items {
                    item.delete()
                        .map_err(|e| ErreurTrousseau::Autre(e.to_string()))?;
                }
                Ok(())
            }
        })
    }

    fn cles(&self) -> Vec<PathBuf> {
        self.avec_collection(Vec::new(), |a| {
            let Acces::Pret(col) = a else {
                return Vec::new();
            };
            let filtre = HashMap::from([("application", APPLICATION), ("type", TYPE)]);
            let Ok(items) = col.search_items(filtre) else {
                return Vec::new();
            };
            let mut chemins: Vec<PathBuf> = items
                .iter()
                .filter_map(|i| i.get_attributes().ok()?.remove("cle"))
                .map(PathBuf::from)
                .collect();
            chemins.sort();
            chemins.dedup();
            chemins
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn ecrire_puis_lire() {
        let c = CoffreMemoire::default();
        let k = Path::new("/a/cle");
        assert!(c.lire(k).is_none());
        c.ecrire(k, "mot").unwrap();
        assert_eq!(c.lire(k).as_deref().map(String::as_str), Some("mot"));
        assert_eq!(c.etat(), EtatTrousseau::Disponible);
    }

    #[test]
    fn oublier_retire() {
        let c = CoffreMemoire::default();
        let k = Path::new("/a/cle");
        c.ecrire(k, "mot").unwrap();
        c.oublier(k).unwrap();
        assert!(c.lire(k).is_none());
        c.oublier(k).unwrap();
    }

    #[test]
    fn cles_liste_les_chemins() {
        let c = CoffreMemoire::default();
        c.ecrire(Path::new("/b"), "1").unwrap();
        c.ecrire(Path::new("/a"), "2").unwrap();
        assert_eq!(c.cles(), vec![PathBuf::from("/a"), PathBuf::from("/b")]);
    }

    #[test]
    fn tilde_pour_le_dossier_personnel() {
        let home = std::env::home_dir().unwrap();
        assert_eq!(
            chemin_lisible(&home.join(".ssh/id_ed25519")),
            "~/.ssh/id_ed25519"
        );
        assert_eq!(chemin_lisible(&home), "~");
        assert_eq!(chemin_lisible(Path::new("/opt/cle")), "/opt/cle");
    }

    #[test]
    fn erreurs_en_francais() {
        assert_eq!(
            ErreurTrousseau::Verrouille.to_string(),
            "le trousseau est verrouillé"
        );
        assert_eq!(
            ErreurTrousseau::Absent.to_string(),
            "aucun trousseau disponible"
        );
    }
}
