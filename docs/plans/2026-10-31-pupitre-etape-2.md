# Pupitre — Étape 2 : SSH sans saisie répétée — Plan d'implémentation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal :** dans les shells de Pupitre, un agent SSH est toujours joignable, les phrases de passe sont demandées dans une fenêtre accessible de Pupitre et peuvent être mémorisées dans le trousseau du système ; résultat : plus aucune saisie après la première. Une liste de connexions ouvre `ssh <hôte>` dans un nouvel onglet.

**Architecture :** une nouvelle crate `pupitre-ssh` regroupe la recherche d'agent, le trousseau (Secret Service), la lecture des invites d'OpenSSH, la lecture de `~/.ssh/config` et le binaire `pupitre-askpass`. `pupitre-fond` garantit l'agent, exporte `SSH_AUTH_SOCK`, `SSH_ASKPASS` et `SSH_ASKPASS_REQUIRE=force` aux shells, relaie les demandes de `pupitre-askpass` vers l'interface (protocole v2, rétrocompatible v1) et charge dans l'agent les clés mémorisées. L'interface affiche la fenêtre de demande dans la fenêtre qui contient l'onglet concerné, et la liste des connexions.

**Tech Stack :** Rust, `secret-service` 4 (API `blocking`, feature `rt-async-io-crypto-rust`), `zeroize` 1, `glob` 0.3, OpenSSH client 10 (`ssh`, `ssh-agent`, `ssh-add`, `ssh-keygen`) ; tests : `gnome-keyring-daemon` sous `dbus-run-session`.

**Spec :** `docs/conception/2026-10-25-pupitre-conception.md`, §10 (SSH et secrets), §11 (vie privée), §13 (robustesse), §15 (étape 2).

**Hors étape 2** : marqueur SSH dans l'invite, Starship, OSC 133 (étape 3) ; notifications, paquet `.deb` (étape 4). Pupitre ne réécrit aucun client SSH et ne touche jamais à la vérification des empreintes (OpenSSH s'en charge).

## Global Constraints

- Tout ce qui figure dans les Global Constraints de l'étape 1 (`docs/plans/2026-10-25-pupitre-etape-1.md`) reste valable : X11 et Wayland, GTK ≥ 4.18, VTE ≥ 0.80, SPDX `GPL-3.0-or-later`, pas de `unwrap`/`expect`/`panic!` hors tests, écritures par `ecrire_atomique`, textes d'interface en français, `suivi-tokens.md` mis à jour à la fin de chaque tâche.
- **L'environnement de la session de bureau n'est jamais modifié** : `SSH_AUTH_SOCK` n'est exporté qu'aux shells lancés par `pupitre-fond`.
- **Jamais de secret en clair** : ni sur disque, ni dans la ligne de commande d'un processus, ni dans un journal, ni dans un message d'erreur. Les phrases de passe ne transitent que par le socket Unix de Pupitre (dossier `0700`), par des tubes et par le trousseau ; elles sont gardées dans des `zeroize::Zeroizing<String>`.
- Trousseau verrouillé ou absent : message clair, repli sur la saisie à chaque fois. **Pupitre ne déclenche jamais lui-même la demande de déverrouillage du trousseau.**
- OpenSSH est utilisé tel quel (`ssh`, `ssh-agent`, `ssh-add` trouvés dans `PATH`) ; leur absence n'empêche jamais Pupitre de fonctionner.
- Aucun argument fourni par l'utilisateur (hôte, utilisateur, clé) ne peut devenir une option de `ssh` : refus de tout champ qui commence par `-`, `--` avant l'hôte.
- Fichier des connexions de Pupitre : `$XDG_CONFIG_HOME/pupitre/connexions.toml` (champs : nom, hôte, utilisateur, port, clé). `~/.ssh/config` est **lu seulement**.
- Interface minimale : le porteur n'utilise qu'un seul serveur.

## Review Focus

1. **Phrase de passe mémorisée devenue fausse** (clé changée de phrase) → au démarrage, le chargement échoue, l'entrée du trousseau est oubliée et un message le dit ; la prochaine connexion redemande la phrase. Aucune boucle de tentatives. Test : Task 7 `phrase_memorisee_fausse_est_oubliee`.
2. **L'askpass disparaît pendant que la fenêtre est ouverte** (Ctrl+C dans `ssh`, onglet fermé) → la fenêtre de demande se ferme seule ; aucune réponse n'est envoyée à personne. Test : Task 6 `demande_annulee_si_askpass_part`, Task 9 `fenetre_fermee_quand_demande_annulee`.
3. **Aucune interface connectée** (l'interface a planté pendant un `git pull`) → `pupitre-askpass` répond tout de suite par un échec avec un message sur stderr ; une interface qui se (re)connecte reçoit les demandes en attente. Test : Task 6 `sans_interface_refus_immediat`, `demande_renvoyee_a_une_interface_qui_arrive`.
4. **Hôte ou utilisateur commençant par `-`, ou avec espace / retour à la ligne** dans `connexions.toml` ou dans `~/.ssh/config` → entrée refusée avec la ligne fautive, jamais passée à `ssh`. Test : Task 10 `hote_option_refuse`, `alias_avec_tiret_ignore`.
5. **Agent qui meurt pendant la session** (`ssh-agent` tué, `pkill ssh-agent`) → le prochain onglet ouvert retrouve un agent (relancé si c'est le nôtre) et les clés mémorisées y sont rechargées. Test : Task 5 `agent_relance_apres_mort`.

---

## Structure des fichiers

```
crates/ssh/Cargo.toml                     nouvelle crate pupitre-ssh (lib + bin pupitre-askpass)
crates/ssh/src/lib.rs                     réexports
crates/ssh/src/agent.rs                   agent_joignable, chercher_agent, AgentPupitre
crates/ssh/src/trousseau.rs               trait Coffre, CoffreSecretService, CoffreMemoire
crates/ssh/src/invite.rs                  classer_invite (textes d'OpenSSH → Genre)
crates/ssh/src/hotes.rs                   hotes_ssh_config (~/.ssh/config, Include)
crates/ssh/src/bin/pupitre-askpass.rs     programme SSH_ASKPASS
crates/ssh/tests/trousseau_reel.rs        Coffre contre gnome-keyring (ignoré hors scripts)
crates/proto/src/lib.rs                   VERSION 2, messages de demande (ajoutés en fin d'enum)
crates/proto/src/ssh.rs                   types partagés Genre, EtatTrousseau
crates/fond/src/ssh.rs                    EtatSsh : agent, variables des shells, chargement des clés
crates/fond/src/serveur.rs                relais des demandes, variables passées à Session::open
crates/config/src/connexions.rs           Connexion, charger_connexions, enregistrer_connexions
crates/pupitre/src/client.rs              repli en v1 face à un processus de fond ancien
crates/pupitre/src/demande.rs             fenêtre de demande (phrase de passe, hôte inconnu…)
crates/pupitre/src/connexions.rs          fenêtre « Connexions SSH » et formulaire
crates/pupitre/src/fenetre.rs             routage des demandes, menu, raccourci, titre provisoire
crates/pupitre/src/raccourcis.rs          Raccourci::Connexions (Ctrl+Maj+O)
crates/pupitre/tests/gui_ssh.rs           tests graphiques SSH
scripts/trousseau-test.sh                 lance gnome-keyring-daemon déverrouillé (HOME temporaire)
tests/a11y/orca/scenario.py               vérifications 15 et 16
tests/isolement/verifier.py               pupitre-askpass jugé, session SSH ajoutée
docs/orca-liste-de-controle.md, docs/verifications/etape-2.md
```

---

### Task 0 : Levée de risque — comportement réel d'OpenSSH 10 et du trousseau (jetable)

But : figer les textes d'invite et les comportements dont dépend tout le plan. **Point de décision** : si un critère échoue, arrêter et revenir vers le porteur.

**Files :** Create: `essai-jetable/ssh/essai.sh`, `essai-jetable/ssh/RESULTATS.md` (hors espace de travail).

- [ ] **Step 1 : Askpass espion.** Un script `espion.sh` qui ajoute `argv[1]`, `SSH_ASKPASS_PROMPT` et son PID parent à un fichier, puis répond une valeur lue dans un fichier. Clé de test : `ssh-keygen -t ed25519 -N secret1 -f $T/cle`.
- [ ] **Step 2 : Relever, avec `SSH_ASKPASS=espion.sh SSH_ASKPASS_REQUIRE=force`, sans `DISPLAY` ni `WAYLAND_DISPLAY`, sans terminal (`setsid … </dev/null`) :**
  - `ssh-add $T/cle` : texte exact de la 1re invite ; avec une mauvaise réponse, texte de la 2e (« Bad passphrase… ») ; réponse vide → `ssh-add` s'arrête (code ≠ 0).
  - `ssh-add -c $T/cle` puis usage de la clé : `SSH_ASKPASS_PROMPT=confirm`.
  - `ssh -i $T/cle -o UserKnownHostsFile=$T/kh nimporte@127.0.0.1 -p 1` (port fermé) puis `ssh -i $T/cle -p 22 localhost` si un sshd répond : invite de phrase de clé (`Enter passphrase for key '…': `) ; invite d'hôte inconnu si atteignable.
  - `git ls-remote https://exemple.invalide/x` avec `GIT_TERMINAL_PROMPT=0` retiré : git appelle-t-il `SSH_ASKPASS` (invite « Username for … ») ?
  - un descripteur ouvert non `CLOEXEC` dans l'appelant arrive-t-il jusqu'à l'askpass lancé par `ssh-add` ? (information seulement)
- [ ] **Step 3 : Trousseau.** Mini-programme Rust : `secret-service` 4 en `blocking`, sous `dbus-run-session` avec `gnome-keyring-daemon --unlock --components=secrets` (mot de passe sur stdin, `HOME` temporaire). Vérifier : écriture, recherche par attributs, suppression, `is_locked()` sur la collection par défaut **sans** déclencher d'invite ; sans service sur le bus : erreur rendue en moins de 2 s (pas d'attente infinie).
- [ ] **Step 4 : Consigner** les textes exacts et les résultats dans `RESULTATS.md`. **Critères** : `force` utilise l'askpass sans écran ; les invites de phrase contiennent le chemin de la clé ; `is_locked()` ne déclenche aucune invite. Recopier les textes exacts dans les tests de la Task 3.

---

### Task 1 : Crate `pupitre-ssh` et recherche d'agent

**Files :**
- Create: `crates/ssh/Cargo.toml`, `crates/ssh/src/lib.rs`, `crates/ssh/src/agent.rs`
- Modify: `Cargo.toml` (membre `crates/ssh`)

**Interfaces :**
- Produces :
  - `pub fn agent_joignable(socket: &Path, delai: Duration) -> bool` — connexion, envoi de `SSH_AGENTC_REQUEST_IDENTITIES` (trame `00 00 00 01 0B`), vrai si la réponse est de type `SSH_AGENT_IDENTITIES_ANSWER` (12). Lecture et écriture limitées par `delai`.
  - `pub fn candidats_agent(runtime: &Path, tmp: &Path) -> Vec<PathBuf>` — dans l'ordre : `runtime/openssh_agent`, `runtime/ssh-agent.socket`, `runtime/gcr/ssh`, `runtime/keyring/ssh`, `runtime/gnupg/S.gpg-agent.ssh`, puis `tmp/ssh-*/agent.*` ; seulement des sockets appartenant à l'utilisateur courant (`MetadataExt::uid` = `getuid`, `FileTypeExt::is_socket`).
  - `pub fn chercher_agent(env_sock: Option<&Path>, runtime: &Path, tmp: &Path, delai: Duration) -> Option<PathBuf>` — `env_sock` s'il est joignable, sinon le premier candidat joignable.
  - `pub struct AgentPupitre` ; `pub fn lancer(socket: &Path) -> io::Result<AgentPupitre>` (supprime un socket périmé, lance `ssh-agent -D -a <socket>`, attend au plus 2 s qu'il soit joignable) ; `pub fn socket(&self) -> &Path` ; `pub fn vivant(&mut self) -> bool` ; `Drop` tue et récupère l'enfant.
  - Constante `pub const DELAI_SONDE: Duration = Duration::from_millis(300);`

- [ ] **Step 1 : Tests** dans `agent.rs` (`#[cfg(test)]`), dossier temporaire :
  - `faux_agent_reconnu` : un `UnixListener` qui répond `00 00 00 05 0C 00 00 00 00` → `agent_joignable` vrai.
  - `socket_muet_refuse` : listener qui accepte et ne répond pas → faux en moins de `2 * DELAI_SONDE`.
  - `socket_mort_refuse` : fichier socket dont le listener est fermé → faux.
  - `fichier_ordinaire_ignore` : `runtime/openssh_agent` fichier ordinaire → absent de `candidats_agent`.
  - `ordre_des_candidats` : deux faux agents (`gcr/ssh`, `tmp/ssh-x/agent.1`) → `gcr/ssh` d'abord.
  - `env_prioritaire` / `env_mort_puis_candidat` : `chercher_agent` choisit l'agent de l'environnement s'il répond, sinon un candidat.
  - `vrai_ssh_agent` (sauté avec un message si `ssh-agent` absent du `PATH`) : `AgentPupitre::lancer` → `agent_joignable` vrai ; après `drop`, le socket ne répond plus.
- [ ] **Step 2 :** `cargo test -p pupitre-ssh agent` → échec de compilation (fonctions absentes).
- [ ] **Step 3 : Implémenter** dans `agent.rs` (`nix` pour `getuid`, `glob` pour `ssh-*/agent.*`).
- [ ] **Step 4 :** `cargo test -p pupitre-ssh agent` → tout passe ; `cargo clippy -p pupitre-ssh` → 0 avertissement.
- [ ] **Step 5 : Commit** `feat(ssh): recherche et lancement de l'agent SSH`.

---

### Task 2 : Trousseau (Secret Service)

**Files :**
- Create: `crates/ssh/src/trousseau.rs`, `crates/ssh/tests/trousseau_reel.rs`, `scripts/trousseau-test.sh`

**Interfaces :**
- Produces :
  - `pub enum EtatTrousseau { Disponible, Verrouille, Absent }` (dérive `Serialize, Deserialize, Clone, Copy, Debug, PartialEq`), **défini dans `crates/proto/src/ssh.rs`** car il voyage dans le protocole (Task 4) et réexporté par `pupitre-ssh` (qui dépend de `pupitre-proto`).
  - `pub trait Coffre { fn etat(&self) -> EtatTrousseau; fn lire(&self, cle: &Path) -> Option<Zeroizing<String>>; fn ecrire(&self, cle: &Path, phrase: &str) -> Result<(), ErreurTrousseau>; fn oublier(&self, cle: &Path) -> Result<(), ErreurTrousseau>; fn cles(&self) -> Vec<PathBuf>; }`
  - `pub struct CoffreSecretService` (`pub fn ouvrir() -> Self`, jamais d'échec : le service est cherché à chaque appel) et `pub struct CoffreMemoire` (pour les tests des autres tâches, `Default`).
  - `pub enum ErreurTrousseau { Verrouille, Absent, Autre(String) }` avec `Display` en français.
  - Attributs de chaque élément : `application = "pupitre"`, `type = "phrase-cle-ssh"`, `cle = <chemin absolu>`. Libellé : `Pupitre — phrase de passe de <chemin avec ~>`. Collection par défaut.
  - `pub fn chemin_lisible(p: &Path) -> String` (remplace le dossier personnel par `~`), utilisé aussi par l'interface.
- Règle : `etat()`, `lire()` et `cles()` ne déverrouillent jamais (pas d'appel `unlock`) ; collection verrouillée → `Verrouille` / `None` / vide. Toute erreur D-Bus → `Absent`, jamais de panique. Délai D-Bus borné (Task 0).

- [ ] **Step 1 : Tests unitaires** de `CoffreMemoire` et `chemin_lisible` (`ecrire_puis_lire`, `oublier_retire`, `cles_liste_les_chemins`, `tilde_pour_le_dossier_personnel`).
- [ ] **Step 2 : Test réel** `crates/ssh/tests/trousseau_reel.rs` (`#[ignore]`, lancé par les scripts) : `etat() == Disponible` ; `ecrire` → `lire` rend la phrase ; `cles()` la contient ; `oublier` → `lire` `None` ; un élément d'une autre application avec le même attribut `cle` n'est ni lu ni effacé. Test `sans_service_absent` : `DBUS_SESSION_BUS_ADDRESS` vers un socket inexistant → `Absent` en moins de 2 s.
- [ ] **Step 3 : `scripts/trousseau-test.sh`** : utilisé comme préfixe dans un bus déjà privé ; crée un `HOME` temporaire, lance `echo -n test | gnome-keyring-daemon --unlock --components=secrets --daemonize`, exporte les variables qu'il imprime, exécute `"$@"`, tue le démon à la sortie. Brancher dans `scripts/test-x11.sh` et `scripts/test-wayland.sh` : `dbus-run-session -- scripts/trousseau-test.sh cargo test …`.
- [ ] **Step 4 :** échec des tests, implémentation, puis `scripts/test-x11.sh -p pupitre-ssh` → tout passe.
- [ ] **Step 5 : Commit** `feat(ssh): phrases de passe dans le trousseau du système`.

---

### Task 3 : Lecture des invites d'OpenSSH

**Files :** Create: `crates/ssh/src/invite.rs`

**Interfaces :**
- Produces :
  - `pub enum Genre { PhraseCle { cle: PathBuf, nouvel_essai: bool }, HoteInconnu, Confirmation, Information, Texte { masque: bool } }` (dérive `Serialize, Deserialize, Clone, Debug, PartialEq`), défini lui aussi dans `crates/proto/src/ssh.rs` et réexporté par `pupitre-ssh`.
  - `pub fn classer_invite(invite: &str, prompt_env: Option<&str>) -> Genre` :
    - `prompt_env == Some("confirm")` → `Confirmation` ; `Some("none")` → `Information` ;
    - « Enter passphrase for key '<chemin>': » et « Enter passphrase for <chemin>: » (avec suffixe éventuel « (will confirm each use) ») → `PhraseCle { nouvel_essai: false }` ;
    - « Bad passphrase, try again for <chemin>: » → `PhraseCle { nouvel_essai: true }` ;
    - contient « (yes/no » → `HoteInconnu` ;
    - sinon `Texte { masque }`, `masque` vrai sauf si l'invite (minuscules) contient « username ».
  - Chemin relatif ou commençant par `~` → rendu absolu (dossier personnel).
- [ ] **Step 1 : Tests** avec les textes exacts relevés à la Task 0 : `phrase_ssh`, `phrase_ssh_add`, `phrase_avec_confirmation`, `phrase_incorrecte`, `hote_inconnu_multiligne`, `mot_de_passe_masque`, `nom_utilisateur_git_visible`, `confirm_et_none`, `chemin_avec_espace_et_apostrophe` (`/home/u/mes clés/l'id` : la dernière `'` avant `': ` ferme le chemin), `invite_vide_texte_masque`.
- [ ] **Step 2–4 :** échec, implémentation, succès (`cargo test -p pupitre-ssh invite`).
- [ ] **Step 5 : Commit** `feat(ssh): reconnaissance des invites d'OpenSSH`.

---

### Task 4 : Protocole v2 (rétrocompatible)

**Files :**
- Modify: `crates/proto/src/lib.rs` (utilise `Genre` et `EtatTrousseau` de `crates/proto/src/ssh.rs`, Tasks 2 et 3), `crates/fond/src/serveur.rs` (accueil), `crates/pupitre/src/client.rs` (repli), `crates/fond/fuzz/` (nouvelles variantes dans la cible)

**Interfaces :**
- Produces (variantes **ajoutées à la fin** des enums, pour que l'encodage postcard des variantes v1 ne change pas) :
  - `pub const VERSION: u32 = 2; pub const VERSION_MIN: u32 = 1;`
  - `pub type DemandeId = u64;`
  - `ClientMsg::Demander { session: Option<SessionId>, invite: String, genre: Genre, trousseau: EtatTrousseau }` (envoyé par `pupitre-askpass`)
  - `ClientMsg::Repondre { demande: DemandeId, reponse: Option<String>, memoriser: bool }` (interface ; `None` = refus/annulation ; pour `Confirmation` et `HoteInconnu`, `Some("yes")`/`Some("no")`)
  - `ClientMsg::ChargerCle { cle: PathBuf }`
  - `ServerMsg::Demande { demande: DemandeId, session: Option<SessionId>, invite: String, genre: Genre, trousseau: EtatTrousseau }`
  - `ServerMsg::DemandeClose { demande: DemandeId }`
  - `ServerMsg::Reponse { reponse: Option<String>, memoriser: bool }` (vers l'askpass)
  - `ServerMsg::Message(String)` (texte à montrer dans le bandeau de toutes les fenêtres)
- Accueil : le processus de fond accepte `VERSION_MIN..=VERSION` et répond `Welcome { version: min(client, VERSION) }` ; il n'envoie jamais de variante v2 à un client v1.
- Client : `Hello { version: 2 }` ; si la réponse est l'erreur « version du protocole incompatible », reconnexion avec `Hello { version: 1 }` ; `Pupitre` retient la version obtenue et affiche le message persistant « Le processus de fond est d'une version plus ancienne : les fonctions SSH de Pupitre seront actives quand tous les onglets auront été fermés. »
- [ ] **Step 1 : Tests** : `proto` `encodage_v1_inchange` (octets attendus de `ClientMsg::Open`, `ServerMsg::Output` figés en dur depuis le code actuel **avant** la modification) ; `nouvelles_variantes_aller_retour` ; `fond/tests/serveur.rs` `client_v1_accueilli_sans_message_v2` ; `pupitre/tests/client.rs` `repli_v1_face_a_un_fond_ancien` (faux serveur qui refuse 2 et accepte 1).
- [ ] **Step 2–4 :** échec, implémentation, succès (`cargo test -p pupitre-proto -p pupitre-fond -p pupitre --test client`). Lancer 60 s la cible de fuzzing des trames : aucun plantage.
- [ ] **Step 5 : Commit** `feat(proto): version 2, demandes de l'askpass, compatible v1`.

---

### Task 5 : Agent garanti et variables des shells (processus de fond)

**Files :**
- Create: `crates/fond/src/ssh.rs`
- Modify: `crates/fond/src/serveur.rs` (`ouvrir` passe les variables à `Session::open`), `crates/fond/Cargo.toml`, `crates/ssh/Cargo.toml` (bin `pupitre-askpass` déclaré, corps minimal jusqu'à la Task 7)

**Interfaces :**
- Consumes : `chercher_agent`, `AgentPupitre`, `DELAI_SONDE` (Task 1).
- Produces :
  - `pub struct EtatSsh` ; `pub fn new(socket_fond: &Path, askpass: Option<PathBuf>) -> Self` ; `pub fn variables(&mut self, session: SessionId) -> Vec<(String, String)>`.
  - `variables` : `SSH_AUTH_SOCK` (agent trouvé ou lancé) ; si l'askpass existe : `SSH_ASKPASS`, `SSH_ASKPASS_REQUIRE=force` ; toujours `PUPITRE_SOCKET=<socket du fond>`, `PUPITRE_SESSION=<id>`. Aucun agent possible → pas de `SSH_AUTH_SOCK` ajouté (celui hérité reste) et un `ServerMsg::Message` unique : « Agent SSH indisponible : les phrases de passe seront demandées à chaque connexion. »
  - Recherche au démarrage ; ensuite, au plus une nouvelle sonde toutes les 10 s, faite dans `variables` (durée bornée par `DELAI_SONDE`). Notre agent : socket `<dossier du socket du fond>/agent.sock`, relancé s'il est mort.
  - `pub fn chemin_askpass() -> Option<PathBuf>` : variable `PUPITRE_ASKPASS`, sinon `pupitre-askpass` à côté de l'exécutable courant (même logique que `binaire_fond`), seulement s'il existe.
  - `pub fn agent_change(&mut self) -> bool` : vrai une fois après chaque nouvel agent (déclenche le chargement des clés, Task 7).
- [ ] **Step 1 : Tests** (`fond/tests/serveur.rs`, avec `XDG_RUNTIME_DIR` et `TMPDIR` temporaires, `SSH_AUTH_SOCK` retiré) : `shell_recoit_ssh_auth_sock` (`echo $SSH_AUTH_SOCK` dans la session → chemin joignable) ; `agent_de_l_environnement_reutilise` (faux agent dans `SSH_AUTH_SOCK`) ; `agent_existant_retrouve` (faux agent dans `runtime/openssh_agent`, `SSH_AUTH_SOCK` absent) ; `agent_relance_apres_mort` (tuer notre `ssh-agent`, attendre 10 s, ouvrir une session → nouvel agent joignable) ; `variables_askpass_presentes` (`PUPITRE_ASKPASS=/bin/true`) ; `sans_askpass_pas_de_variable` ; `sans_ssh_agent_message` (`PATH` sans `ssh-agent`, aucun agent → `ServerMsg::Message`, la session s'ouvre quand même).
- [ ] **Step 2–4 :** échec, implémentation, succès (`cargo test -p pupitre-fond`).
- [ ] **Step 5 : Commit** `feat(fond): agent SSH garanti dans les shells de Pupitre`.

---

### Task 6 : Relais des demandes (processus de fond)

**Files :** Modify: `crates/fond/src/serveur.rs`

**Interfaces :**
- Consumes : messages v2 (Task 4).
- Comportement :
  - Un client qui envoie `Demander` est un **demandeur** (askpass) ; tous les autres clients v2 accueillis sont des **interfaces**.
  - `Demander` → numéro neuf ; `ServerMsg::Demande` envoyé à toutes les interfaces ; aucune interface → `Reponse { reponse: None, memoriser: false }` tout de suite.
  - Premier `Repondre` reçu pour une demande ouverte → `Reponse` au demandeur, `DemandeClose` aux autres interfaces ; réponses suivantes ignorées.
  - Demandeur déconnecté → `DemandeClose` à toutes les interfaces.
  - Interface accueillie (ou qui envoie `List`) → reçoit toutes les demandes ouvertes.
  - Un demandeur ne compte pas comme client pour l'arrêt automatique du fond ; une demande ouverte non plus.
  - Le contenu de `Repondre` n'est jamais écrit dans un journal ni dans un `Error`.
- [ ] **Step 1 : Tests** (`fond/tests/serveur.rs`, clients bruts sur le socket) : `demande_relayee_puis_reponse` ; `premiere_reponse_gagne` (deux interfaces) ; `demande_annulee_si_askpass_part` ; `sans_interface_refus_immediat` ; `demande_renvoyee_a_une_interface_qui_arrive` ; `client_v1_ne_recoit_pas_de_demande`.
- [ ] **Step 2–4 :** échec, implémentation, succès.
- [ ] **Step 5 : Commit** `feat(fond): relais des demandes de phrase de passe`.

---

### Task 7 : `pupitre-askpass` et chargement des clés mémorisées

**Files :**
- Modify: `crates/ssh/src/bin/pupitre-askpass.rs`, `crates/fond/src/ssh.rs`, `crates/fond/src/serveur.rs`

**Interfaces :**
- Consumes : `classer_invite`, `Coffre`, `CoffreSecretService`, messages v2.
- `pupitre-askpass` (règle : **stdout ne reçoit que la réponse suivie de `\n`** ; tout message va sur stderr, préfixé `pupitre-askpass : `) :
  - **Mode normal** (appelé par `ssh`, `ssh-add`, git) : lit `argv[1]` et `SSH_ASKPASS_PROMPT` ; `classer_invite` ; se connecte à `PUPITRE_SOCKET` (absent ou injoignable → code 1, message) ; `Hello{2}` puis `Demander { session: PUPITRE_SESSION, trousseau: coffre.etat() (seulement pour PhraseCle, sinon Absent) }` ; attend `Reponse`.
    - `reponse: None` → code 1, rien sur stdout.
    - `Confirmation` : `Some("yes")` → code 0, rien sur stdout ; sinon code 1.
    - `Information` : attend sans fin (`ssh` le tue quand c'est fini).
    - `PhraseCle` avec `memoriser` et trousseau `Disponible` → `coffre.ecrire(cle, phrase)` (échec : message sur stderr, on continue), écrit la phrase sur stdout, envoie `ChargerCle { cle }`, ferme, code 0.
    - Autres → écrit la réponse, code 0.
  - **Mode trousseau** (`PUPITRE_ASKPASS_MODE=trousseau`, utilisé seulement par le chargement ci-dessous) : `PhraseCle { nouvel_essai: false }` → phrase lue dans le trousseau, ou code 1 ; `PhraseCle { nouvel_essai: true }` → `coffre.oublier(cle)`, code 1 ; tout autre genre → code 1. Jamais de connexion au fond, jamais d'interface.
- Chargement (processus de fond, **dans un thread à part**, jamais dans le thread central) : `pub fn charger_cle(agent: &Path, askpass: &Path, cle: &Path) -> Result<(), String>` lance `ssh-add -- <cle>` (stdin fermé, `SSH_AUTH_SOCK`, `SSH_ASKPASS`, `SSH_ASKPASS_REQUIRE=force`, `PUPITRE_ASKPASS_MODE=trousseau`), tué après 20 s. Échec → `ServerMsg::Message` : « La phrase de passe mémorisée pour <chemin lisible> ne fonctionne plus ; elle a été oubliée. Elle sera redemandée. »
  - Sur `ChargerCle` ; et quand `agent_change()` (démarrage, agent relancé) : pour chaque `coffre.cles()` dont le fichier existe et dont la clé publique (`<cle>.pub`, deux premiers champs) n'est pas dans `ssh-add -L`.
  - Trousseau `Verrouille` à ce moment → message unique « Trousseau verrouillé : les clés mémorisées n'ont pas été chargées. » et nouvel essai à chaque ouverture d'onglet tant que des clés n'ont pas été chargées.
- [ ] **Step 1 : Tests** (`crates/ssh` pour le binaire, avec `CoffreMemoire` injecté via une fonction `executer(args, env, coffre, sortie) -> i32` testable ; `fond/tests/serveur.rs` pour le chargement, sous `scripts/trousseau-test.sh`) : `sans_socket_code_1` ; `refus_rien_sur_stdout` ; `phrase_ecrite_et_memorisee` ; `confirmation_codes` ; `mode_trousseau_rend_la_phrase` ; `mode_trousseau_nouvel_essai_oublie` ; `cle_memorisee_chargee_au_demarrage` (clé de test chiffrée par `ssh-keygen -N`, phrase dans le trousseau, démarrage du fond → `ssh-add -l` la liste) ; `phrase_memorisee_fausse_est_oubliee` (mauvaise phrase dans le trousseau → message, entrée oubliée, `ssh-add` terminé en moins de 20 s) ; `cle_deja_dans_l_agent_pas_rechargee`.
- [ ] **Step 2–4 :** échec, implémentation, succès (`scripts/test-x11.sh -p pupitre-ssh -p pupitre-fond`).
- [ ] **Step 5 : Commit** `feat(ssh): askpass de Pupitre et chargement des clés mémorisées`.

---

### Task 8 : Fichier des connexions et hôtes de `~/.ssh/config`

**Files :**
- Create: `crates/config/src/connexions.rs`, `crates/ssh/src/hotes.rs`
- Modify: `crates/config/src/lib.rs`

**Interfaces :**
- Produces :
  - `pub struct Connexion { pub nom: String, pub hote: String, pub utilisateur: Option<String>, pub port: Option<u16>, pub cle: Option<PathBuf> }` (serde, `deny_unknown_fields`) ; fichier : tableau `[[connexion]]`.
  - `pub fn chemin_connexions() -> PathBuf` ; `pub fn charger_connexions(chemin: &Path) -> Result<Vec<Connexion>, ErreurConfig>` (absent → vide ; invalide → ligne fautive, comme `charger`) ; `pub fn enregistrer_connexions(chemin: &Path, v: &[Connexion]) -> io::Result<()>` (par `ecrire_atomique`).
  - Validation : `nom` non vide ; `hote` et `utilisateur` non vides, sans blanc ni caractère de contrôle, ne commençant pas par `-` ; `port` 1–65535 ; noms uniques.
  - `pub fn argv_ssh(c: &Connexion) -> Vec<String>` → `["ssh", "-p", port, "-l", utilisateur, "-i", cle, "--", hote]` (options présentes seulement si renseignées).
  - `pub fn hotes_ssh_config(fichier: &Path) -> Vec<String>` (dans `pupitre-ssh`) : alias des lignes `Host` (mot-clé insensible à la casse, `Host=x` accepté), sans motifs (`*`, `?`, `!`), sans alias commençant par `-`, sans doublon, dans l'ordre ; suit `Include` (chemins relatifs à `~/.ssh`, motifs `glob`, profondeur ≤ 8, boucles ignorées) ; fichier absent ou illisible → vide. Pour un alias : argv `["ssh", "--", alias]`.
- [ ] **Step 1 : Tests** : `connexions_aller_retour` ; `fichier_absent_vide` ; `hote_option_refuse` (`hote = "-oProxyCommand=x"` → erreur ligne 3) ; `port_hors_bornes_refuse` ; `noms_en_double_refuses` ; `argv_complet`, `argv_minimal` ; `hotes_simples` (`Host a b` → a, b) ; `motifs_ignores` ; `alias_avec_tiret_ignore` ; `include_relatif_et_glob` ; `include_en_boucle_termine` ; `host_egal`.
- [ ] **Step 2–4 :** échec, implémentation, succès.
- [ ] **Step 5 : Commit** `feat(config): connexions SSH mémorisées et lecture de ~/.ssh/config`.

---

### Task 9 : Fenêtre de demande (interface)

**Files :**
- Create: `crates/pupitre/src/demande.rs`, `crates/pupitre/tests/gui_ssh.rs` (+ entrée `[[test]] harness = false` dans `Cargo.toml`)
- Modify: `crates/pupitre/src/fenetre.rs` (traitement de `Demande`, `DemandeClose`, `Message`)

**Interfaces :**
- Consumes : `ServerMsg::Demande/DemandeClose/Message`, `ClientMsg::Repondre`, `chemin_lisible`.
- Produces : `pub fn ouvrir_demande(parent: &gtk::Window, d: &DemandeAffichee, repondre: impl Fn(Option<String>, bool) + 'static) -> gtk::Window` ; `Pupitre` garde `demandes: RefCell<HashMap<DemandeId, gtk::Window>>`.
- Fenêtre parente : celle qui contient l'onglet de `session` ; sinon la fenêtre active ; aucune fenêtre → une nouvelle. Modale, `transient_for`, rôle `Dialog`, annoncée par `messages::annoncer_focus_une_fois_affiche` (focus sur la saisie ou sur le bouton par défaut).
- Contenu par genre (textes exacts) :
  - `PhraseCle` : titre « Phrase de passe SSH » ; texte « Phrase de passe de la clé <chemin lisible> : » (précédé de « Phrase incorrecte. » si `nouvel_essai`) ; `gtk::PasswordEntry` avec `show-peek-icon` ; case « Mémoriser dans le trousseau » cochée par défaut si `Disponible` ; si `Verrouille` : case inactive + « Trousseau verrouillé : la phrase ne peut pas être mémorisée. » ; si `Absent` : case inactive + « Aucun trousseau disponible : la phrase sera demandée à chaque fois. » ; boutons « Annuler » / « Valider » (par défaut, Entrée).
  - `HoteInconnu` : titre « Serveur inconnu » ; texte d'OpenSSH tel quel (étiquette sélectionnable) ; boutons « Refuser » (focus initial) / « Faire confiance et se connecter » → `"no"` / `"yes"`.
  - `Confirmation` : titre « Confirmer l'usage de la clé » ; texte d'OpenSSH ; « Refuser » / « Autoriser » → `"no"` / `"yes"`.
  - `Texte { masque }` : titre « Question de SSH » ; texte d'OpenSSH ; saisie masquée ou non ; « Annuler » / « Valider ».
  - `Information` : pas de fenêtre ; message dans le bandeau de la fenêtre parente (texte d'OpenSSH), retiré à `DemandeClose`.
- Échap, « Annuler », fermeture de la fenêtre → `Repondre { reponse: None }`. `DemandeClose` → fenêtre fermée sans réponse. Le texte saisi est effacé de la saisie dès l'envoi.
- `ServerMsg::Message(t)` → `afficher_message(t)` dans chaque fenêtre.
- [ ] **Step 1 : Tests graphiques** (`gui_ssh.rs`, sous `scripts/trousseau-test.sh`, clé de test chiffrée, `SSH_AUTH_SOCK` retiré, binaires `pupitre-fond` et `pupitre-askpass` du dossier `target`) :
  - `ssh_add_ouvre_la_fenetre` : `ssh-add <cle>` tapé dans l'onglet → fenêtre « Phrase de passe SSH » dont le texte contient le chemin, focus dans la saisie.
  - `phrase_validee_cle_dans_l_agent` : saisie + Entrée → `ssh-add -l` dans l'onglet affiche l'empreinte ; trousseau : la phrase y est.
  - `phrase_non_memorisee` : case décochée → clé dans l'agent, rien dans le trousseau.
  - `annuler_ne_repond_rien` : Échap → `ssh-add` termine avec un code ≠ 0.
  - `fenetre_fermee_quand_demande_annulee` : Ctrl+C dans l'onglet pendant que la fenêtre est ouverte → fenêtre fermée en moins de 2 s.
  - `trousseau_absent_case_inactive` (sans `scripts/trousseau-test.sh`, variable de test qui coupe le bus) → case inactive et texte « Aucun trousseau disponible… ».
  - `fenetre_dans_la_bonne_fenetre` : deux fenêtres, demande depuis l'onglet de la seconde → parent = seconde.
- [ ] **Step 2 : Sonde AT-SPI** : ajouter à `tests/a11y/` une règle pour la fenêtre de demande : rôle dialogue nommé « Phrase de passe SSH », saisie mot de passe nommée par le texte de la question, case à cocher nommée. Lancer la sonde depuis `ssh_add_ouvre_la_fenetre`.
- [ ] **Step 3–4 :** échec, implémentation, succès (`scripts/test-x11.sh -p pupitre --test gui_ssh`, puis `scripts/test-wayland.sh` pareil).
- [ ] **Step 5 : Commit** `feat(pupitre): fenêtre de phrase de passe SSH`.

---

### Task 10 : Fenêtre « Connexions SSH »

**Files :**
- Create: `crates/pupitre/src/connexions.rs`
- Modify: `crates/pupitre/src/fenetre.rs` (menu, action `win.connexions`, `Attente::NouvelOnglet` avec titre provisoire), `crates/pupitre/src/raccourcis.rs`, `crates/pupitre/tests/gui_ssh.rs`

**Interfaces :**
- Consumes : `charger_connexions`, `enregistrer_connexions`, `argv_ssh`, `hotes_ssh_config` (Task 8).
- Produces : `Raccourci::Connexions` (Ctrl+Maj+O) ; entrée de menu « Connexions SSH… » (première section, après « Nouvelle fenêtre ») ; `FenetrePupitre::ouvrir_connexion(&self, argv: Vec<String>, titre: String)`.
- Fenêtre « Connexions SSH » (modale, rôle `Dialog`) :
  - Une `gtk::ListBox` : d'abord les connexions de Pupitre (« nom — utilisateur@hôte:port »), puis les hôtes de `~/.ssh/config` (« alias (~/.ssh/config) »). Focus sur la première ligne. Liste vide → texte « Aucune connexion. Ajoutez-en une, ou décrivez vos serveurs dans ~/.ssh/config. » et focus sur « Ajouter… ».
  - Entrée ou double-clic sur une ligne, ou « Se connecter » → nouvel onglet dans la fenêtre d'origine, `Open { argv }`, titre provisoire = nom ou alias (affiché tant que le programme n'a pas donné de titre), fenêtre des connexions fermée.
  - « Ajouter… », « Modifier… », « Supprimer » (les deux derniers inactifs sur une ligne de `~/.ssh/config`). « Supprimer » demande confirmation (« Supprimer la connexion « nom » ? »).
  - Formulaire : « Nom », « Hôte », « Utilisateur (facultatif) », « Port (facultatif) », « Clé » (`gtk::DropDown` : « Par défaut » + clés privées de `~/.ssh` ayant un `.pub`). Erreur de validation → texte sous le champ fautif, annoncé (`messages::annoncer`), focus sur ce champ.
  - Enregistrement par `enregistrer_connexions` ; échec d'écriture → message, rien de perdu dans le formulaire. `connexions.toml` invalide → message persistant avec la ligne, la liste montre seulement `~/.ssh/config`, et le fichier n'est **jamais** réécrit tant qu'il est invalide (boutons Ajouter/Modifier/Supprimer inactifs).
- [ ] **Step 1 : Tests graphiques** : `raccourci_ouvre_les_connexions` ; `hotes_ssh_config_listes` (HOME temporaire avec `.ssh/config`) ; `ajouter_puis_relire` (formulaire rempli → `connexions.toml` contient la connexion) ; `hote_invalide_signale` (« -x » → message sous « Hôte », rien d'écrit) ; `se_connecter_ouvre_un_onglet` (connexion vers `127.0.0.1` port 1 → nouvel onglet titré du nom, la sortie de `ssh` y apparaît : « Connection refused ») ; `fichier_invalide_pas_reecrit` ; `menu_contient_connexions`.
- [ ] **Step 2–4 :** échec, implémentation, succès (X11 puis Wayland).
- [ ] **Step 5 : Commit** `feat(pupitre): liste des connexions SSH`.

---

### Task 11 : Orca, isolement et documentation

**Files :** Modify: `tests/a11y/orca/scenario.py`, `docs/orca-liste-de-controle.md`, `tests/isolement/verifier.py`, `scripts/test-orca.sh`, `scripts/verifier-isolement.sh` ; Create: `docs/verifications/etape-2.md`

- [ ] **Step 1 : Orca, vérification 15** (« Phrase de passe ») : dans l'onglet, `ssh-add <clé de test>` → Orca annonce « Phrase de passe SSH, boîte de dialogue », la question avec le chemin, « saisie de mot de passe » ; Tab → « Mémoriser dans le trousseau, case à cocher, cochée » ; Échap → retour au terminal (invite lue). Le scénario lance son propre `gnome-keyring-daemon` (`scripts/trousseau-test.sh`).
- [ ] **Step 2 : Orca, vérification 16** (« Connexions ») : Ctrl+Maj+O → « Connexions SSH, boîte de dialogue » puis la première ligne ; flèche bas → ligne suivante lue ; Échap → terminal.
- [ ] **Step 3 :** `scripts/test-orca.sh` → 16/16, trois passages de suite. Mettre à jour le tableau de `docs/orca-liste-de-controle.md`.
- [ ] **Step 4 : Isolement** : ajouter `pupitre-askpass` aux programmes jugés ; ajouter à la session type un `ssh-add` de la clé de test avec mémorisation, puis `ssh -p 1 127.0.0.1` (connexion refusée, lancée par l'utilisateur dans le shell : `ssh` n'est pas jugé). Attendu : aucune connexion réseau de Pupitre, rien d'écrit hors des dossiers de Pupitre, **la phrase de test n'apparaît dans aucun fichier** sous `HOME` ni `XDG_RUNTIME_DIR` (recherche de la chaîne après la session) ni dans les arguments d'aucun `execve` de la trace.
- [ ] **Step 5 : Commit** `test(a11y): vérifications Orca et isolement pour SSH`.

---

### Task 12 : Vérification de fin d'étape

- [ ] **Step 1 :** `cargo clippy --workspace --all-targets` → 0 avertissement ; `cargo fmt --check`.
- [ ] **Step 2 :** `scripts/test-x11.sh` et `scripts/test-wayland.sh` → tout passe.
- [ ] **Step 3 :** fuzzing des trames 5 min (cible étendue à la Task 4) → aucun plantage.
- [ ] **Step 4 :** `scripts/verifier-isolement.sh` → conforme ; `scripts/test-orca.sh` → 16/16.
- [ ] **Step 5 : Essai sur le poste du porteur** (préparé par Claude, fait par le porteur en 2 minutes) : dans Pupitre, `git pull` dans un dépôt GitHub → fenêtre de phrase de passe, Mémoriser ; second `git pull` → aucune question ; fermer tous les onglets, attendre 10 s, rouvrir Pupitre, `git pull` → aucune question.
- [ ] **Step 6 :** consigner dans `docs/verifications/etape-2.md`, cocher le plan, mettre à jour `suivi-tokens.md`, commit `docs: vérifications de l'étape 2`.

---

## Choix du concepteur (à valider par le porteur)

1. **La fenêtre de demande s'affiche dans Pupitre** (relais par le processus de fond), pas comme une fenêtre séparée : sous Wayland, une fenêtre lancée par un autre programme peut ne pas recevoir le focus clavier, ce qui la rendrait inutilisable avec Orca.
2. **`SSH_ASKPASS_REQUIRE=force`** : OpenSSH ne vérifie `prefer` que par `DISPLAY`, absent sous Wayland pur. Conséquence : dans Pupitre, toutes les questions de `ssh` (phrase, mot de passe, serveur inconnu) passent par la fenêtre, et git peut aussi y poser ses questions de nom d'utilisateur et de mot de passe HTTPS.
3. **« Mémoriser » coché par défaut** quand le trousseau est disponible (but : plus aucune saisie après la première).
4. **Clé non mémorisée = non chargée dans l'agent** : la phrase sera redemandée à la connexion suivante. Seules les clés mémorisées sont chargées automatiquement.
5. **Les clés mémorisées sont chargées dans l'agent trouvé**, même s'il s'agit de l'agent du bureau : l'environnement du bureau n'est pas modifié, mais git lancé hors de Pupitre pourra, lui aussi, profiter des clés chargées dans cet agent.
6. **Raccourci Ctrl+Maj+O** pour « Connexions SSH… ».
