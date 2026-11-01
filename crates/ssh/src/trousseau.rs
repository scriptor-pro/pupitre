// SPDX-License-Identifier: GPL-3.0-or-later
//! Phrases de passe des clés SSH dans le trousseau du système (Secret Service).
//!
//! Règles : on ne déverrouille jamais le trousseau, on ne crée jamais de
//! collection, et aucune erreur D-Bus ne fait paniquer : service ou collection
//! introuvables donnent « absent », les autres échecs une [`ErreurTrousseau`].
//! Le service est cherché à chaque appel : il peut apparaître ou disparaître
//! pendant la vie de Pupitre.
//!
//! Temps borné : la bibliothèque n'offre pas de délai. Chaque opération tourne
//! donc sur un fil à part, attendu au plus [`DELAI_TROUSSEAU`] ; au-delà, elle
//! rend [`ErreurTrousseau::Delai`] (« le trousseau ne répond pas »), qui n'est
//! jamais confondu avec une entrée absente ni avec un trousseau absent. Le fil
//! en retard est abandonné et finit de lui-même (fuite bornée par le nombre
//! d'opérations). Seul [`Coffre::etat`] rend « absent » dans ce cas : son type
//! voyage dans le protocole et n'a que trois valeurs.
//!
//! Course connue : entre `is_locked` et `CreateItem`, le trousseau peut se
//! verrouiller ; le démon afficherait alors sa propre demande de déverrouillage.
//! La fenêtre est minuscule et la conséquence bénigne.

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

/// Échec d'une opération sur le trousseau.
#[derive(Debug, Clone, PartialEq)]
pub enum ErreurTrousseau {
    /// Trousseau présent mais verrouillé (jamais déverrouillé par Pupitre).
    Verrouille,
    /// Pas de Secret Service, ou pas de collection par défaut.
    Absent,
    /// Le Secret Service n'a pas répondu dans le délai : on ne sait rien de l'entrée.
    Delai,
    /// Toute autre erreur (D-Bus, chemin non UTF-8, phrase illisible…).
    Autre(String),
}

impl fmt::Display for ErreurTrousseau {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Verrouille => write!(f, "le trousseau est verrouillé"),
            Self::Absent => write!(f, "aucun trousseau disponible"),
            Self::Delai => write!(f, "le trousseau ne répond pas"),
            Self::Autre(m) => write!(f, "erreur du trousseau : {m}"),
        }
    }
}

impl std::error::Error for ErreurTrousseau {}

/// Dépôt des phrases de passe, indexées par le chemin absolu de la clé.
pub trait Coffre {
    /// État du trousseau, sans jamais le déverrouiller.
    fn etat(&self) -> EtatTrousseau;
    /// Phrase enregistrée pour `cle`. `Ok(None)` seulement si le trousseau est lisible et
    /// que l'entrée n'existe pas ; un trousseau verrouillé, absent ou muet est une erreur.
    fn lire(&self, cle: &Path) -> Result<Option<Zeroizing<String>>, ErreurTrousseau>;
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
    fn lire(&self, cle: &Path) -> Result<Option<Zeroizing<String>>, ErreurTrousseau> {
        let m = self.phrases.lock().map_err(|_| ErreurTrousseau::Absent)?;
        Ok(m.get(cle).map(|p| Zeroizing::new(p.to_string())))
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

/// Délai maximal accordé à une opération du Secret Service. Chaque opération ouvre une
/// connexion D-Bus et fait un échange de clés : sur une machine chargée, cela dépasse
/// 2 s, et une phrase mémorisée passerait pour absente. Aucun appel n'a lieu sur le
/// thread de l'interface (fond et askpass seulement).
pub const DELAI_TROUSSEAU: Duration = Duration::from_secs(10);

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

    /// Exécute `f` sur un fil à part et attend au plus `DELAI_TROUSSEAU`. Passé ce délai,
    /// rend [`ErreurTrousseau::Delai`] ; si le fil ne peut être lancé ou plante,
    /// [`ErreurTrousseau::Autre`]. Le fil en retard est abandonné : il finira de lui-même.
    fn avec_collection<T: Send + 'static>(
        &self,
        f: impl FnOnce(&Acces<'_>) -> T + Send + 'static,
    ) -> Result<T, ErreurTrousseau> {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("pupitre-trousseau".into())
            .spawn(move || {
                let _ = tx.send(Self::acceder(f));
            })
            .map_err(|e| ErreurTrousseau::Autre(e.to_string()))?;
        rx.recv_timeout(DELAI_TROUSSEAU).map_err(|e| match e {
            mpsc::RecvTimeoutError::Timeout => ErreurTrousseau::Delai,
            mpsc::RecvTimeoutError::Disconnected => {
                ErreurTrousseau::Autre("fil du trousseau interrompu".into())
            }
        })
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
    /// Délai dépassé : « absent » (voir la documentation du module).
    fn etat(&self) -> EtatTrousseau {
        self.avec_collection(|a| match a {
            Acces::Pret(_) => EtatTrousseau::Disponible,
            Acces::Verrouille => EtatTrousseau::Verrouille,
            Acces::Absent => EtatTrousseau::Absent,
        })
        .unwrap_or(EtatTrousseau::Absent)
    }

    fn lire(&self, cle: &Path) -> Result<Option<Zeroizing<String>>, ErreurTrousseau> {
        let cle = cle
            .to_str()
            .ok_or_else(|| ErreurTrousseau::Autre("chemin non UTF-8".into()))?
            .to_string();
        self.avec_collection(move |a| {
            let col = match a {
                Acces::Pret(col) => col,
                Acces::Verrouille => return Err(ErreurTrousseau::Verrouille),
                Acces::Absent => return Err(ErreurTrousseau::Absent),
            };
            let items = col
                .search_items(attributs(&cle))
                .map_err(|e| ErreurTrousseau::Autre(e.to_string()))?;
            let Some(item) = items.first() else {
                return Ok(None);
            };
            let secret = Zeroizing::new(
                item.get_secret()
                    .map_err(|e| ErreurTrousseau::Autre(e.to_string()))?,
            );
            // UTF-8 invalide : erreur (octets effacés), jamais « absente ».
            match String::from_utf8(secret.to_vec()) {
                Ok(s) => Ok(Some(Zeroizing::new(s))),
                Err(e) => {
                    drop(Zeroizing::new(e.into_bytes()));
                    Err(ErreurTrousseau::Autre(
                        "phrase illisible (UTF-8 invalide)".into(),
                    ))
                }
            }
        })?
    }

    fn ecrire(&self, cle: &Path, phrase: &str) -> Result<(), ErreurTrousseau> {
        let chemin = cle
            .to_str()
            .ok_or_else(|| ErreurTrousseau::Autre("chemin non UTF-8".into()))?
            .to_string();
        let libelle = format!("Pupitre — phrase de passe de {}", chemin_lisible(cle));
        let phrase = Zeroizing::new(phrase.to_string());
        self.avec_collection(move |a| match a {
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
        })?
    }

    fn oublier(&self, cle: &Path) -> Result<(), ErreurTrousseau> {
        let chemin = cle
            .to_str()
            .ok_or_else(|| ErreurTrousseau::Autre("chemin non UTF-8".into()))?
            .to_string();
        self.avec_collection(move |a| match a {
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
        })?
    }

    fn cles(&self) -> Vec<PathBuf> {
        self.avec_collection(|a| {
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
        .unwrap_or_default()
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
        assert!(c.lire(k).unwrap().is_none());
        c.ecrire(k, "mot").unwrap();
        assert_eq!(
            c.lire(k).unwrap().as_deref().map(String::as_str),
            Some("mot")
        );
        assert_eq!(c.etat(), EtatTrousseau::Disponible);
    }

    #[test]
    fn oublier_retire() {
        let c = CoffreMemoire::default();
        let k = Path::new("/a/cle");
        c.ecrire(k, "mot").unwrap();
        c.oublier(k).unwrap();
        assert!(c.lire(k).unwrap().is_none());
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
        assert_eq!(
            ErreurTrousseau::Delai.to_string(),
            "le trousseau ne répond pas"
        );
    }
}
