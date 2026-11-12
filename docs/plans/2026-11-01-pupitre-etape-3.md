# Pupitre — Étape 3 : shells et invite — Plan d'implémentation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal :** Pupitre détecte les shells installés et lance chacun avec son intégration (sans toucher aux fichiers de l'utilisateur) ; l'invite Starship (celle du système) suit le thème ; sans Starship, chaque shell garde l'invite de l'utilisateur ; le processus de fond repère les marques de commandes (OSC 133), ce qui permet d'annoncer à Orca la fin d'une commande avec son résultat et de naviguer de commande en commande au clavier.

**Architecture :** une nouvelle crate `pupitre-shell` (sans GTK) détecte les shells, leur niveau de prise en charge, la commande d'installation adaptée à la distribution, et construit le **lancement** d'un shell (argv + variables). Les fichiers d'intégration (`assets/shell/`, installés plus tard dans `/usr/share/pupitre/shell/`) chargent d'abord la configuration habituelle de l'utilisateur, puis l'invite et les marques OSC 133 / OSC 7. L'interface calcule le lancement et l'envoie au fond (protocole v3 : ouverture avec variables). `pupitre-fond` lit les marques OSC 133 dans le flux, mesure la durée des commandes et transmet les marques **dans l'ordre** du flux ; l'interface note la ligne VTE de chaque marque, annonce les fins de commande et permet de sauter d'une commande à l'autre.

**Tech Stack :** Rust, GTK 4.18 / VTE 0.80 (existants), Starship du système (1.22.1 sous Debian 13, paquet `starship`, dépendance recommandée du futur `.deb`), bash 5.2, zsh 5.9, fish 4.0, nushell 0.116 ; niveau 2 : elvish 0.21, xonsh 0.19, tcsh 6.24.

**Spec :** `docs/conception/2026-10-25-pupitre-conception.md`, §4.2 (OSC 133 intercepté par le fond), §6 (shells et invite), §7.2 (palette), §8 (annonce de fin de commande, navigation), §11 (rien de téléchargé à l'exécution), §12 (configuration), §13 (robustesse), §15 (étape 3).

**Hors étape 3 :** coche ✓/✗ sur l'onglet et bulle du bureau pour une commande finie en arrière-plan, OSC 9/777, OSC 52, bandeau de disposition clavier (étape 4) ; paquet `.deb` (étape 4 : il embarquera `assets/shell/` et recommandera le paquet `starship`).

## Global Constraints

- Tout ce qui figure dans les Global Constraints des étapes 1 et 2 reste valable (X11 et Wayland, GTK ≥ 4.18, VTE ≥ 0.80, SPDX `GPL-3.0-or-later`, pas de `unwrap`/`expect`/`panic!` hors tests, `ecrire_atomique`, textes en français, jamais de secret en clair, `suivi-tokens.md` à jour).
- **Aucun texte affiché écrit en dur** (à partir de la Task 0bis) : chaque texte vu ou entendu par l'utilisateur (interface, annonces d'Orca, messages, sortie des commandes, aide) passe par `pupitre-i18n` avec un identifiant ; le texte français est dans `crates/i18n/langues/fr/pupitre.ftl`. Les tests qui comparent des phrases fixent `LANGUAGE=fr`.
- **Aide à jour** : chaque tâche qui change un comportement visible met à jour `assets/aide/fr.md` (et retire la mention « (à venir) » des fonctions livrées). La Task 9 vérifie qu'il n'en reste plus pour l'étape 3.
- **Fichiers de l'utilisateur jamais modifiés ni remplacés** : `~/.bashrc`, `~/.zshrc`, `~/.config/fish/`, `~/.config/nushell/`, `~/.config/starship.toml`… Ce dernier n'est **jamais lu** (invite configurée par `STARSHIP_CONFIG`).
- **Les fichiers d'intégration chargent d'abord la configuration habituelle de l'utilisateur**, puis ajoutent l'intégration Pupitre.
- **Rien n'est téléchargé à l'exécution** : Starship n'est jamais installé par Pupitre (dépendance recommandée du paquet) ; Pupitre affiche une commande d'installation pour un shell absent, sans jamais l'exécuter.
- **Jamais d'erreur bloquante** : shell configuré absent, Starship absent ou en échec, fichiers d'intégration introuvables → l'onglet s'ouvre quand même (repli : shell de connexion, invite de l'utilisateur, shell tel quel), avec un message clair.
- Niveaux (§6.1) : **Complet** = bash, zsh, fish, nushell (Starship, OSC 133, dossier courant OSC 7 ; testés à chaque version) ; **Invite gérée** = elvish, xonsh, tcsh, ion, PowerShell (`pwsh`) ; **Tel quel** = tout autre shell.
- Couleurs de l'invite : uniquement les **16 couleurs de la palette** (noms ANSI de Starship : `green`, `red`, `blue`, `purple`, `yellow`, `cyan`, `bright-black`…), jamais de couleur exacte : changer de thème change l'invite, y compris celles déjà affichées. Aucune icône Nerd Font.
- Le flux de chaque session est relayé **tel quel** à VTE (les marques OSC 133 aussi : VTE les ignore) ; le fond n'ajoute ni ne retire aucun octet pour l'OSC 133.
- Protocole **v3** rétrocompatible : un fond v3 accepte les interfaces v1, v2 et v3 ; une interface v3 face à un fond v2 ouvre les onglets à l'ancienne (sans variables d'intégration) et n'a pas de marques. Variantes nouvelles **ajoutées en fin d'enum**.

## Review Focus

1. **Configuration de l'utilisateur qui pose déjà ses propres crochets** (`PROMPT_COMMAND`, `precmd`, `starship init` dans `~/.bashrc`, fish 4 qui émet lui-même l'OSC 133) → une seule marque de chaque sorte par commande, donc une seule annonce. Test : Task 2 `marques_uniques_avec_crochets_utilisateur` (bash, zsh) et `fish_sans_double_marque`.
2. **Séquence OSC 133 coupée entre deux lectures du PTY, mal formée ou démesurée** (`cat` d'un fichier binaire) → ni plantage ni blocage, mémoire bornée, octets relayés intacts. Test : Task 5 `marque_coupee_en_deux`, `osc_demesure_abandonne`, et cible de fuzzing `marques`.
3. **Fausses marques pendant une commande** (un programme ou un `cat` affiche `ESC]133;A`) → ignorées tant que la commande en cours n'est pas finie ; seule la marque de fin (D) est prise. Test : Task 5 `marques_ignorees_pendant_une_commande`.
4. **Reprise après plantage de l'interface** → les marques rejouées servent à la navigation mais ne déclenchent **aucune** annonce. Test : Task 5 `relecture_rejoue_les_marques`, Task 7 `marques_rejouees_sans_annonce`.
5. **Shell configuré absent, Starship absent, dossier d'intégration absent** → onglet ouvert avec le repli, message unique avec la commande d'installation, aucun plantage. Test : Task 1 `integration_absente_tel_quel_avec_avertissement`, Task 3 `starship_absent_invite_utilisateur`, Task 6 `shell_configure_absent_message`.

---

## Structure des fichiers

```
crates/shell/Cargo.toml                 nouvelle crate pupitre-shell (sans GTK)
crates/shell/src/lib.rs                 réexports
crates/shell/src/detection.rs           ShellConnu, Niveau, shells_installes, niveau_de
crates/shell/src/installation.rs        Distribution, commande_installation
crates/shell/src/lancement.rs           Lancement, lancement, ModeInvite, Ressources
crates/shell/src/starship.rs            chercher_starship, configuration par défaut
assets/shell/bash/pupitre.bash          intégration bash (--rcfile)
assets/shell/zsh/.zshenv, .zshrc        intégration zsh (ZDOTDIR)
assets/shell/fish/pupitre.fish          intégration fish (-C)
assets/shell/nushell/pupitre.nu         intégration nushell
assets/shell/elvish/pupitre.elv, xonsh/pupitre.xsh, tcsh/…   niveau 2 (selon Task 0)
assets/shell/starship.toml              invite par défaut (copiée au premier lancement)
crates/proto/src/lib.rs                 VERSION 3, Ouvrir, Marque, MarqueCommande, EtatTrousseau::Lent
crates/fond/src/marques.rs              LecteurMarques (OSC 133 dans le flux, borné)
crates/fond/src/serveur.rs              Ouvrir, envoi des marques dans l'ordre, relecture
crates/fond/fuzz/fuzz_targets/marques.rs  cible de fuzzing
crates/config/src/fichier.rs            shell, invite, annonce_fin_commande, [[shells]]
crates/pupitre/src/shells.rs            menu « Nouvel onglet avec », pupitre shells, messages
crates/pupitre/src/commandes.rs         marques par onglet, annonces, navigation
crates/pupitre/src/raccourcis.rs        CommandePrecedente / CommandeSuivante
crates/pupitre/tests/gui_commandes.rs   tests graphiques des marques
tests/a11y/orca/scenario.py             vérifications 17 et 18
tests/vm/commun.sh                      essais d'invite et de marques sous Sway, GNOME, KDE
docs/verifications/etape-3.md
```

---

### Task 0 : Levée de risque — shells, Starship et OSC 133 réels (jetable)

But : figer les mécanismes dont dépend le plan. **Point de décision** : si un critère échoue, arrêter et revenir vers le porteur.

**Files :** Create: `essai-jetable/shell/essai.sh`, `essai-jetable/shell/RESULTATS.md`.

- [x] **Step 1 : bash.** Avec `HOME` temporaire contenant un `.bashrc` qui pose `export VU_BASHRC=1` et son propre `PROMPT_COMMAND`, lancer `bash --rcfile <fichier> -i` dans un PTY (`script -qc`) : relever si `/etc/bash.bashrc` est lu, si `VU_BASHRC` est posé, si `PS0` est développé avant chaque commande.
- [x] **Step 2 : zsh.** `ZDOTDIR=<dossier Pupitre>` : vérifier que nos `.zshenv`/`.zshrc` peuvent rendre `ZDOTDIR` d'origine et charger ceux de l'utilisateur (`VU_ZSHRC=1`) ; `precmd_functions` / `preexec_functions` disponibles.
- [x] **Step 3 : fish 4.0.2 et nushell 0.116.** Relever les octets émis autour d'une commande : fish émet-il déjà `ESC]133;A/B/C/D` ? nushell avec `$env.config.shell_integration.osc133 = true` ? Option de lancement de fish qui s'exécute **après** `config.fish` (`-C`) ; option de nushell pour un fichier supplémentaire après celui de l'utilisateur.
- [x] **Step 4 : niveau 2.** elvish (`-rc`), xonsh (`--rc`), tcsh (existe-t-il un moyen sans toucher `~/.tcshrc` ? sinon tcsh passe « tel quel » et on le note).
- [x] **Step 5 : Starship** (celui du système, `/usr/bin/starship`). `starship init bash|zsh|fish|nu|elvish|xonsh|tcsh --print-full-init` : émet-il de l'OSC 133 lui-même ? `STARSHIP_CONFIG` respecté, `~/.config/starship.toml` ignoré (vérifié par `strace -e openat`).
- [x] **Step 6 : Consigner** dans `RESULTATS.md`. **Critères :** pour bash, zsh, fish, nushell, un mécanisme charge la configuration de l'utilisateur puis la nôtre sans écrire dans `HOME` ; Starship n'ouvre pas `~/.config/starship.toml`.

---

### Task 0bis : Traduction — mise en place (français seulement)

Décidé par le porteur le 2026-11-01 : Pupitre sera traduit en anglais, espagnol, italien, allemand et néerlandais. Cette tâche pose le système ; les traductions sont une étape à part (conception §15). Elle passe **avant** les autres pour que les nouveaux textes de l'étape 3 y entrent directement.

**Files :** Create: `crates/i18n/` (crate `pupitre-i18n`, sans GTK), `crates/i18n/langues/fr/pupitre.ftl`, `crates/i18n/langues/{en,es,it,de,nl}/pupitre.ftl` (vides, avec un en-tête) ; Move: `assets/aide/fr.md` → `assets/aide/fr.md` ; Modify: tous les fichiers de `crates/pupitre/src/`, `crates/fond/src/` et `crates/ssh/src/` qui contiennent des textes affichés, `crates/config/src/` (messages d'erreur), `assets/config/pupitre.toml` → produit depuis `fr.ftl` (commentaires), `assets/fr.pupitre.Pupitre.desktop` (`Name[fr]`, `Comment[fr]`, + entrées vides prêtes).

**Interfaces :**
- Produces : `fn langues_demandees(env: &dyn Fn(&str) -> Option<String>) -> Vec<LanguageIdentifier>` (ordre : `LANGUAGE` (liste séparée par `:`), puis `LC_ALL`, `LC_MESSAGES`, `LANG` ; `C`/`POSIX` ignorés ; `de_AT.UTF-8` → `de-AT` puis `de`) ; `fn init(langues: &[LanguageIdentifier])` (une fois au démarrage, chaque binaire) ; `fn t(id: &str) -> String` ; `fn ta(id: &str, args: &[(&str, ArgI18n)]) -> String` avec `enum ArgI18n { Texte(String), Nombre(i64) }` (pluriels Fluent) ; `fn langue_active() -> LanguageIdentifier` ; `fn aide() -> &'static str` (aide de la langue active, sinon anglais, sinon français). Bibliothèques : `fluent-bundle`, `unic-langid` (licences MIT/Apache, compatibles GPL-3.0). Fichiers `.ftl` **intégrés à l'exécutable** (`include_str!`) : traduction toujours disponible, hors ligne.
- Repli, par phrase : langue demandée → **anglais** (pour une langue que Pupitre ne connaît pas, l'anglais est plus souvent compris que le français) → **français** (langue source, toujours complète). Un identifiant inconnu partout donne l'identifiant lui-même, jamais de plantage.
- Réglage `langue = ""` dans `pupitre.toml` (vide : langue du système ; sinon `"fr"`, `"en"`, `"es"`, `"it"`, `"de"`, `"nl"`), appliqué au prochain lancement.
- Ce qui **n'est pas traduit** : les noms des réglages et des valeurs de `pupitre.toml` (`police`, `theme`, `"sombre"`…), les sous-commandes (`check`, `aide`, `shells` ; alias anglais `help` accepté), les noms de fichiers.

- [x] **Step 1 : Tests** : `langues_par_variables` (`LANGUAGE=nl:de`, `LANG=fr_BE.UTF-8` → `[nl, de]` ; `LANG=de_AT.UTF-8` seul → `[de-AT, de]` ; `LANG=C` → `[]`) ; `repli_par_phrase` (identifiant présent seulement en `fr` + langue `de` → texte français ; présent en `en` → texte anglais) ; `identifiant_inconnu_sans_plantage` ; `pluriel` (`fr` : 1 → « 1 problème », 3 → « 3 problèmes ») ; `sans_marques_d_isolement` (aucun U+2068/U+2069 dans un texte avec argument : `set_use_isolating(false)`, sinon Orca et le terminal les afficheraient) ; `tous_les_ftl_valides` (chaque fichier se lit sans erreur) ; `identifiants_utilises_existent_en_fr` (chaque `t("…")` / `ta("…"` du dépôt existe dans `fr.ftl`, recherche dans les sources) ; `aucun_identifiant_orphelin` (chaque identifiant de `fr.ftl` est utilisé) ; `memes_arguments_partout` (une traduction n'utilise que les arguments de la phrase française) ; `textes_inchanges` : tous les tests existants (unitaires, graphiques, Orca 1 à 16) passent avec `LANGUAGE=fr` **sans modifier une seule phrase attendue**.
- [x] **Step 2 :** échec.
- [x] **Step 3 : Implémenter.** Remplacer chaque texte affiché par un identifiant (noms en français, en tirets : `onglet-fermer-question`). `pupitre-fond` et `pupitre-askpass` appellent `init` avec les variables reçues. Le `.desktop` reste un fichier (les bureaux lisent `Name[de]`…) : un test vérifie que `Name[fr]` et `Comment[fr]` correspondent à `fr.ftl`.
- [x] **Step 4 :** tests verts ; `LANGUAGE=de cargo run` → interface en français (aucune traduction encore), sans avertissement ni plantage.
- [x] **Step 5 : Commit** `feat(i18n): système de traduction (Fluent), textes français extraits`.

---

### Task 1 : crate `pupitre-shell` — détection, niveaux, installation, lancement

**Files :** Create: `crates/shell/{Cargo.toml,src/lib.rs,src/detection.rs,src/installation.rs,src/lancement.rs}` ; Modify: `Cargo.toml` (membre).

**Interfaces :**
- Produces : `enum Niveau { Complet, InviteGeree, TelQuel }` ; `struct ShellConnu { nom: &'static str, executable: &'static str, niveau: Niveau, paquets: &[(Distribution, &'static str)] }` ; `const SHELLS_CONNUS: &[ShellConnu]` (bash, zsh, fish, nu, elvish, xonsh, tcsh, ion, pwsh) ; `fn niveau_de(chemin: &Path) -> Niveau` (par le nom du fichier, `-` de connexion retiré) ; `fn shells_installes(etc_shells: &str, path: &OsStr) -> Vec<ShellInstalle>` avec `ShellInstalle { chemin: PathBuf, nom: String, niveau: Niveau }` (dédoublonnés par chemin réel) ; `enum Distribution { Debian, Fedora, Arch, OpenSuse, Alpine, Autre }` ; `fn distribution(os_release: &str) -> Distribution` (`ID` puis `ID_LIKE`) ; `fn commande_installation(shell: &str, d: Distribution) -> Option<String>` (ex. `"sudo apt install fish"`, `None` si inconnu) ; `enum ModeInvite { Starship, Shell }` ; `struct Ressources { integration: Option<PathBuf>, starship: Option<PathBuf>, config_starship: PathBuf }` ; `struct Lancement { argv: Vec<String>, env: Vec<(String, String)>, avertissement: Option<String> }` ; `fn lancement(shell: &Path, mode: ModeInvite, r: &Ressources) -> Lancement`.

- [x] **Step 1 : Tests** (`#[cfg(test)]`, entrées en texte, aucun accès au système) : `detecte_etc_shells_et_path` (doublons `/bin/bash` et `/usr/bin/bash` → un seul si même fichier réel) ; `niveaux_conformes_a_la_conception` (bash/zsh/fish/nu → Complet ; elvish/xonsh/tcsh/ion/pwsh → InviteGeree ; `dash` → TelQuel ; `-bash` → Complet) ; `distribution_par_id_like` (`ID=linuxmint\nID_LIKE="ubuntu debian"` → Debian) ; `commande_installation_debian` (`fish` → `"sudo apt install fish"`, `nu` → `"sudo apt install nushell"`) ; `commande_inconnue_none` ; `lancement_bash_complet` (argv commence par le chemin de bash, contient `--rcfile` et le fichier `bash/pupitre.bash` ; env contient `STARSHIP_CONFIG` et `PUPITRE_INVITE=starship`) ; `lancement_mode_shell` (`PUPITRE_INVITE=shell`, pas de `STARSHIP_CONFIG`) ; `lancement_tel_quel` (argv = `[chemin]`, env vide) ; `integration_absente_tel_quel_avec_avertissement` ; `starship_absent_mode_shell_force` (`starship: None` + `ModeInvite::Starship` → `PUPITRE_INVITE=shell`, pas de `STARSHIP_CONFIG`, avertissement « Starship introuvable : l'invite habituelle du shell est gardée. Pour l'installer : sudo apt install starship »).
- [x] **Step 2 :** `cargo test -p pupitre-shell` → échec de compilation.
- [x] **Step 3 : Implémenter.** `lancement` ne vérifie que l'existence des fichiers ; le repli « shell absent » est fait par l'appelant (Task 6) qui passe le shell de connexion. Mécanismes par shell : ceux validés à la Task 0. Variables posées : `PUPITRE_INTEGRATION=<dossier>`, `PUPITRE_INVITE=starship|shell`, `PUPITRE_STARSHIP=<binaire>` si présent, `STARSHIP_CONFIG=<config_starship>` en mode Starship.
- [x] **Step 4 :** tests verts ; `cargo clippy -p pupitre-shell` → 0 avertissement.
- [x] **Step 5 : Commit** `feat(shell): détection des shells, niveaux et lancement`.

---

### Task 2 : Fichiers d'intégration — configuration de l'utilisateur, puis marques et dossier courant

**Files :** Create: `assets/shell/**` ; Test: `crates/shell/tests/integration_reelle.rs` (lance chaque shell installé dans un PTY via `portable-pty`, `HOME` temporaire ; un shell absent → test sauté avec un message).

**Marques attendues** (une de chaque par commande, `BEL` comme terminateur) : `ESC]133;A BEL` au début de l'invite, `ESC]133;B BEL` à la fin de l'invite, `ESC]133;C BEL` juste avant l'exécution, `ESC]133;D;<code> BEL` après (pas de D avant la toute première invite) ; `ESC]7;file://<hôte><dossier> BEL` à chaque invite.

- [x] **Step 1 : Tests** pour bash, zsh, fish, nushell : `<shell>_charge_la_config_utilisateur` (le fichier habituel de l'utilisateur dans le `HOME` temporaire pose `VU=1` ; `echo $VU` → `1`) ; `<shell>_marques_d_une_commande` (`true` puis `false` → séquence A B C D;0 A B C D;1 A B) ; `<shell>_dossier_courant` (`cd /tmp` → OSC 7 avec `/tmp`) ; `marques_uniques_avec_crochets_utilisateur` (bash et zsh : la config de l'utilisateur pose déjà un `PROMPT_COMMAND`/`precmd` qui affiche `X` et appelle `starship init`) ; `fish_sans_double_marque` ; `aucune_ecriture_dans_home` (liste des fichiers du `HOME` temporaire identique avant/après, hors fichiers d'historique propres au shell) ; `mode_shell_garde_l_invite_native` (`PUPITRE_INVITE=shell` : l'invite définie par l'utilisateur, `MONINVITE>`, est affichée, marques présentes). Niveau 2 (elvish, xonsh, tcsh selon Task 0) : `<shell>_invite_geree` (charge la config de l'utilisateur, invite Starship si présent).
- [x] **Step 2 :** échec (fichiers absents).
- [x] **Step 3 : Écrire les fichiers.** Chaque fichier : 1) charge la config habituelle de l'utilisateur (comme le shell l'aurait fait seul) ; 2) si `PUPITRE_INVITE=starship` et `PUPITRE_STARSHIP` exécutable : `starship init` avec ce binaire ; si `shell`, ou si le binaire a disparu entre-temps : rien, l'invite de l'utilisateur reste ; 3) pose les marques **après** tout le reste (pour qu'aucun crochet de l'utilisateur ne les écrase), sans les doubler si le shell les émet déjà (Task 0) ; 4) n'écrit rien dans `HOME`.
- [x] **Step 4 :** tests verts (`cargo test -p pupitre-shell --test integration_reelle`).
- [x] **Step 5 : Commit** `feat(shell): intégration des shells sans toucher aux fichiers de l'utilisateur`.

---

### Task 3 : Starship du système et invite par défaut

**Files :** Create: `crates/shell/src/starship.rs`, `assets/shell/starship.toml`.

**Interfaces :**
- Produces : `fn chercher_starship(env_pupitre: Option<&OsStr>, path: &OsStr) -> Option<PathBuf>` — ordre : `PUPITRE_STARSHIP` (pour les essais) ; `starship` du `PATH` ; sinon `None`. `const CONFIG_STARSHIP_DEFAUT: &str` (contenu de `assets/shell/starship.toml`, `include_str!`). `fn creer_config_starship_si_absente(chemin: &Path) -> io::Result<()>` (via `ecrire_atomique`, `$XDG_CONFIG_HOME/pupitre/starship.toml`).

**Invite par défaut** (reproduit l'invite actuelle du porteur, deux lignes) :
```
14:02:37 charge 0.42 git:main Δ●○
⌁⌁⌁utilisateur@machine ~/Projets/pupitre λ
```
Ligne 1 : heure (`bright-black`), charge 1 min lue dans `/proc/loadavg` (module `custom`, `bright-black`), branche git (`purple`) et état (`yellow` : `Δ` modifié, `●` indexé, `○` non suivi). Ligne 2 : marqueurs `⚙⚙⚙` (`red`) si `ENVIRONMENT=prod` et `⌁⌁⌁` (`cyan`) si `SSH_CONNECTION`, `utilisateur@machine` (`green`), dossier (`blue`), symbole : `≠` si `ENVIRONMENT=prod`, sinon `#` pour root, `»` en SSH, `λ` sinon ; symbole en `red` si la commande précédente a échoué. `command_timeout = 500`. Uniquement des noms de couleurs ANSI.

- [x] **Step 1 : Tests** : `chercher_starship_ordre` (dossiers temporaires avec faux exécutables) ; `config_defaut_sans_couleur_exacte` (aucun `#` suivi de 6 chiffres hexadécimaux dans `CONFIG_STARSHIP_DEFAUT`) ; `config_defaut_valide` (si un Starship est trouvé : `STARSHIP_CONFIG=<fichier> starship prompt` → code 0, contient `λ`, aucune ligne `[WARN]`) ; `config_existante_jamais_ecrasee` ; `starship_absent_invite_utilisateur` (bash et zsh : `PATH` sans Starship, config de l'utilisateur posant l'invite `MONINVITE>` → `MONINVITE>` affichée, marques présentes, avertissement unique) ; `starship_n_ouvre_pas_la_config_utilisateur` (si `strace` présent : `openat` de `~/.config/starship.toml` absent de la trace).
- [x] **Step 2 :** échec.
- [x] **Step 3 : Implémenter.** Rien n'est compilé ni copié : Pupitre utilise le `starship` du `PATH`. `config_defaut_valide` sert de garde-fou si une version future de Starship change le format (un `[WARN]` fait échouer le test).
- [x] **Step 4 :** tests verts.
- [x] **Step 5 : Commit** `feat(shell): invite Starship par défaut, invite de l'utilisateur sans Starship`.

---

### Task 4 : Protocole v3 — ouverture avec variables, marques de commandes, trousseau lent

**Files :** Modify: `crates/proto/src/lib.rs`, `crates/proto/src/ssh.rs`, `crates/ssh/src/trousseau.rs`, `crates/ssh/src/askpass.rs`, `crates/pupitre/src/demande.rs`, `crates/pupitre/src/client.rs`.

**Interfaces :**
- Produces : `VERSION = 3` ; `ClientMsg::Ouvrir { argv: Vec<String>, cwd: Option<PathBuf>, cols: u16, rows: u16, env: Vec<(String, String)> }` ; `ServerMsg::Marque { id: SessionId, marque: MarqueCommande, rejouee: bool }` ; `enum MarqueCommande { Invite, Saisie, Execution, Fin { code: Option<i32>, duree_ms: Option<u64> } }` (A, B, C, D) ; `EtatTrousseau::Lent` (en fin d'enum) ; `Coffre::etat()` rend `Lent` quand le délai est dépassé.

- [x] **Step 1 : Tests** : `encodage_v1_inchange` et octets v2 figés restent verts ; `nouvelles_variantes_v3_aller_retour` ; fond : `client_v2_ne_recoit_ni_marque_ni_lent` (`Lent` devient `Absent` pour un client v2) ; interface : `ouvrir_v3_ou_open_selon_le_fond` (faux fond v2 → `Open`, sans variables) ; ssh : `etat_lent_si_delai_depasse` (Coffre de test qui dort) ; demande : `trousseau_lent_texte` (texte « Le trousseau ne répond pas assez vite : la phrase ne sera pas mémorisée cette fois. », case décochée et inactive).
- [x] **Step 2–4 :** échec, implémentation, succès (`cargo test -p pupitre-proto -p pupitre-ssh -p pupitre-fond`, `scripts/test-x11.sh -p pupitre --test gui_ssh`). Lancer 60 s la cible de fuzzing `trame` : aucun plantage.
- [x] **Step 5 : Commit** `feat(proto): version 3 — ouverture avec variables, marques de commandes, trousseau lent`.

---

### Task 5 : Le fond lit les marques OSC 133

**Files :** Create: `crates/fond/src/marques.rs`, `crates/fond/fuzz/fuzz_targets/marques.rs` ; Modify: `crates/fond/src/serveur.rs`, `crates/fond/src/session.rs` (variables reçues), `crates/fond/fuzz/Cargo.toml`.

**Interfaces :**
- Produces : `struct LecteurMarques` ; `fn LecteurMarques::lire(&mut self, octets: &[u8], maintenant: Instant) -> Vec<Morceau>` avec `enum Morceau { Octets(Vec<u8>), Marque(MarqueCommande) }` — la concaténation des `Octets` rendus est **exactement** l'entrée ; une marque est rendue juste après les octets qui la terminent. `const OSC_MAX: usize = 4096`.

**Règles :** OSC reconnu = `ESC ] 133 ; <lettre> [; …] (BEL | ESC \)` ; au-delà de `OSC_MAX` octets sans terminateur, la séquence est abandonnée (octets relayés, aucune marque). Entre `C` et `D`, seules les marques `D` comptent. `D;<code>` : code entier, absent ou illisible → `None`. Durée = de `C` à `D` (`None` sans `C`).

- [x] **Step 1 : Tests** (`marques.rs`) : `commande_complete` ; `marque_coupee_en_deux` (coupure à chaque position possible → mêmes marques, mêmes octets) ; `terminateur_esc_backslash` ; `osc_demesure_abandonne` ; `marques_ignorees_pendant_une_commande` ; `code_illisible_none` ; `autres_osc_intacts` (OSC 0, 7, 8, 52 relayés sans marque). Serveur (`tests/serveur.rs`) : `marques_dans_l_ordre_du_flux` (session `printf 'a\e]133;A\ab'` → `Output("a…A")`, `Marque(Invite)`, `Output("b")` dans cet ordre) ; `ouvrir_avec_variables` (`Ouvrir { env: [("X","1")] }` → `echo $X` donne `1`) ; `relecture_rejoue_les_marques` (`Attach` → `Replay` et `Marque { rejouee: true }` dans l'ordre) ; `client_v2_flux_sans_marque`.
- [x] **Step 2–4 :** échec, implémentation, succès. Fuzzing `marques` 5 min : aucun plantage ; propriété vérifiée dans la cible : concaténation des octets = entrée.
- [x] **Step 5 : Commit** `feat(fond): marques de commandes OSC 133 transmises dans l'ordre du flux`.

---

### Task 6 : Choix du shell, menu « Nouvel onglet avec », `pupitre shells`

**Files :** Create: `crates/pupitre/src/shells.rs` ; Modify: `crates/config/src/fichier.rs`, `assets/config/pupitre.toml`, `crates/pupitre/src/fenetre.rs`, `crates/pupitre/src/main.rs`, `crates/pupitre/src/verification.rs` ; Test: `crates/pupitre/tests/gui_shells.rs`, `crates/config` (tests unitaires).

**Configuration** (commentée dans le fichier par défaut) : `shell = ""` (vide : shell de connexion ; sinon nom ou chemin, ex. `"fish"`) ; `invite = "starship"` (`"shell"` : garder l'invite du shell) ; `annonce_fin_commande = "longues"` (`"toutes"`, `"aucune"`) ; `[[shells]]` avec `nom` et `commande` (liste, ex. `["ssh", "serveur"]` ; premier élément non vide et sans `-` initial).

- [x] **Step 1 : Tests** : config : `shells_declares_lus`, `commande_vide_refusee` (ligne fautive), `valeurs_inconnues_refusees` (`invite = "autre"` → erreur avec la ligne). Graphiques : `menu_nouvel_onglet_avec` (sous-menu listant les shells installés puis les shells déclarés) ; `onglet_ouvert_avec_le_shell_choisi` (`/bin/sh` via le menu → `ps -p $$ -o comm=` donne `sh`) ; `shell_configure_absent_message` (`shell = "nexistepas"` → l'onglet s'ouvre avec le shell de connexion, message unique : « Le shell « nexistepas » est introuvable : le shell de connexion est utilisé. ») ; `fish_absent_commande_installation` (`shell = "fish"` avec un `PATH` sans fish → message contenant « sudo apt install fish »). CLI : `pupitre shells` liste chemin, niveau (« complet », « invite gérée », « tel quel ») et, pour les shells connus absents, la commande d'installation ; code 0.
- [x] **Step 2–4 :** échec, implémentation, succès (`scripts/test-x11.sh -p pupitre -p pupitre-config`). Les ouvertures d'onglet (nouvel onglet, menu, connexions SSH) passent toutes par `Lancement`, envoyé en `Ouvrir` (v3) ou en `Open` (fond v2).
- [x] **Step 5 : Commit** `feat(pupitre): choix du shell, onglet avec un autre shell, pupitre shells`.

---

### Task 7 : Annonce de fin de commande et navigation de commande en commande

**Files :** Create: `crates/pupitre/src/commandes.rs` ; Modify: `crates/pupitre/src/terminal.rs`, `crates/pupitre/src/fenetre.rs`, `crates/pupitre/src/raccourcis.rs` ; Test: `crates/pupitre/tests/gui_commandes.rs`.

**Interfaces :**
- Produces : `struct MarquesOnglet` (lignes VTE absolues des marques, bornée par l'historique : une ligne sortie de l'historique est oubliée) ; `Raccourci::CommandePrecedente` (Ctrl+Maj+↑) et `Raccourci::CommandeSuivante` (Ctrl+Maj+↓) ; `fn texte_annonce(code: Option<i32>, duree_ms: Option<u64>) -> String`.

**Textes :** fin réussie : « Commande terminée en 12 s. » ; échec : « Commande en échec, code 2, en 12 s. » ; sans durée : « Commande terminée. » / « Commande en échec, code 2. » ; durées : `s` sous 60 s, sinon « 2 min 5 s ». Navigation : défile pour mettre la ligne de l'invite en haut et annonce « Commande : » suivi du texte de la ligne (sans l'invite si la marque `B` donne la colonne), puis « en échec, code N » si elle a échoué ; au bout de la liste : « Aucune commande avant. » / « Aucune commande après. ».

**Règles d'annonce :** seulement pour l'onglet visible de la fenêtre active (les autres : étape 4) ; `longues` = durée ≥ 5 s ; jamais pour une marque rejouée ; priorité « polie » (n'interrompt pas Orca).

- [x] **Step 1 : Tests** : `texte_annonce_cas` (0/2/None × 3 s/125 s/None) ; graphiques (faux fond dans le processus, comme `gui_demarrage`) : `annonce_commande_longue` (marques C puis D;0 avec `duree_ms = 6000` → annonce « Commande terminée en 6 s. » sur l'objet accessible du terminal) ; `pas_d_annonce_commande_courte` ; `annonce_echec` ; `onglet_cache_sans_annonce` ; `marques_rejouees_sans_annonce` ; `navigation_precedente_suivante` (trois commandes, Ctrl+Maj+↑ deux fois → vue sur la 2e invite, annonce contenant son texte) ; `navigation_bout_de_liste` ; `raccourcis_sans_conflit` (Ctrl+Maj+↑/↓ ne sont pas envoyés au shell).
- [x] **Step 2–4 :** échec, implémentation, succès (`scripts/test-x11.sh -p pupitre --test gui_commandes`, puis Wayland).
- [x] **Step 5 : Commit** `feat(pupitre): annonce de fin de commande et navigation de commande en commande`.

---

### Task 7bis : Aide dans Pupitre, disponible hors ligne

Demandée par le porteur le 2026-11-01.

**Files :** Create: `crates/pupitre/src/aide.rs` ; Modify: `crates/pupitre/src/fenetre.rs` (menu, action `win.aide`), `crates/pupitre/src/raccourcis.rs` (`Raccourci::Aide`), `crates/pupitre/src/main.rs` (`pupitre aide` affiche l'aide complète), `assets/aide/fr.md`.

**Interfaces :**
- Produces : `aide de la langue active par `pupitre_i18n::aide()` (Task 0bis ; `assets/aide/<langue>.md`, **dans l'exécutable** : toujours disponible, sans réseau ni fichier installé) ; `struct Section { titre: String, niveau: u8, texte: String }` ; `fn sections(aide: &str) -> Vec<Section>` (découpe aux titres `#`, `##`, `###`) ; `fn ouvrir_aide(app: &gtk::Application, parent: Option<&gtk::Window>, section: Option<&str>)` (une seule fenêtre d'aide : la rouvrir la ramène au premier plan).

**Fenêtre « Aide de Pupitre »** : à gauche, la liste des sections (`GtkListBox`, chaque ligne rôle titre et niveau) ; à droite, le texte de la section choisie dans un `GtkTextView` en lecture seule, curseur visible (Orca le lit ligne par ligne et mot par mot). Mise en forme minimale : titres en gras, blocs de code en police du terminal, tableaux rendus en lignes « Action : raccourci » (un tableau Markdown n'est pas lisible tel quel par Orca). Champ « Rechercher dans l'aide » (filtre les sections contenant le texte). Échap ferme. Ctrl+Tab passe d'une zone à l'autre. Taille du texte : suit le zoom de la configuration de police. Couleurs : celles du thème de Pupitre (contraste vérifié).

**Raccourci : Ctrl+Maj+H** (et entrée de menu « Aide » en fin de menu). **F1 n'est pas pris** : comme F10, il reste aux programmes (aide de mc, htop, vim…).

- [x] **Step 1 : Tests** : `sections_decoupees` (titres de `AIDE` dans l'ordre, aucun texte perdu : la concaténation des textes et des titres redonne tout le contenu hors marques `#`) ; `tableau_lisible` (une ligne `| Nouvel onglet | Ctrl+Maj+T |` devient « Nouvel onglet : Ctrl+Maj+T ») ; `raccourcis_de_l_aide_conformes` (chaque raccourci cité dans la section 2 de `AIDE` est reconnu par `raccourci()` avec l'action annoncée — l'aide ne peut pas mentir sur les raccourcis) ; `raccourci_aide` (Ctrl+Maj+H → `Raccourci::Aide` ; F1 → `None`). Graphiques : `aide_ouverte_par_le_menu` (fenêtre « Aide de Pupitre », rattachée à l'application, liste non vide, focus sur la liste) ; `aide_unique` (deux ouvertures → une seule fenêtre) ; `aide_echap_ferme`. CLI : `pupitre aide` → code 0, contient « Raccourcis clavier ».
- [x] **Step 2 :** échec.
- [x] **Step 3 : Implémenter.** Pas de dépendance Markdown : le découpage et la mise en forme minimale ci-dessus suffisent pour ce fichier, écrit par nous.
- [x] **Step 4 :** tests verts ; Orca automatisé (Orca muet) : vérification 19 — Ctrl+Maj+H → Orca dit « Aide de Pupitre » puis le nom de la première section.
- [x] **Step 5 : Commit** `feat(pupitre): aide intégrée (Ctrl+Maj+H), disponible hors ligne`.

---

### Task 7ter : Menu « Préférences »

Demandé par le porteur le 2026-11-01 (la fenêtre de réglages complète est à l'étape 4).

**Files :** Create: `crates/pupitre/src/preferences.rs` ; Modify: `crates/pupitre/src/fenetre.rs` (sous-menu, actions), `assets/aide/fr.md`.

**Interfaces :**
- Consumes : `chemin_config()`, `creer_defaut_si_absent` (config) ; `creer_config_starship_si_absente` (Task 3) ; `MessagesFenetre` (bandeau).
- Produces : `enum FichierPrefs { Configuration, Invite, Connexions }` ; `fn chemin(f: FichierPrefs) -> PathBuf` ; `fn ouvrir(f: FichierPrefs, parent: &gtk::Window, signaler: impl Fn(String) + 'static)`.

**Menu** (section avant « Aide ») : sous-menu « Préférences » avec « Modifier la configuration… » (`pupitre.toml`), « Modifier l'invite… » (`starship.toml`), « Modifier les connexions… » (`connexions.toml`). Le fichier est créé s'il manque (fichier par défaut commenté ; `connexions.toml` vide commenté), puis ouvert dans l'éditeur de texte par défaut via `gtk::FileLauncher` (portail sous Wayland, sans dépendance supplémentaire). Message après ouverture : « Les modifications s'appliquent dès l'enregistrement. » Échec (aucun éditeur, portail absent) : message persistant donnant le chemin complet du fichier, à ouvrir soi-même ; jamais de plantage.

- [x] **Step 1 : Tests** : `chemins_preferences` (`XDG_CONFIG_HOME` temporaire → trois chemins sous `pupitre/`) ; `fichier_cree_si_absent` (aucun fichier → créé avec le contenu par défaut ; existant → inchangé, octet pour octet) ; graphiques : `menu_preferences` (sous-menu présent avec ses trois entrées, accessibles au clavier) ; `ouverture_impossible_message` (lanceur en échec simulé → message contenant le chemin du fichier, annoncé).
- [x] **Step 2 :** échec.
- [x] **Step 3 : Implémenter.** L'aide (section 11) décrit le menu.
- [x] **Step 4 :** tests verts.
- [x] **Step 5 : Commit** `feat(pupitre): menu Préférences ouvrant les fichiers de configuration`.

---

### Task 8 : Orca, isolement, VM

**Files :** Modify: `tests/a11y/orca/scenario.py`, `tests/a11y/orca/attendu/*.json`, `docs/orca-liste-de-controle.md`, `tests/isolement/verifier.py`, `tests/vm/commun.sh`.

- [x] **Step 1 : Orca (automatisé, Orca muet)** : vérification 17 — `sleep 6` dans un onglet bash intégré → Orca dit « Commande terminée en 6 s. » ; vérification 18 — après `echo un`, `echo deux`, Ctrl+Maj+↑ → Orca dit « Commande : echo deux », encore → « Commande : echo un ». 19/19 trois fois de suite.
- [x] **Step 2 : Isolement** : `starship` ajouté aux programmes jugés ; session type avec invite Starship dans un dépôt git → aucune connexion réseau, aucune écriture hors des dossiers de Pupitre, `~/.config/starship.toml` jamais ouvert.
- [x] **Step 3 : VM** : `tests/vm/commun.sh` gagne deux essais — « invite Starship affichée » (texte du terminal lu par AT-SPI contient `λ`) et « fin de commande annoncée » (événement d'annonce AT-SPI reçu après `sleep 6`). `scripts/vm.sh sway|gnome|kde essayer` → 15/15 chacun.
- [x] **Step 4 : Commit** `test(a11y): annonces et navigation des commandes, isolement de Starship, essais en VM`.

---

### Task 9 : Vérification de fin d'étape

- [x] **Step 1 :** `cargo clippy --workspace --all-targets -- -D warnings` → 0 ; `cargo fmt --check`.
- [x] **Step 2 :** `scripts/test-x11.sh` et `scripts/test-wayland.sh` → tout passe (machine peu chargée ; relancer une fois un échec du trousseau et le signaler).
- [x] **Step 3 :** fuzzing `trame` et `marques`, 5 min chacun → aucun plantage.
- [x] **Step 4 :** `scripts/verifier-isolement.sh` → conforme ; `scripts/test-orca.sh` → 18/18 ; VM Sway, GNOME, KDE → 15/15.
- [x] **Step 5 : Essai sur le poste du porteur** (2 minutes, rien à écouter) : ouvrir Pupitre, vérifier que l'invite ressemble à l'actuelle ; `sleep 6` → (Orca, s'il est lancé, annonce la fin) ; menu ▸ « Nouvel onglet avec » ▸ fish ; `pupitre shells` dans un terminal ; Ctrl+Maj+H ouvre l'aide ; menu ▸ Préférences ▸ « Modifier la configuration… » ouvre `pupitre.toml` dans l'éditeur.
- [x] **Step 6 :** consigner dans `docs/verifications/etape-3.md`, cocher le plan, mettre à jour `suivi-tokens.md`, commit `docs: vérifications de l'étape 3`.

---

## Choix du concepteur (à valider par le porteur)

1. **[Validé le 2026-11-01]** **Invite par défaut = votre invite actuelle sur deux lignes** (heure, charge, branche git et état ; puis marqueurs, `utilisateur@machine`, dossier, symbole `λ`), et non le `$` écrit dans la conception. Les couleurs viennent du thème de Pupitre.
2. **[Validé le 2026-11-01 ; à expliquer dans l'aide]** **Annonce de fin de commande seulement pour les commandes de 5 s ou plus** (`annonce_fin_commande = "longues"`), réussies ou non ; réglable en `"toutes"` ou `"aucune"`. Pour une commande courte, Orca lit déjà la sortie.
3. **[Validé le 2026-11-01 ; flèches seules écartées : elles restent au shell et aux programmes]** **Raccourcis de navigation : Ctrl+Maj+↑ et Ctrl+Maj+↓** (commande précédente / suivante).
4. **Starship du système** (1.22.1 sous Debian 13), dépendance recommandée du futur paquet, rien de compilé ni d'embarqué ; **sans Starship, chaque shell garde l'invite habituelle de l'utilisateur** (décision du porteur, 2026-11-01), avec un avertissement unique donnant la commande d'installation.
5. **[Validé le 2026-11-01]** **Fichier d'invite modifiable** : `~/.config/pupitre/starship.toml`, créé au premier lancement à partir de l'invite par défaut ; jamais écrasé ensuite.
6. **[Validé le 2026-11-01]** **Shells absents : commande d'installation affichée** dans `pupitre shells` et dans le message quand le shell configuré manque ; le menu « Nouvel onglet avec » ne liste que les shells installés et ceux que vous déclarez.
7. **[Validé le 2026-11-01 ; message : « Le trousseau ne répond pas pour l'instant : la phrase ne peut pas être mémorisée cette fois. »]** **Protocole v3 en profite pour corriger un reste de l'étape 2** : un trousseau trop lent n'est plus présenté comme « Aucun trousseau disponible ».
