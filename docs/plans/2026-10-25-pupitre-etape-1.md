# Pupitre — Étape 1 : terminal à onglets accessible et robuste — Plan d'implémentation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal :** livrer un terminal à onglets utilisable au quotidien, lisible par Orca, dont les shells survivent à un plantage de l'interface, avec les thèmes sombre et clair validés et une configuration TOML rechargée à chaud.

**Architecture :** deux binaires Rust. `pupitre-fond` possède les PTY et les shells, garde un tampon de relecture par session et parle à l'interface par un socket Unix (trames binaires versionnées). `pupitre` (GTK4 + VTE) affiche chaque session dans un `vte4::Terminal` alimenté par `feed()` ; les frappes (signal `commit`) et les tailles repartent vers le processus de fond.

**Tech Stack :** Rust (édition 2021), gtk4 (feature `v4_18`), vte4 (feature `v0_80`), serde + postcard (trames), toml, portable-pty, notify (surveillance de fichier), cargo-fuzz. Tests GUI sous Xvfb et Weston sans écran ; sonde AT-SPI en Python (pyatspi).

**Spec :** `docs/conception/2026-10-25-pupitre-conception.md` (sections 4, 5, 7, 8, 12, 13, 15). Résultats de mesure de VTE : `essai-jetable/RESULTATS.md`.

**Hors étape 1 (plans suivants)** : SSH/trousseau (étape 2) ; détection des shells, Starship, intégration OSC 133 (étape 3) ; bandeau clavier, notifications, OSC 52, thèmes contraste élevé, autres polices, paquet `.deb` (étape 4). À l'étape 1, chaque onglet lance le **shell de connexion** de l'utilisateur, sans intégration.

## Global Constraints

- Linux uniquement ; X11 **et** Wayland ; aucune API propre à X11 dans le code.
- GTK ≥ 4.18, VTE (gtk4) ≥ 0.80 — environnement de développement Debian 13.
- Licence GPL-3.0 (fichier `LICENSE` à la racine, en-tête SPDX `GPL-3.0-or-later` dans chaque fichier source).
- Aucune connexion réseau initiée par Pupitre ; rien de la session (historique, contenu) écrit sur disque.
- Aucun fichier de l'utilisateur hors `$XDG_CONFIG_HOME/pupitre/` et `$XDG_STATE_HOME/pupitre/` n'est écrit.
- Pas de `unwrap`/`expect`/`panic!` hors tests : `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]` dans chaque crate (sauf `#[cfg(test)]`).
- Toute écriture de fichier persistant passe par `ecrire_atomique` (Task 3).
- Textes d'interface en français ; identifiants de code en anglais ; commentaires en français.
- Contraste minimum 4,5:1 (texte), 3:1 (curseur) ; curseur non clignotant par défaut.
- Mettre à jour `suivi-tokens.md` (tokens et modèle) à la fin de chaque tâche.

## Review Focus

1. **Shell introuvable ou qui se termine aussitôt** (chemin invalide, `exit` dans le `.bashrc`) → l'onglet affiche « Le shell s'est terminé (code N) » et propose de rouvrir ; rien ne plante. Test : Task 5 `ouvrir_shell_inexistant_renvoie_erreur`, Task 9 `onglet_shell_termine_affiche_message`.
2. **Sortie énorme et continue** (`yes`, `cat` d'un gros fichier) → l'interface reste réactive, le tampon de relecture reste borné à 8 Mio. Test : Task 4 `tampon_reste_borne`, Task 6 `flux_massif_ne_bloque_pas`.
3. **Octets invalides / binaire** (UTF-8 cassé, coupure au milieu d'un caractère dans le tampon) → aucun plantage, affichage dégradé seulement. Test : Task 4 `tampon_coupe_sans_paniquer`, cible de fuzzing Task 2.
4. **Socket périmé ou second lancement** (processus de fond mort laissant son fichier socket, deux interfaces lancées) → la seconde interface rejoint le même processus de fond ; un socket mort est supprimé et un processus de fond neuf démarre. Test : Task 6 `socket_perime_est_remplace`, Task 7 `deux_clients_voient_les_memes_sessions`.
5. **Configuration absente, illisible, invalide ou police inconnue** → valeurs par défaut ou configuration précédente + message persistant indiquant la ligne. Test : Task 3 `toml_invalide_donne_ligne`, Task 10 `police_inconnue_replie_sur_b612`.

---

## Structure des fichiers

```
Cargo.toml                         espace de travail (crates/*)
LICENSE                            GPL-3.0
crates/proto/src/lib.rs            messages ClientMsg/ServerMsg, VERSION
crates/proto/src/trame.rs          écriture/lecture des trames (longueur + postcard)
crates/config/src/lib.rs           réexports
crates/config/src/couleur.rs       Rgb, luminance, ratio de contraste
crates/config/src/theme.rs         Theme, Role, thèmes intégrés sombre/clair, check_theme
crates/config/src/fichier.rs       Config (TOML), chargement, erreurs avec ligne, défaut commenté
crates/config/src/atomique.rs      ecrire_atomique
crates/fond/src/main.rs            binaire pupitre-fond
crates/fond/src/relecture.rs       ReplayBuffer (anneau borné)
crates/fond/src/session.rs         Session : PTY, shell, lecture, écriture, taille, occupé
crates/fond/src/serveur.rs         socket, clients, routage, arrêt automatique
crates/fond/fuzz/                  cibles cargo-fuzz (trames)
crates/pupitre/src/main.rs         binaire pupitre (+ sous-commande `check`)
crates/pupitre/src/client.rs       connexion au fond, démarrage auto, canal vers GTK
crates/pupitre/src/terminal.rs     TerminalOnglet : vte4::Terminal relié à une session
crates/pupitre/src/fenetre.rs      FenetrePupitre : onglets, détachement, fermeture
crates/pupitre/src/taille.rs       taille d'ouverture (nombre d'or)
crates/pupitre/src/polices.rs      enregistrement des polices embarquées
crates/pupitre/src/raccourcis.rs   raccourcis clavier
crates/pupitre/src/messages.rs     bandeau de messages dans la fenêtre
crates/pupitre/src/rechargement.rs surveillance et application de la configuration
assets/fonts/B612Mono-*.ttf, assets/fonts/OFL-B612.txt
tests/gui/                         tests d'intégration GTK (harness = false)
tests/a11y/sonde.py                sonde AT-SPI
scripts/test-x11.sh, scripts/test-wayland.sh
docs/orca-liste-de-controle.md
```

---

### Task 0 : Essai de levée de risque — VTE alimenté par `feed()` (jetable)

But : confirmer que VTE 0.80 fonctionne sans PTY propre (§4.1 de la spec). **Point de décision** : si un critère échoue, arrêter et revenir vers le porteur avec l'alternative (passer le descripteur du PTY à l'interface).

**Files :** Create: `essai-jetable/feed/` (projet Cargo séparé, hors espace de travail, conteneur `essai-jetable/debian13/` réutilisable).

- [x] **Step 1 : Écrire un mini-programme** : un `vte4::Terminal` sans `spawn`, un `portable-pty` géré par le programme lui-même ; les octets du PTY sont passés à `terminal.feed()`, le signal `commit` écrit dans le PTY, `size-allocate`/`notify::column-count` déclenche `pty.resize()`.
- [x] **Step 2 : Vérifier chaque critère** (script automatisé sous Xvfb, comme `essai-jetable/test.sh`) :
  - la saisie clavier arrive au shell (`commit`) ;
  - les réponses aux requêtes du terminal (DA1 `ESC[c`, DECRQM 2004) **reviennent par `commit`** ;
  - collage sécurisé : `ESC[?2004h` puis collage → le shell reçoit `ESC[200~…ESC[201~` ;
  - souris SGR : `ESC[?1006h ESC[?1000h` puis clic simulé (xdotool) → séquence `ESC[<0;…M` reçue ;
  - la sonde AT-SPI lit le texte (rôle `terminal`) ;
  - vim plein écran se redessine après redimensionnement.
- [x] **Step 3 : Consigner** les résultats dans `essai-jetable/feed/RESULTATS.md` (critère, attendu, obtenu). Si tous passent : continuer. Sinon : arrêter.

> **Résultat (2026-10-31) : les 6 critères passent.** Deux contraintes en découlent pour la Task 8 (voir `essai-jetable/feed/RESULTATS.md`) : VTE n'a pas de propriété `column-count` (détecter la taille dans `size_allocate`) et il ne faut pas dériver `vte4::Terminal` (plantage dans vte4-rs 0.9/0.10).

### Task 1 : Espace de travail, dépôt et outillage

**Files :** Create: `Cargo.toml`, `LICENSE`, `.gitignore`, `crates/*/Cargo.toml`, `crates/*/src/lib.rs` ou `main.rs` vides, `scripts/test-x11.sh`, `scripts/test-wayland.sh`, `rust-toolchain.toml` (canal stable).

- [x] **Step 1 :** `git init` à la racine de `pupitre/` ; `.gitignore` : `target/`, `essai-jetable/target*`, `essai-jetable/.cargo-trixie`, `*.png` hors `maquettes/`.
- [x] **Step 2 :** Espace de travail avec crates `proto`, `config`, `fond` (bin `pupitre-fond` + lib), `pupitre` (bin). Lints globaux dans `[workspace.lints]` (cf. Global Constraints). Dépendances : versions de `gtk4`/`vte4` **compatibles entre elles** (l'essai a montré que `vte4 0.10` + `gtk4 0.11` ne compile pas ; vérifier la paire avant de figer). → **2026-10-31 : `gtk4 0.11` (`v4_18`) + `vte4 0.10` (`v0_80`) compile et passe clippy ; paire retenue.**
- [x] **Step 3 :** `scripts/test-x11.sh` : `xvfb-run -a -s "-screen 0 1920x1080x24" dbus-run-session -- cargo test --workspace -- --include-ignored` ; `scripts/test-wayland.sh` : lance `weston --backend=headless --socket=pupitre-test &`, exporte `WAYLAND_DISPLAY=pupitre-test GDK_BACKEND=wayland`, lance les mêmes tests.
- [x] **Step 4 : Vérifier** : `cargo clippy --workspace --all-targets` → aucun avertissement ; `./scripts/test-x11.sh` → `test result: ok` (0 test).
- [x] **Step 5 : Commit** — `git commit -m "chore: espace de travail Pupitre (proto, config, fond, pupitre)"`

### Task 2 : Protocole entre l'interface et le processus de fond

**Files :** Create: `crates/proto/src/lib.rs`, `crates/proto/src/trame.rs`, `crates/fond/fuzz/fuzz_targets/trame.rs`.

**Interfaces :**
- Produces :
  - `pub const VERSION: u32 = 1;` `pub type SessionId = u64;`
  - `pub enum ClientMsg { Hello { version: u32 }, Open { argv: Option<Vec<String>>, cwd: Option<PathBuf>, cols: u16, rows: u16 }, Input { id: SessionId, data: Vec<u8> }, Resize { id: SessionId, cols: u16, rows: u16 }, Close { id: SessionId }, List, Attach { id: SessionId } }`
  - `pub enum ServerMsg { Welcome { version: u32 }, Opened { id: SessionId }, OpenFailed { error: String }, Output { id: SessionId, data: Vec<u8> }, Replay { id: SessionId, data: Vec<u8> }, Exited { id: SessionId, code: Option<i32> }, Sessions(Vec<SessionInfo>), Busy { id: SessionId, busy: bool }, Error(String) }`
  - `pub struct SessionInfo { pub id: SessionId, pub cols: u16, pub rows: u16, pub busy: bool }`
  - `pub const MAX_FRAME: usize = 16 * 1024 * 1024;`
  - `pub fn write_frame<W: Write, T: Serialize>(w: &mut W, msg: &T) -> io::Result<()>`
  - `pub fn read_frame<R: Read, T: DeserializeOwned>(r: &mut R) -> Result<T, FrameError>`
  - `pub enum FrameError { Io(io::Error), TooLarge(usize), Decode(String), Closed }`
- Format : longueur `u32` petit-boutiste + charge postcard. Tous les types dérivent `Serialize, Deserialize, Debug, Clone, PartialEq`.

- [x] **Step 1 : Tests** (dans `trame.rs`) :
```rust
#[test] fn aller_retour_chaque_message() { /* chaque variante de ClientMsg et ServerMsg écrite puis relue == originale */ }
#[test] fn trame_trop_grande_refusee() { /* en-tête annonçant MAX_FRAME + 1 → Err(FrameError::TooLarge(MAX_FRAME + 1)) */ }
#[test] fn flux_coupe_donne_closed() { /* lecteur vide → Err(FrameError::Closed) ; en-tête tronqué (2 octets) → Err(FrameError::Closed) */ }
#[test] fn charge_corrompue_donne_decode() { /* longueur 3 + octets [0xff,0xff,0xff] → Err(FrameError::Decode(_)) */ }
```
- [x] **Step 2 :** `cargo test -p pupitre-proto` → échec (fonctions absentes).
- [x] **Step 3 :** Implémenter `write_frame` / `read_frame`.
- [x] **Step 4 :** `cargo test -p pupitre-proto` → PASS.
- [x] **Step 5 :** Cible de fuzzing `trame` : octets arbitraires → `read_frame::<_, ClientMsg>` et `<_, ServerMsg>` ne paniquent jamais. Lancer `cargo +nightly fuzz run trame -- -max_total_time=60` → aucun crash.
- [x] **Step 6 : Commit** — `feat(proto): messages versionnés et trames`

### Task 3 : Couleurs, thèmes, vérificateur de contraste, configuration

**Files :** Create: `crates/config/src/{lib.rs,couleur.rs,theme.rs,fichier.rs,atomique.rs}`, `assets/config/pupitre.toml` (défaut commenté).

**Interfaces :**
- Produces :
  - `pub struct Rgb { pub r: u8, pub g: u8, pub b: u8 }` + `impl FromStr` (`"#rrggbb"`, insensible à la casse) + `impl Display` (`#rrggbb` minuscules).
  - `pub fn contrast_ratio(a: Rgb, b: Rgb) -> f64` (WCAG 2.x : luminance relative sRGB, seuil de linéarisation 0,03928).
  - `pub enum Role { Texte, Fond, TexteEtFond }`
  - `pub struct Theme { pub nom: String, pub fond: Rgb, pub texte: Rgb, pub curseur: Rgb, pub selection_fond: Rgb, pub selection_texte: Rgb, pub palette: [Rgb; 16], pub roles: [Role; 16] }`
  - `pub fn theme_sombre() -> Theme`, `pub fn theme_clair() -> Theme`
  - `pub struct ProblemeContraste { pub description: String, pub ratio: f64, pub minimum: f64 }`
  - `pub fn check_theme(t: &Theme) -> Vec<ProblemeContraste>`
  - `pub struct Config { pub police: String, pub theme: ThemeChoix, pub curseur_clignote: bool, pub historique_lignes: u32, pub zoom_max_pourcent: u16 }` ; `pub enum ThemeChoix { Sombre, Clair, Systeme, Fichier(PathBuf) }`
  - `pub fn charger(chemin: &Path) -> Result<Config, ErreurConfig>` ; `pub struct ErreurConfig { pub ligne: Option<usize>, pub message: String }`
  - `pub fn chemin_config() -> PathBuf` (`$XDG_CONFIG_HOME/pupitre/pupitre.toml`)
  - `pub fn creer_defaut_si_absent(chemin: &Path) -> io::Result<bool>`
  - `pub fn ecrire_atomique(chemin: &Path, contenu: &[u8]) -> io::Result<()>` (fichier temporaire dans le même dossier, `fsync`, copie de l'ancien en `<nom>.precedent`, `rename`).
- Valeurs exactes (spec §7.2) :
  - Sombre : fond `#1e1e2e`, texte `#cdd6f4`, curseur `#f5e0dc`, sélection `#45475a`/`#cdd6f4`, palette `#252630 #f38ba8 #a6e3a1 #f9e2af #89b4fa #f5c2e7 #94e2d5 #bac2de #8a8ea6 #f38ba8 #a6e3a1 #f9e2af #89b4fa #f5c2e7 #94e2d5 #b2b8cf` ; rôles : 0 = Fond, 8 = TexteEtFond, autres = Texte.
  - Clair : fond `#eff1f5`, texte `#4c4f69`, curseur `#4c4f69`, sélection `#ccd0da`/`#4c4f69`, palette `#4c4f69 #c30e35 #2d701e #895712 #0a54e7 #b31b8a #116c71 #dce0e8 #5c5f77 #c30e35 #2d701e #895712 #0a54e7 #b31b8a #116c71 #ffffff` ; rôles : 7 et 15 = Fond, 8 = TexteEtFond, autres = Texte.
  - Défauts `Config` : police `"B612 Mono 11"`, thème `Systeme`, curseur non clignotant, historique 10 000 lignes, zoom max 200.
- Règle de `check_theme` (minimum 4,5 ; curseur 3,0) : texte/fond ; sélection ; curseur/fond ; chaque couleur `Texte` ou `TexteEtFond` contre le fond ; chaque `TexteEtFond` contre chaque `Fond` ; chaque `Fond` sous chaque `Texte`. Description en français, ex. `"couleur 8 (#585b70) sur le fond (#1e1e2e)"`.

- [x] **Step 1 : Tests** :
```rust
#[test] fn ratios_de_reference() {
    assert!((contrast_ratio("#ffffff".parse()?, "#000000".parse()?) - 21.0).abs() < 0.01);
    assert!((contrast_ratio("#8a8ea6".parse()?, "#1e1e2e".parse()?) - 5.08).abs() < 0.01);
    assert!((contrast_ratio("#252630".parse()?, "#8a8ea6".parse()?) - 4.65).abs() < 0.01);
}
#[test] fn themes_integres_conformes() { assert!(check_theme(&theme_sombre()).is_empty()); assert!(check_theme(&theme_clair()).is_empty()); }
#[test] fn mocha_d_origine_signale_le_gris() {
    let mut t = theme_sombre(); t.palette[0] = "#45475a".parse()?; t.palette[8] = "#585b70".parse()?; t.palette[15] = "#a6adc8".parse()?;
    let p = check_theme(&t);
    assert!(p.iter().any(|x| x.description.contains("couleur 8") && (x.ratio - 2.46).abs() < 0.01));
    assert_eq!(p.len(), 7); // 1/0, 4/0, 9/0, 12/0, 15/0, 8/fond, 8/0
}
#[test] fn couleur_invalide_refusee() { assert!("#12010".parse::<Rgb>().is_err()); assert!("bleu".parse::<Rgb>().is_err()); }
#[test] fn config_absente_donne_defauts() { /* chemin inexistant → creer_defaut_si_absent crée le fichier ; charger → police "B612 Mono 11", historique 10_000 */ }
#[test] fn toml_invalide_donne_ligne() { /* contenu "police = \"B612 Mono 11\"\nhistorique_lignes = \"beaucoup\"\n" → Err(ErreurConfig { ligne: Some(2), .. }) */ }
#[test] fn ecriture_atomique_garde_precedent() { /* écrire "a" puis "b" → fichier == "b", <nom>.precedent == "a", aucun fichier temporaire restant */ }
```
- [x] **Step 2 :** `cargo test -p pupitre-config` → échec.
- [x] **Step 3 :** Implémenter `couleur.rs`, `theme.rs`, `fichier.rs`, `atomique.rs` ; `assets/config/pupitre.toml` commenté en français (chaque clé, ses valeurs possibles), inclus via `include_str!`.
- [x] **Step 4 :** `cargo test -p pupitre-config` → PASS.
- [x] **Step 5 : Commit** — `feat(config): thèmes validés, vérificateur de contraste, configuration TOML`

### Task 4 : Tampon de relecture

**Files :** Create: `crates/fond/src/relecture.rs`.

**Interfaces :**
- Produces : `pub struct ReplayBuffer` ; `pub const REPLAY_CAPACITY: usize = 8 * 1024 * 1024;` ; `pub fn new(capacity: usize) -> Self` ; `pub fn push(&mut self, data: &[u8])` ; `pub fn snapshot(&self) -> Vec<u8>` ; `pub fn len(&self) -> usize`.
- Comportement : anneau d'octets ; au-delà de la capacité, les plus anciens octets sont écartés ; `snapshot` commence au premier début de ligne (`\n` + 1) suivant le point de coupe, pour ne pas rejouer une séquence d'échappement tronquée (si aucun `\n`, tout le contenu).

- [ ] **Step 1 : Tests** :
```rust
#[test] fn conserve_tout_sous_la_capacite() { /* capacité 16, push "abc\n" → snapshot == "abc\n" */ }
#[test] fn tampon_reste_borne() { /* capacité 1024, push 10 Mio d'octets → len() <= 1024 */ }
#[test] fn reprend_au_debut_de_ligne() { /* capacité 8, push "aaaa\nbbbb\ncc" → snapshot == "cc" ou commence après un '\n' */ }
#[test] fn tampon_coupe_sans_paniquer() { /* push de "é" (2 octets) répétés pour forcer une coupe au milieu → snapshot ne panique pas */ }
```
- [ ] **Step 2 :** `cargo test -p pupitre-fond relecture` → échec.
- [ ] **Step 3 :** Implémenter (`VecDeque<u8>` ou anneau maison).
- [ ] **Step 4 :** → PASS.
- [ ] **Step 5 : Commit** — `feat(fond): tampon de relecture borné`

### Task 5 : Session (PTY + shell)

**Files :** Create: `crates/fond/src/session.rs`.

**Interfaces :**
- Consumes : `ReplayBuffer` (Task 4).
- Produces :
  - `pub struct Session` ; `pub fn open(argv: &[String], cwd: Option<&Path>, cols: u16, rows: u16, env: &[(String, String)]) -> Result<(Session, Receiver<SessionEvent>), OpenError>`
  - `pub enum SessionEvent { Output(Vec<u8>), Exited(Option<i32>) }`
  - `pub fn write_input(&self, data: &[u8]) -> io::Result<()>` ; `pub fn resize(&self, cols: u16, rows: u16) -> io::Result<()>` ; `pub fn is_busy(&self) -> bool` (groupe de premier plan du PTY ≠ processus du shell, via `tcgetpgrp`) ; `pub fn replay(&self) -> Vec<u8>` ; `pub fn kill(&self)`.
  - `pub fn login_shell() -> Vec<String>` (champ shell de `getpwuid`, repli `/bin/sh`).
- Environnement des shells : `TERM=xterm-256color`, `COLORTERM=truecolor`, `TERM_PROGRAM=Pupitre`.
- Thread de lecture par session, enveloppé dans `catch_unwind` : une panique ferme la session concernée (`Exited(None)`) sans toucher aux autres.

- [ ] **Step 1 : Tests** :
```rust
#[test] fn sortie_du_shell_recue() { /* argv ["/bin/sh","-c","printf bonjour"] → un Output contenant "bonjour" puis Exited(Some(0)) */ }
#[test] fn saisie_transmise() { /* /bin/sh interactif, write_input "echo $((6*7))\n" → sortie contient "42" */ }
#[test] fn redimensionnement_visible() { /* resize(100, 30) puis "stty size\n" → sortie contient "30 100" */ }
#[test] fn variables_d_environnement() { /* "echo $TERM $COLORTERM\n" → "xterm-256color truecolor" */ }
#[test] fn occupe_pendant_une_commande() { /* write_input "sleep 2\n", attendre 300 ms → is_busy() == true ; après fin → false */ }
#[test] fn ouvrir_shell_inexistant_renvoie_erreur() { /* argv ["/nexiste/pas"] → Err(OpenError::Spawn(_)) ou Exited(Some(code != 0)) rapidement, sans panique */ }
#[test] fn relecture_contient_la_sortie() { /* après "printf trace", replay() contient "trace" */ }
```
- [ ] **Step 2 :** `cargo test -p pupitre-fond session` → échec.
- [ ] **Step 3 :** Implémenter avec `portable-pty`.
- [ ] **Step 4 :** → PASS.
- [ ] **Step 5 : Commit** — `feat(fond): sessions PTY isolées`

### Task 6 : Serveur du processus de fond

**Files :** Create: `crates/fond/src/serveur.rs`, `crates/fond/src/main.rs`. Test: `crates/fond/tests/serveur.rs`.

**Interfaces :**
- Consumes : `ClientMsg`, `ServerMsg`, `write_frame`, `read_frame` (Task 2) ; `Session` (Task 5).
- Produces :
  - `pub fn socket_path() -> PathBuf` = `$XDG_RUNTIME_DIR/pupitre/fond.sock` (dossier créé en `0700`) ; si `XDG_RUNTIME_DIR` est absent (MX Linux démarré en sysVinit sans elogind, par exemple) : `/tmp/pupitre-<uid>/fond.sock`, dossier `0700` dont le propriétaire est vérifié (refus si autre propriétaire ou droits plus larges).
  - `pub fn run(socket: &Path, idle_exit: Duration) -> io::Result<()>`.
  - Binaire `pupitre-fond [--socket CHEMIN]`.
- Comportement : poignée de main `Hello{VERSION}` → `Welcome{VERSION}` (version différente → `Error("version")` puis fermeture) ; plusieurs clients ; `Output`/`Exited`/`Busy` diffusés à tous les clients ; `Attach` renvoie `Replay` ; **socket existant** : tenter une connexion — si elle échoue, supprimer le fichier et écouter ; si elle réussit, quitter avec le code 0 (un fond tourne déjà) ; arrêt automatique quand ni session ni client pendant `idle_exit` (5 s en production).

- [ ] **Step 1 : Tests d'intégration** (dossier temporaire comme `XDG_RUNTIME_DIR`) :
```rust
#[test] fn poignee_de_main() { /* Hello{1} → Welcome{1} ; Hello{99} → Error contenant "version" */ }
#[test] fn ouvrir_saisir_lire() { /* Open sh → Opened{id} ; Input "echo coucou\n" → Output contenant "coucou" */ }
#[test] fn session_survit_au_client() { /* client A ouvre une session, se déconnecte ; client B : List → 1 session ; Attach → Replay contenant la sortie précédente */ }
#[test] fn sans_xdg_runtime_dir_repli_sur_tmp() { /* XDG_RUNTIME_DIR retiré → socket_path() == /tmp/pupitre-<uid>/fond.sock, dossier en 0700 */ }
#[test] fn socket_perime_est_remplace() { /* créer un fichier socket sans serveur ; run() démarre et accepte une connexion */ }
#[test] fn arret_si_inactif() { /* idle_exit 200 ms, aucun client → run() retourne Ok en < 2 s */ }
#[test] fn flux_massif_ne_bloque_pas() { /* session "yes | head -c 50000000" ; un second client envoie List et reçoit Sessions en < 1 s pendant le flux */ }
```
- [ ] **Step 2 :** `cargo test -p pupitre-fond --test serveur` → échec.
- [ ] **Step 3 :** Implémenter (threads + canaux ; un thread par client ; pas de blocage d'un client lent sur les autres : file d'envoi bornée par client, client trop lent déconnecté).
- [ ] **Step 4 :** → PASS.
- [ ] **Step 5 : Commit** — `feat(fond): serveur, multi-clients, arrêt automatique`

### Task 7 : Client côté interface

**Files :** Create: `crates/pupitre/src/client.rs`. Test: `crates/pupitre/tests/client.rs`.

**Interfaces :**
- Consumes : protocole (Task 2), binaire `pupitre-fond` (Task 6).
- Produces :
  - `pub struct Client` ; `pub fn connect_or_spawn() -> Result<(Client, async_channel::Receiver<ServerMsg>), ClientError>` : se connecte à `socket_path()` ; à défaut lance `pupitre-fond` (même dossier que l'exécutable courant, sinon `PATH`) puis réessaie pendant 2 s.
  - `pub fn send(&self, msg: ClientMsg) -> Result<(), ClientError>` (non bloquant pour GTK : file + thread d'écriture).
  - Perte de connexion → message `ServerMsg::Error("connexion perdue")` sur le canal.

- [ ] **Step 1 : Tests** :
```rust
#[test] fn demarre_le_fond_si_absent() { /* XDG_RUNTIME_DIR temporaire, aucun fond → connect_or_spawn() réussit et Welcome reçu */ }
#[test] fn deux_clients_voient_les_memes_sessions() { /* client 1 ouvre une session ; client 2 : List → même id */ }
#[test] fn perte_de_connexion_signalee() { /* tuer le fond → le canal reçoit Error("connexion perdue") */ }
```
- [ ] **Step 2 :** → échec. **Step 3 :** Implémenter. **Step 4 :** → PASS.
- [ ] **Step 5 : Commit** — `feat(pupitre): client du processus de fond`

### Task 8 : Fenêtre, terminal VTE, police et taille d'ouverture

**Files :** Create: `crates/pupitre/src/{main.rs,terminal.rs,polices.rs,taille.rs}`, `assets/fonts/B612Mono-{Regular,Bold,Italic,BoldItalic}.ttf` (version officielle, dépôt polarsys/b612), `assets/fonts/OFL-B612.txt`. Test: `tests/gui/fenetre.rs`.

**Interfaces :**
- Consumes : `Client` (Task 7), `Theme`, `Config` (Task 3).
- Produces :
  - `pub fn taille_ouverture(largeur_ecran: i32) -> (i32, i32)` = `(round(l × 0,5), round(l × 0,5 / 1,618))`.
  - `pub fn enregistrer_polices_embarquees(dossier: &Path) -> bool` (ajoute le dossier à fontconfig via `FcConfigAppFontAddDir` ; dossier : `$PUPITRE_ASSETS/fonts`, sinon `/usr/share/pupitre/fonts`, sinon `assets/fonts` en développement).
  - `pub struct TerminalOnglet` ; `pub fn new(client: Rc<Client>, id: SessionId, config: &Config, theme: &Theme) -> Self` ; `pub fn widget(&self) -> &CadreTerminal` (conteneur `gtk::Widget` qui contient le `vte4::Terminal` ; **ne jamais dériver `vte4::Terminal`**, cf. Task 0) ; `pub fn terminal(&self) -> &vte4::Terminal` ; `pub fn on_output(&self, data: &[u8])` (→ `feed`) ; `pub fn appliquer(&self, config: &Config, theme: &Theme)`.
  - Réglages VTE : `set_colors(texte, fond, palette)`, `set_color_cursor`, `set_color_highlight`, `set_font`, `set_cursor_blink_mode(Off | On selon config)`, `set_scrollback_lines(historique_lignes)`, `set_allow_hyperlink(true)`, `set_audible_bell(false)`.
  - `commit` → `ClientMsg::Input` ; changement de colonnes/lignes → `ClientMsg::Resize`, détecté dans `CadreTerminal::size_allocate` (allouer l'enfant, puis comparer `column_count()`/`row_count()` à la grille précédente — VTE n'a pas de propriété `column-count`).
  - Ctrl+Maj+V → `paste_clipboard()` (VTE n'a pas de raccourci de collage propre).
  - Thème `Systeme` : lit la préférence clair/sombre du bureau via le portail freedesktop (`org.freedesktop.portal.Settings`, `ReadOne("org.freedesktop.appearance", "color-scheme")`, D-Bus local) et suit le signal `SettingChanged` ; portail absent ou valeur « sans préférence » → thème sombre.
  - Texte atténué (SGR 2) : mesurer sur capture le contraste rendu par VTE pour `ESC[2m` avec chaque thème ; s'il passe sous 4,5:1, désactiver l'atténuation (rendu normal) et le noter dans `docs/verifications/etape-1.md`.

- [ ] **Step 1 : Tests** :
```rust
#[test] fn taille_nombre_d_or() { assert_eq!(taille_ouverture(1920), (960, 593)); assert_eq!(taille_ouverture(2560), (1280, 791)); }
// tests/gui/fenetre.rs (harness = false, sous scripts/test-x11.sh et test-wayland.sh)
// fenetre_s_ouvre_et_affiche_le_shell : lancer pupitre, la sonde AT-SPI trouve un rôle "terminal" dont le texte contient l'invite.
// police_b612_utilisee : la police effective (pango) de la vue terminal a pour famille "B612 Mono".
// curseur_fixe_par_defaut : terminal.cursor_blink_mode() == Off.
// theme_suit_le_systeme : portail simulé (service D-Bus de test) renvoyant color-scheme = 2 (clair) → fond #eff1f5 ; signal SettingChanged à 1 (sombre) → fond #1e1e2e.
// texte_attenue_lisible : "printf '\e[2mattenue\e[0m'" puis capture → contraste mesuré des pixels du texte ≥ 4,5:1 avec chaque thème.
```
- [ ] **Step 2 :** → échec. **Step 3 :** Implémenter. **Step 4 :** `./scripts/test-x11.sh` et `./scripts/test-wayland.sh` → PASS.
- [ ] **Step 5 : Commit** — `feat(pupitre): fenêtre VTE reliée au processus de fond`

### Task 9 : Onglets, fenêtres multiples, fermeture et reprise après plantage

**Files :** Create: `crates/pupitre/src/fenetre.rs`. Modify: `main.rs`. Test: `tests/gui/onglets.rs`.

**Interfaces :**
- Consumes : `TerminalOnglet` (Task 8), `Client` (Task 7), `ServerMsg::{Busy, Exited, Sessions}`.
- Produces : `pub struct FenetrePupitre` ; `pub fn nouvelle(app: &gtk::Application, client: Rc<Client>) -> Self` ; `pub fn ajouter_onglet(&self, id: SessionId)` ; `pub fn ouvrir_session(&self)` ; `pub fn detacher_onglet(&self, id: SessionId)`.
- Comportement :
  - `gtk::Notebook` : onglets réorganisables et détachables (glisser vers l'extérieur ou menu « Détacher dans une nouvelle fenêtre ») ; renommage par double-clic ou menu ; nom accessible de chaque onglet = titre.
  - Titre : titre envoyé par le programme, sinon dernier élément du dossier courant (OSC 7).
  - Fermeture d'un onglet/fenêtre : si une session est `busy` → dialogue « Une commande est en cours dans cet onglet. Fermer quand même ? » (Annuler par défaut) ; si confirmé → `ClientMsg::Close`.
  - `Exited` → l'onglet affiche « Le shell s'est terminé (code N). Appuyez sur Entrée pour en ouvrir un nouveau. » (zone de texte lisible par Orca) ; pas de fermeture automatique.
  - **Au démarrage** : `List` ; s'il existe des sessions (reprise après plantage), recréer un onglet par session dans une fenêtre, `Attach` chacune et donner `Replay` à `feed()` ; sinon ouvrir une session.
  - **Connexion perdue** : message persistant « Le processus de fond s'est arrêté : les shells ouverts sont perdus. » + bouton « Ouvrir un nouveau terminal ».

- [ ] **Step 1 : Tests GUI** :
```text
nouvel_onglet_ouvre_une_session      : Ctrl+Maj+T → 2 onglets, 2 sessions côté fond.
fermeture_occupee_demande_confirmation : lancer "sleep 30" puis Ctrl+Maj+W → dialogue présent (AT-SPI rôle "dialog") ; Annuler → onglet conservé.
onglet_shell_termine_affiche_message : taper "exit" → texte "Le shell s'est terminé (code 0)" présent dans l'arbre AT-SPI.
reprise_apres_plantage               : taper "echo avant-plantage" ; kill -9 de l'interface ; relancer pupitre → 1 onglet dont le texte contient "avant-plantage" ; la session répond à "echo apres".
```
- [ ] **Step 2 :** → échec. **Step 3 :** Implémenter. **Step 4 :** tests X11 et Wayland → PASS.
- [ ] **Step 5 : Commit** — `feat(pupitre): onglets, fenêtres, reprise après plantage`

### Task 10 : Rechargement de la configuration, messages et `pupitre check`

**Files :** Create: `crates/pupitre/src/{rechargement.rs,messages.rs}`. Modify: `main.rs` (sous-commande). Test: `tests/gui/configuration.rs`, `crates/pupitre/tests/check.rs`.

**Interfaces :**
- Consumes : `charger`, `check_theme`, `chemin_config` (Task 3) ; `TerminalOnglet::appliquer` (Task 8).
- Produces :
  - `pub fn surveiller(chemin: &Path, rappel: impl Fn(Result<Config, ErreurConfig>) + 'static)` (crate `notify`, regroupement des événements sur 200 ms ; suit aussi le remplacement du fichier par `rename`).
  - `pub struct BandeauMessages` ; `pub fn afficher(&self, texte: &str, duree: Option<Duration>)` (`None` = persistant avec bouton Fermer) ; chaque message est annoncé (`gtk::Accessible::announce`, priorité polie).
  - `pupitre check [--theme CHEMIN]` : affiche chaque `ProblemeContraste` (`"couleur 8 (#585b70) sur le fond (#1e1e2e) : 2,46:1, minimum 4,5:1"`), code de sortie 0 si aucun, 1 sinon.
- Comportement : configuration valide → appliquée à tous les onglets ; invalide → configuration précédente conservée + message persistant « Configuration invalide, ligne N : … » ; thème avec problèmes de contraste → appliqué + message persistant listant les problèmes ; police introuvable (aucune famille fontconfig correspondante) → B612 Mono + message.

- [ ] **Step 1 : Tests** :
```text
check_themes_integres_ok          : `pupitre check` → code 0.
check_theme_mocha_echoue          : thème fichier avec 8 = #585b70 → code 1, sortie contient "2,46:1".
rechargement_change_les_couleurs  : écrire theme = "clair" → couleur de fond VTE == #eff1f5 en < 1 s.
config_invalide_garde_l_ancienne  : écrire "historique_lignes = \"x\"" → fond inchangé, message contenant "ligne 1" dans l'arbre AT-SPI.
police_inconnue_replie_sur_b612   : police = "Nexiste Pas 11" → famille effective "B612 Mono", message présent.
```
- [ ] **Step 2 :** → échec. **Step 3 :** Implémenter. **Step 4 :** → PASS.
- [ ] **Step 5 : Commit** — `feat(pupitre): configuration à chaud, messages, pupitre check`

### Task 11 : Clavier, zoom, recherche, copie, liens, cloche

**Files :** Create: `crates/pupitre/src/raccourcis.rs`. Modify: `terminal.rs`, `fenetre.rs`. Test: `tests/gui/clavier.rs`.

**Interfaces :**
- Produces : `pub fn installer_raccourcis(fenetre: &FenetrePupitre)` ; table des raccourcis (spec §12.1) : Ctrl+Maj+T/W/N (onglet, fermer, fenêtre), Ctrl+Maj+C/V (copier/coller), Ctrl+Maj+F (recherche), Ctrl+PgSuiv/PgPréc, Alt+1…9, F11, Ctrl+plus/moins/0 (zoom).
- Comportement :
  - Raccourcis reconnus par le caractère produit, avec repli sur le code physique de la touche (rangée des chiffres pour Alt+1…9) — à vérifier avec la disposition `fr` variante `dvorak`.
  - Zoom : `set_font_scale` de 0,5 à `zoom_max_pourcent/100` (2,0) par pas de 0,1 ; Ctrl+0 → 1,0.
  - Recherche : barre `gtk::SearchBar` au-dessus du terminal, `search_set_regex` (texte simple échappé ou expression régulière via case à cocher), Entrée/Maj+Entrée = suivant/précédent, Échap ferme et rend le focus au terminal.
  - Copie : `copy_clipboard_format(Text)` ; collage : `paste_clipboard` ; sélection primaire active ; pas de copie automatique.
  - URL en texte brut : `match_add_regex` (http/https/file), ouverture uniquement sur Ctrl+clic via `gtk::UriLauncher`.
  - Cloche : signal `bell` → flash de la zone terminal (une seule inversion de 150 ms, donc ≤ 3 flashs/s même pour des cloches répétées : ignorer les cloches à moins de 350 ms d'intervalle) + ● sur l'onglet s'il n'est pas actif ; aucun son.
  - Réduire les animations : si `gtk-enable-animations` est faux, curseur forcé non clignotant et flash remplacé par l'indicateur d'onglet seul.

- [ ] **Step 1 : Tests GUI** :
```text
zoom_jusqu_a_200                : 20 × Ctrl+plus → font_scale == 2.0 ; Ctrl+0 → 1.0.
alt_chiffre_change_d_onglet     : 3 onglets, disposition "fr dvorak" (setxkbmap sous Xvfb) ; Alt+touche physique "2" → onglet 2 actif.
recherche_trouve_le_texte       : "echo aiguille" ; Ctrl+Maj+F, saisir "aiguille", Entrée → search_find_previous() a trouvé (sélection non vide).
cloches_rapprochees_limitees    : 10 BEL en 1 s → ≤ 3 flashs comptés.
lien_non_ouvert_sans_ctrl       : clic simple sur une URL → aucun appel à UriLauncher (lanceur simulé).
```
- [ ] **Step 2 :** → échec. **Step 3 :** Implémenter. **Step 4 :** → PASS.
- [ ] **Step 5 : Commit** — `feat(pupitre): raccourcis, zoom, recherche, copie, liens, cloche`

### Task 12 : Accessibilité — sonde automatique et liste de contrôle Orca

**Files :** Create: `tests/a11y/sonde.py`, `tests/gui/accessibilite.rs`, `docs/orca-liste-de-controle.md`.

**Interfaces :**
- Consumes : application complète (Tasks 8–11).
- Produces : `tests/a11y/sonde.py --app pupitre --attendu FICHIER.json` (retourne 0 si l'arbre correspond : rôles, noms, présence de texte) ; liste de contrôle d'environ 15 minutes pour le porteur.

- [ ] **Step 1 : Tests** :
```text
arbre_accessible             : fenêtre (rôle frame, nom "Pupitre"), liste d'onglets (page tab list) dont chaque onglet a un nom non vide, terminal (rôle terminal) avec texte.
tout_au_clavier              : en partant du terminal, Tab/Maj+Tab et les raccourcis atteignent : onglets, menu, barre de recherche, dialogue de fermeture ; le focus est toujours sur un élément visible (AT-SPI state "focused").
messages_annonces            : un message du bandeau produit un événement d'annonce AT-SPI.
```
- [ ] **Step 2 :** → échec. **Step 3 :** Corriger ce qui manque (noms accessibles, relations `labelled-by`, ordre de focus).
- [ ] **Step 4 :** → PASS sous X11 et Wayland.
- [ ] **Step 5 :** Rédiger `docs/orca-liste-de-controle.md` : 10 à 15 vérifications concrètes (Orca lit l'invite ; lit la sortie d'une commande ; annonce le changement d'onglet et son titre ; lit le dialogue de fermeture ; lit un message de configuration invalide ; zoom 200 % sans perte de contenu ; recherche utilisable à l'oreille…), chacune avec la manipulation et le résultat attendu.
- [ ] **Step 6 : Commit** — `test(a11y): sonde AT-SPI, liste de contrôle Orca`

### Task 13 : Vérification de fin d'étape

- [ ] **Step 1 :** `cargo clippy --workspace --all-targets` → 0 avertissement ; `./scripts/test-x11.sh` et `./scripts/test-wayland.sh` → PASS ; `cargo +nightly fuzz run trame -- -max_total_time=300` → aucun crash.
- [ ] **Step 2 :** Vérifier l'absence de connexion réseau : lancer Pupitre sous `unshare -n` (ou `strace -f -e trace=connect`) pendant une session type → aucun `connect` hors socket Unix.
- [ ] **Step 3 :** Vérifier que rien n'est écrit hors `$XDG_CONFIG_HOME/pupitre` et `$XDG_STATE_HOME/pupitre` (`strace -f -e trace=openat,rename` filtré sur `O_WRONLY|O_CREAT`).
- [ ] **Step 4 :** Machines virtuelles GNOME/Wayland, KDE/Wayland, Sway : lancer Pupitre, ouvrir 3 onglets, `kill -9` de l'interface, relancer → sessions retrouvées. Consigner dans `docs/verifications/etape-1.md`.
- [ ] **Step 5 :** Remettre au porteur : instructions de lancement (`cargo run --release -p pupitre`), la liste de contrôle Orca, et le résumé des vérifications. Mettre à jour `suivi-tokens.md`.
- [ ] **Step 6 : Commit** — `docs: vérifications de l'étape 1`
