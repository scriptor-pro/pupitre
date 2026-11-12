# Pupitre — Étape 4 : le reste de la version 1 — Plan d'implémentation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal :** Pupitre prévient sans déranger (✓/✗ et ● sur les onglets, bulles du bureau, copie par un programme signalée), montre et annonce la disposition du clavier, offre deux thèmes à contraste élevé, d'autres polices, des raccourcis modifiables, une fenêtre de réglages qui garde les commentaires de `pupitre.toml`, et dit sa version (« À propos », `pupitre -v`).

**Architecture :** le processus de fond, qui lit déjà les OSC 133 dans le flux, reconnaît aussi l'écriture du presse-papiers (OSC 52) et les demandes de notification (OSC 9, OSC 777) et les transmet dans l'ordre du flux (protocole v4) ; les octets restent relayés tels quels à VTE. L'interface décide de ce qu'elle montre (indicateurs d'onglet, bandeau de messages, bulles `GNotification`) selon une section `[notifications]` de la configuration. La fenêtre de réglages n'a pas d'état propre : elle modifie `pupitre.toml` avec `toml_edit` (commentaires gardés), puis le rechargement à chaud existant applique la nouvelle configuration.

**Tech Stack :** Rust, GTK 4.18 / VTE 0.80 (existants), `toml_edit` (déjà dans `Cargo.lock`, ajouté comme dépendance directe de `pupitre-config`), `gio::Notification` (bulles locales, sans dépendance), `gdk::Device::layout_names()` / `active_layout_index()` (GTK 4.18), polices officielles IBM Plex Mono, Nordwand Mono, Ioskeley Mono (SIL OFL 1.1).

**Spec :** `docs/conception/2026-10-25-pupitre-conception.md`, §4.2 (OSC 52 et 9/777 interceptés par le fond), §5 (bandeau de disposition clavier), §7.1 (polices), §7.2 (thèmes à contraste élevé, cible 7:1), §9 (notifications), §11 (vie privée : OSC 52 lecture bloquée, écriture autorisée avec message ; rien de téléchargé), §12 (fenêtre de réglages sur le même fichier), §13 (robustesse), §14 (OSC 9/777 pour les agents), §15 (étape 4). Demandes du porteur du 2026-11-12 : élément de menu « À propos », `pupitre -v`, bouton « + » (conception §2).

**Déjà fait :** paquet `.deb` (`scripts/construire-deb.sh`, avancé le 2026-11-12) ; cette étape ne fait que le tenir à jour (nouvelles polices et licences).

**Hors étape 4 :** traductions (étape 5) ; indication de mode par onglet (root, SSH… : version ultérieure, conception §2) ; suivi automatique du contraste élevé du bureau (GTK 4.18 ne l'expose pas : choix 8).

## Global Constraints

- Tout ce qui figure dans les Global Constraints des étapes 1 à 3 reste valable (X11 et Wayland, GTK ≥ 4.18, VTE ≥ 0.80, SPDX `GPL-3.0-or-later`, pas de `unwrap`/`expect`/`panic!` hors tests, `ecrire_atomique`, jamais de secret en clair, aucun texte affiché écrit en dur : tout passe par `pupitre-i18n`, texte français dans `crates/i18n/langues/fr/pupitre.ftl`, aide à jour dans `assets/aide/fr.md` avec retrait des « (à venir) » livrés, `suivi-tokens.md` à jour).
- **Notifications (§9)** : tout silencieux (aucun son, jamais) ; chaque case réglable ; jamais un seul canal (une bulle est toujours doublée d'un indicateur d'onglet ou d'un message dans la fenêtre) ; au plus 3 flashs par seconde ; bulles par les notifications de GIO (locales), le mode « Ne pas déranger » est celui du bureau (Pupitre ne le contourne jamais : pas d'urgence « critique »).
- **OSC 52** : lecture (`?`) jamais servie ; écriture autorisée, toujours signalée par « Un programme a copié N caractères » (3 s), désactivable.
- **Rien n'est téléchargé à l'exécution** : les polices sont embarquées dans le paquet ; une police installée sur le système est utilisable par son nom.
- **Fichiers de l'utilisateur** : la fenêtre de réglages ne modifie que `pupitre.toml`, en gardant ses commentaires, l'ordre des clés et les clés qu'elle ne connaît pas ; écriture atomique avec sauvegarde (`pupitre.toml.bak`).
- **Protocole v4** rétrocompatible : variantes nouvelles **ajoutées en fin d'enum**, jamais envoyées à un client v1 à v3 (`adapter_pour`) ; une interface v4 face à un fond v3 fonctionne sans copie OSC 52 ni notification de programme.
- Les raccourcis restent **indépendants de la disposition** (reconnus par la touche physique comme aujourd'hui) : la disposition du porteur est fr dvorak.

## Review Focus

1. **OSC 52 démesuré, base64 invalide ou coupé entre deux lectures du PTY** (`cat` d'un fichier binaire, programme qui copie 50 Mio) → ni plantage ni blocage, mémoire bornée, octets relayés intacts, rien copié au-delà de la borne. Test : Task 1 `osc52_coupe_en_deux`, `osc52_demesure_ignore`, `osc52_base64_invalide_ignore`, fuzzing `marques`.
2. **Programme qui envoie des notifications en rafale** (OSC 9 dans une boucle) → l'interface reste fluide, au plus une bulle par onglet toutes les 10 s, l'indicateur ● reste. Test : Task 2 `notifications_en_rafale_bornees`.
3. **`pupitre.toml` modifié à la main avec des commentaires, des clés dans un autre ordre, ou une erreur de syntaxe**, puis un réglage changé dans la fenêtre → commentaires et ordre gardés ; fichier invalide : aucune écriture, message clair. Test : Task 8 `reglage_garde_les_commentaires`, `fichier_invalide_pas_d_ecriture`.
4. **Raccourci configuré invalide ou en conflit** (`nouvel_onglet = "Ctrl+Truc"`, deux actions sur la même touche) → raccourci par défaut gardé pour l'action fautive, message avec la ligne, les autres raccourcis marchent ; en dvorak, un raccourci configuré suit la touche physique comme les autres. Test : Task 6 `raccourci_invalide_garde_le_defaut`, `conflit_signale`.
5. **Changement de disposition du clavier avec plusieurs fenêtres ouvertes ou le bandeau masqué** → une seule annonce (dans la fenêtre active), même masqué. Test : Task 3 `une_annonce_par_changement`.

---

## Structure des fichiers

```
crates/proto/src/lib.rs                 VERSION 4, ServerMsg::PressePapiers, ServerMsg::Notification
crates/fond/src/marques.rs              Morceau::PressePapiers / Notification, bornes OSC 52
crates/fond/src/serveur.rs              transmission dans l'ordre, adapter_pour v4
crates/config/src/fichier.rs            [notifications], bandeau_clavier, [raccourcis], ThemeChoix contraste
crates/config/src/theme.rs              theme_contraste_sombre/clair, minimum 7:1
crates/config/src/edition.rs            modifier_cle (toml_edit, commentaires gardés)
crates/pupitre/src/notifications.rs     indicateurs d'onglet, bulles, limite de débit
crates/pupitre/src/clavier.rs           bandeau de disposition, annonces
crates/pupitre/src/raccourcis.rs        raccourcis configurables
crates/pupitre/src/reglages.rs          fenêtre de réglages
crates/pupitre/src/apropos.rs           « À propos », version
crates/pupitre/src/main.rs              pupitre -v / --version, pupitre polices
assets/fonts/                           IBM Plex Mono, Nordwand Mono, Ioskeley Mono + OFL
scripts/construire-deb.sh               copyright des nouvelles polices
tests/a11y/orca/scenario.py             vérifications 22 à 26
docs/verifications/etape-4.md
```

---

### Task 0 : Levée de risque (jetable)

**Files :** Create: `essai-jetable/etape4/` (scripts et `RESULTATS.md`), rien de livré.

- [ ] **Step 1 : OSC 9 et 777 réels.** Relever les octets émis par Claude Code (fin de tâche, attente de réponse), par `notify-send`-like de `printf '\e]9;texte\a'` et `printf '\e]777;notify;titre;corps\a'` ; noter les variantes rencontrées (OSC 9 de ConEmu `9;4;…` = barre de progression, à **ignorer**).
- [ ] **Step 2 : Bulles.** Sous Xfce (poste du porteur, xfce4-notifyd) et en VM GNOME/KDE : une `gio::Notification` envoyée par `fr.pupitre.Pupitre` s'affiche-t-elle ? Le clic active-t-il l'action `app.montrer-onglet` avec son paramètre ? Le mode « Ne pas déranger » la retient-il ?
- [ ] **Step 3 : Disposition.** Sous X11 (poste du porteur) et Weston : valeurs de `layout_names()` et `active_layout_index()` pour fr dvorak + us ; le signal `notify::active-layout-index` arrive-t-il au changement ? (Xvfb ignore les changements de disposition : connu.)
- [ ] **Step 4 : `toml_edit`.** Aller-retour de `assets/config/pupitre.toml` et d'un fichier annoté à la main : modifier `theme` garde-t-il octet pour octet le reste ?
- [ ] **Step 5 : Consigner** dans `essai-jetable/etape4/RESULTATS.md`. **Critères :** bulles visibles sous Xfce et GNOME ; disposition lisible sous X11 ; aller-retour `toml_edit` sans perte. Si les bulles ne marchent pas sous Xfce : arrêter et en parler au porteur.

---

### Task 1 : Le fond reconnaît OSC 52 (écriture), OSC 9 et OSC 777 — protocole v4

**Files :** Modify: `crates/proto/src/lib.rs`, `crates/fond/src/marques.rs`, `crates/fond/src/serveur.rs`, `crates/fond/fuzz/fuzz_targets/marques.rs`, `crates/pupitre/src/client.rs` (négociation v4→v3).

**Interfaces :**
- Produces : `VERSION = 4` ; `ServerMsg::PressePapiers { id: SessionId, texte: String }` et `ServerMsg::Notification { id: SessionId, titre: Option<String>, corps: String }`, en fin d'enum, `est_v4()` ; `Morceau::PressePapiers(String)`, `Morceau::Notification { titre: Option<String>, corps: String }` ; `const PRESSE_PAPIERS_MAX: usize = 1 << 20` (octets décodés) ; `const NOTIFICATION_MAX: usize = 1024` (caractères, au-delà tronqué avec « … »).

**Règles :** OSC 52 : `52;<cibles>;<base64>` avec cibles contenant `c` ou vide → `PressePapiers` (texte UTF-8 ; base64 ou UTF-8 invalide → ignoré) ; `52;…;?` (lecture) → rien. OSC 9 : `9;<texte>` → `Notification { titre: None }`, sauf `9;4;…` (progression ConEmu) ignoré. OSC 777 : `777;notify;<titre>;<corps>` → `Notification { titre: Some }`. Séquence plus longue que `PRESSE_PAPIERS_MAX` en base64 (4/3) : suivie sans être copiée (mémoire bornée), puis ignorée. Caractères de contrôle retirés du titre et du corps. Même limite de débit que les marques (`LimiteMarques`).

- [ ] **Step 1 : Tests** (`marques.rs`) : `osc52_copie` (`\e]52;c;QUJD\a` → `PressePapiers("ABC")`, octets intacts) ; `osc52_lecture_ignoree` ; `osc52_coupe_en_deux` ; `osc52_demesure_ignore` (2 Mio → rien, mémoire du lecteur < 2 × `OSC_MAX` après) ; `osc52_base64_invalide_ignore` ; `osc9_notification` ; `osc9_progression_ignoree` ; `osc777_notification` ; `controles_retires` ; serveur : `ordre_octets_puis_presse_papiers`, `adapter_pour_v3_retire_v4` ; client : `fond_v3_negocie_v3`. Fuzzing : la cible `marques` couvre les nouveaux préfixes.
- [ ] **Step 2 :** `cargo test -p pupitre-proto -p pupitre-fond` → échec.
- [ ] **Step 3 : Implémenter** dans l'automate existant (préfixes `133;`, `52;`, `9;`, `777;notify;`) : seule la charge OSC 52 est gardée au-delà de `OSC_MAX`, jusqu'à la borne ; base64 décodé sans dépendance nouvelle (fonction locale, testée).
- [ ] **Step 4 :** tests verts ; fuzzing `marques` 5 min sans plantage.
- [ ] **Step 5 : Commit** `feat(fond): presse-papiers (OSC 52) et notifications (OSC 9/777) transmis — protocole v4`.

---

### Task 2 : Notifications dans l'interface

**Files :** Create: `crates/pupitre/src/notifications.rs` ; Modify: `crates/config/src/fichier.rs`, `assets/config/pupitre.toml`, `crates/pupitre/src/fenetre.rs`, `crates/pupitre/src/main.rs` (action `app.montrer-onglet`), `assets/aide/fr.md` ; Test: `crates/pupitre/tests/gui_notifications.rs`, `crates/config` (unitaires).

**Interfaces :**
- Consumes : `ServerMsg::PressePapiers`, `ServerMsg::Notification` (Task 1) ; `connecter_fin_commande` (existant) ; `BandeauMessages::afficher(texte, Some(durée))`.
- Produces : `struct ReglagesNotifications { fin_commande_onglet: bool, fin_commande_bulle: bool, programme_onglet: bool, programme_bulle: bool, presse_papiers: bool, cloche_onglet: bool }` (champ `notifications` de `Config`, section `[notifications]`, tout à `true` par défaut) ; `enum Indicateur { Aucun, Reussi, Echec, Signale }` porté par `Onglet` (remplace le booléen `signale`) ; `fn bulle_permise(dernier: Option<Instant>, maintenant: Instant) -> bool` (intervalle `INTERVALLE_BULLES = 10 s` par onglet).

**Comportement (§9) :**

| Événement | Onglet (si non affiché) | Fenêtre | Bulle |
|---|---|---|---|
| Commande finie (marque D) | ✓ (code 0) ou ✗ | — | si aucune fenêtre de Pupitre n'est active **et** durée > 10 s |
| Notification d'un programme | ● | — | oui (titre : `titre` ou nom de l'onglet ; corps) |
| Cloche | ● (existant) | — | non |
| Copie par un programme | — | « Un programme a copié N caractères. » (3 s) | non |

L'indicateur s'efface quand l'onglet devient actif ; son texte accessible suit le modèle de la cloche : « titre, commande réussie », « titre, commande en échec », « titre, notification ». Clic sur la bulle → la fenêtre et l'onglet sont montrés (`app.montrer-onglet`, paramètre : numéro de session). `presse_papiers = false` : rien n'est copié ni affiché.

- [ ] **Step 1 : Tests** : config : `notifications_par_defaut`, `notifications_lues`, `cle_inconnue_refusee` ; unitaires : `bulle_permise_intervalle` ; graphiques (faux fond dans le processus, comme `gui_commandes`) : `fin_commande_onglet_cache_coche`, `echec_onglet_cache_croix`, `indicateur_efface_a_l_activation`, `nom_accessible_avec_indicateur`, `notification_programme_point`, `copie_programme_message` (texte dans le presse-papiers, message « Un programme a copié 3 caractères. »), `copie_desactivee`, `notifications_en_rafale_bornees` (100 OSC 9 en 1 s → interface réactive, une seule bulle demandée), `bulle_seulement_hors_premier_plan` (une fausse fonction d'envoi compte les bulles).
- [ ] **Step 2 :** échec.
- [ ] **Step 3 : Implémenter.** L'envoi de bulle passe par une fonction remplaçable (tests) ; la vraie appelle `gio::Application::send_notification` avec l'identifiant `onglet-<session>` (une nouvelle bulle du même onglet remplace la précédente).
- [ ] **Step 4 :** tests verts sous X11 et Wayland.
- [ ] **Step 5 : Commit** `feat(pupitre): notifications — ✓/✗ et ● sur les onglets, bulles du bureau, copie signalée`.

---

### Task 3 : Bandeau de disposition clavier

**Files :** Create: `crates/pupitre/src/clavier.rs` ; Modify: `crates/config/src/fichier.rs` (`bandeau_clavier`), `assets/config/pupitre.toml`, `crates/pupitre/src/fenetre.rs` (entre onglets et terminal), `assets/aide/fr.md` ; Test: `crates/pupitre/tests/gui_clavier.rs` (existant), unitaires dans `clavier.rs`.

**Interfaces :**
- Produces : `enum BandeauClavier { Nom, Code, Masque }` (réglage `bandeau_clavier = "nom" | "code" | "masque"`, défaut `"nom"`) ; `fn texte_disposition(noms: &[String], actif: i32, mode: BandeauClavier) -> Option<String>` (nom complet tel que donné par GTK, ex. « French (Dvorak) » ; code = mot avant la parenthèse abrégé par la table `CODES` [`French`→`fr`, `English`→`en`, …], sinon le nom) ; `struct SuiviDisposition` (un seul par application : écoute `notify::active-layout-index` du clavier du siège par défaut, annonce dans la fenêtre active, met à jour le bandeau de chaque fenêtre).

**Bandeau :** une ligne de texte non focalisable (indicateur seul, aucune action au clic), rôle « status », contraste du thème ; annonce « Disposition du clavier : French (Dvorak) » à chaque changement, y compris quand le bandeau est masqué ; aucune annonce au démarrage. Disposition illisible (index -1, liste vide) : bandeau caché, aucune erreur.

- [ ] **Step 1 : Tests** : `texte_disposition_cas` (nom, code, masque, index hors bornes → `None`) ; `config_bandeau_clavier` (valeurs, défaut, valeur inconnue refusée avec la ligne) ; graphiques : `bandeau_place_et_texte` (entre la barre d'onglets et le terminal, texte non vide sous Xvfb), `bandeau_masque`, `une_annonce_par_changement` (changement simulé via `SuiviDisposition::changer_pour_tests`, deux fenêtres → une annonce), `bandeau_non_focalisable` (Ctrl+Tab ne s'y arrête pas).
- [ ] **Step 2 :** échec. **Step 3 :** implémenter. **Step 4 :** tests verts (X11, Wayland).
- [ ] **Step 5 : Commit** `feat(pupitre): bandeau de disposition clavier, changements annoncés`.

---

### Task 4 : Thèmes à contraste élevé

**Point d'arrêt :** les valeurs ci-dessous sont des propositions (choix 1) ; avant d'écrire le code, produire `maquettes/theme-contraste-sombre.png` et `maquettes/theme-contraste-clair.png` (même gabarit que `maquettes/theme-clair.png`) et attendre la validation du porteur. Les valeurs validées remplacent celles-ci dans ce plan et dans la conception §7.2.

**Files :** Modify: `crates/config/src/theme.rs`, `crates/config/src/fichier.rs` (`ThemeChoix::ContrasteSombre`, `ThemeChoix::ContrasteClair` ; valeurs `"contraste-sombre"`, `"contraste-clair"`), `crates/config/src/theme_fichier.rs` (clé facultative `minimum`), `assets/config/pupitre.toml`, `assets/aide/fr.md`, `docs/conception/2026-10-25-pupitre-conception.md` §7.2.

**Interfaces :**
- Produces : `pub fn theme_contraste_sombre() -> Theme`, `pub fn theme_contraste_clair() -> Theme` ; champ `Theme::minimum: f64` (4,5 pour les thèmes existants et par défaut, 7,0 pour les deux nouveaux ; le curseur reste à 3,0) ; `check_theme` utilise `t.minimum`.

**Propositions (à valider) :** contraste sombre : fond `#000000`, texte `#ffffff`, 0 `#000000` (fond), 1/9 `#ff8080`, 2/10 `#7ee787`, 3/11 `#ffd75f`, 4/12 `#8cb4ff`, 5/13 `#ff9cf5`, 6/14 `#5fe0e0`, 7 `#e0e0e0`, 8 `#b0b0b0` (texte + fond), 15 `#ffffff`, sélection `#ffffff` sur `#000080`… ; contraste clair : fond `#ffffff`, texte `#000000`, couleurs 1–6 assombries jusqu'à ≥ 7:1 sur le fond et sur les panneaux 7. Chaque valeur est ajustée jusqu'à ce que `check_theme` ne signale rien à 7:1.

- [ ] **Step 1 : Maquettes et validation** (voir le point d'arrêt).
- [ ] **Step 2 : Tests** : `themes_contraste_conformes` (`check_theme` vide pour les deux, `minimum == 7.0`) ; `themes_existants_inchanges` (sombre et clair : valeurs et minimum 4,5 identiques) ; `theme_fichier_minimum` (`minimum = 7` lu ; absent → 4,5 ; < 4,5 refusé) ; `config_contraste` (`theme = "contraste-sombre"` lu).
- [ ] **Step 3 :** échec. **Step 4 :** implémenter. **Step 5 :** tests verts ; `pupitre check` sur chaque thème intégré → aucun problème.
- [ ] **Step 6 : Commit** `feat(config): thèmes à contraste élevé sombre et clair (7:1)`.

---

### Task 5 : Autres polices et `pupitre polices`

**Files :** Create: `assets/fonts/IBMPlexMono-*.ttf`, `NordwandMono-*.ttf`, `IoskeleyMono-*.ttf` (Regular, Bold, Italic, BoldItalic), `assets/fonts/OFL-IBMPlexMono.txt`, `OFL-NordwandMono.txt`, `OFL-IoskeleyMono.txt` ; Modify: `crates/pupitre/src/polices.rs`, `crates/pupitre/src/main.rs`, `assets/config/pupitre.toml` (commentaire de `police`), `scripts/construire-deb.sh` (copyright), `assets/aide/fr.md`.

**Sources (versions officielles des dépôts, conception §7.1)** : IBM Plex Mono (github.com/IBM/plex, dernière version publiée), Nordwand Mono (github.com/tywr/Nordwand-Mono), Ioskeley Mono (dépôt officiel). Noter dans le commit la version et l'adresse de chaque archive, et la somme SHA-256 de chaque fichier.

**Interfaces :**
- Produces : `fn polices_chasse_fixe() -> Vec<String>` (familles à chasse fixe vues par Pango, triées, sans doublon) ; `pupitre polices` : une famille par ligne, les embarquées marquées « (fournie avec Pupitre) », code 0 ; police demandée introuvable au rechargement : fontconfig est relu une fois (`FcConfigBringUptoDate`) avant le repli sur B612 Mono (une police installée pendant que Pupitre tourne est prise sans redémarrer).

- [ ] **Step 1 : Tests** : `polices_embarquees_toutes_presentes` (cinq familles enregistrées, chacune avec ses quatre styles et son texte OFL) ; `pupitre_polices_liste` (contient « B612 Mono (fournie avec Pupitre) » et « IBM Plex Mono (fournie avec Pupitre) », code 0) ; `nouvelle_police_sans_redemarrer` (police copiée dans le `~/.local/share/fonts` de test après le démarrage, `police = "<elle> 11"` → appliquée, pas de repli) ; `.deb` : `construire-deb.sh` liste les trois nouvelles licences dans `copyright` (lintian sans erreur).
- [ ] **Step 2 :** échec. **Step 3 :** télécharger les polices officielles (à la construction seulement), implémenter. **Step 4 :** tests verts.
- [ ] **Step 5 : Commit** `feat: polices IBM Plex Mono, Nordwand Mono, Ioskeley Mono ; pupitre polices`.

---

### Task 6 : Raccourcis modifiables

**Files :** Modify: `crates/config/src/fichier.rs` (`[raccourcis]`), `assets/config/pupitre.toml`, `crates/pupitre/src/raccourcis.rs`, `crates/pupitre/src/fenetre.rs`, `assets/aide/fr.md` (section 2 : « Changer un raccourci ») ; Test: unitaires de `raccourcis.rs`, `crates/pupitre/tests/gui_clavier.rs`.

**Interfaces :**
- Produces : section `[raccourcis]` : une clé par action, valeur au format de l'aide (`"Ctrl+Maj+T"`, `"Alt+1"`, `"F11"` ; modificateurs `Ctrl`, `Maj`, `Alt`, `Super`), `""` = action sans raccourci ; noms des clés : `nouvel_onglet`, `fermer_onglet`, `nouvelle_fenetre`, `connexions`, `copier`, `coller`, `rechercher`, `onglet_suivant`, `onglet_precedent`, `plein_ecran`, `zoom_plus`, `zoom_moins`, `zoom_normal`, `commande_precedente`, `commande_suivante`, `aide` (la Task 8 ajoute `reglages`) ; `struct TableRaccourcis` (défauts + remplacements) ; `fn analyser_raccourci(texte: &str) -> Result<Combinaison, ErreurRaccourci>` ; `fn raccourci_dans(table: &TableRaccourcis, touche, code, mods) -> Option<Raccourci>` (même reconnaissance par touche physique que `raccourci()` aujourd'hui).

**Règles :** valeur invalide → défaut gardé pour cette action, message persistant avec la ligne ; deux actions sur la même combinaison → les deux gardent leur défaut, message nommant les deux ; Ctrl+Tab, Ctrl+Maj+Tab (sortir du terminal), Alt+1…9 et Ctrl+PgSuiv/PgPréc ne sont pas modifiables (accessibilité : toujours disponibles) ; Tab, F1, F10 ne peuvent pas être pris.

- [ ] **Step 1 : Tests** : `analyser_cas` (« Ctrl+Maj+T », « maj+ctrl+t », « F11 », « Ctrl+Truc » → erreur) ; `raccourci_remplace` ; `raccourci_vide_desactive` ; `raccourci_invalide_garde_le_defaut` ; `conflit_signale` ; `touches_reservees_refusees` (Tab, F1, F10, Ctrl+Tab) ; `dvorak_touche_physique` (combinaison configurée reconnue par son code matériel) ; `raccourcis_de_l_aide_conformes` (existant) toujours vert.
- [ ] **Step 2 :** échec. **Step 3 :** implémenter (le menu affiche les raccourcis en vigueur). **Step 4 :** tests verts.
- [ ] **Step 5 : Commit** `feat(pupitre): raccourcis modifiables dans pupitre.toml`.

---

### Task 7 : « À propos », `pupitre -v`, bouton « + »

**Files :** Create: `crates/pupitre/src/apropos.rs` ; Modify: `Cargo.toml` (`repository = "https://github.com/scriptor-pro/pupitre"` dans `[workspace.package]`, hérité par les crates), `crates/pupitre/src/main.rs`, `crates/pupitre/src/fenetre.rs` (menu, bouton), `assets/aide/fr.md`.

**Interfaces :**
- Produces : `const VERSION: &str = env!("CARGO_PKG_VERSION")`, `const DEPOT: &str = env!("CARGO_PKG_REPOSITORY")` ; `fn ouvrir_apropos(parent: &gtk::Window)` (une seule fenêtre) ; `pupitre -v` et `pupitre --version` → `pupitre 0.1.0` sur une ligne, code 0.

**« À propos de Pupitre »** (`gtk::AboutDialog`, entrée en fin de menu, après « Aide ») : nom, version, commentaire « Terminal à onglets accessible », site = `DEPOT`, licence `gtk::License::Gpl30` (« ou toute version ultérieure »), section « Polices » listant les cinq polices embarquées avec « SIL Open Font License 1.1 ». Échap ferme ; tout est lu par Orca.

**Bouton « + »** (choix 6) : dans la barre d'onglets, à gauche du bouton de menu ; nom accessible « Nouvel onglet », infobulle « Nouvel onglet (Ctrl+Maj+T) » (raccourci en vigueur), atteint par Ctrl+Tab ; même action que `win.nouvel-onglet`.

- [ ] **Step 1 : Tests** : CLI `version_courte_et_longue` (`-v` et `--version` → `pupitre 0.1.0\n`, code 0) ; graphiques : `apropos_contenu` (version, site, licence, section « Polices » avec les cinq familles), `apropos_unique`, `bouton_nouvel_onglet` (nom accessible, clic → un onglet de plus, même shell que l'onglet actif).
- [ ] **Step 2 :** échec. **Step 3 :** implémenter. **Step 4 :** tests verts.
- [ ] **Step 5 : Commit** `feat(pupitre): À propos, pupitre -v, bouton de nouvel onglet`.

---

### Task 8 : Fenêtre de réglages

**Files :** Create: `crates/config/src/edition.rs`, `crates/pupitre/src/reglages.rs` ; Modify: `crates/config/Cargo.toml` (`toml_edit`), `crates/config/src/lib.rs`, `crates/pupitre/src/fenetre.rs` (Préférences ▸ « Réglages… » en tête du sous-menu ; `Raccourci::Reglages`, sans touche par défaut, clé `reglages` de `[raccourcis]` (Task 6)), `assets/aide/fr.md` ; Test: unitaires de `edition.rs`, `crates/pupitre/tests/gui_reglages.rs`.

**Interfaces :**
- Consumes : `chemin_config()`, `ecrire_atomique`, `charger` ; `polices_chasse_fixe()` (Task 5) ; `shells_proposes` (étape 3) ; `ThemeChoix` (Task 4) ; `BandeauClavier` (Task 3) ; `ReglagesNotifications` (Task 2).
- Produces : `fn modifier_cle(texte: &str, chemin: &[&str], valeur: toml_edit::Value) -> Result<String, ErreurEdition>` (remplace la valeur en gardant commentaires, ordre, clés inconnues ; ajoute la clé en fin de table si absente) ; `fn enregistrer_reglage(chemin: &[&str], valeur: toml_edit::Value) -> Result<(), ErreurEdition>` (lit le fichier, refuse si `charger` échoue, sauvegarde `.bak`, `ecrire_atomique`) ; `fn ouvrir_reglages(app: &gtk::Application, parent: &gtk::Window)` (une seule fenêtre).

**Fenêtre « Réglages de Pupitre »** : une colonne de champs, chacun avec son étiquette (relation `labelled-by`) : police (liste des polices à chasse fixe) et taille (6 à 72) ; thème (système, sombre, clair, contraste élevé sombre, contraste élevé clair) ; shell (shell de connexion, shells installés) ; invite (Starship, celle du shell) ; annonce de fin de commande ; copie à la sélection ; bandeau clavier ; les six cases de `[notifications]`. **Chaque changement est enregistré aussitôt** (choix 3) et appliqué par le rechargement à chaud ; message bref « Enregistré. ». Fichier invalide à l'ouverture : la fenêtre s'ouvre en lecture seule avec le message « pupitre.toml contient une erreur (ligne N) : corrigez-la d'abord. » et aucun champ modifiable. Échap ferme ; ordre de tabulation = ordre visuel. Raccourcis : pas de champ dans cette fenêtre (modifiables dans le fichier, Task 6).

- [ ] **Step 1 : Tests** : `reglage_garde_les_commentaires` (fichier par défaut, `theme` changé → seul le texte de la valeur change, octet pour octet ailleurs) ; `cle_absente_ajoutee` ; `table_notifications_creee` ; `cles_inconnues_gardees` ; `fichier_invalide_pas_d_ecriture` ; `sauvegarde_bak` ; graphiques : `reglages_champs_etiquetes` (chaque champ a un nom accessible), `changement_applique` (thème changé dans la fenêtre → fond du terminal changé en moins de 1 s), `fichier_invalide_lecture_seule`, `reglages_unique`, `echap_ferme`.
- [ ] **Step 2 :** échec. **Step 3 :** implémenter. **Step 4 :** tests verts (X11, Wayland).
- [ ] **Step 5 : Commit** `feat(pupitre): fenêtre de réglages (pupitre.toml, commentaires gardés)`.

---

### Task 9 : Restes de l'étape 3 (si validé, choix 7)

**Files :** Modify: `crates/pupitre/src/shells.rs`, `crates/pupitre/src/fenetre.rs`, `docs/verifications/etape-3.md` (restes réglés).

- M7 : `entrees_menu` calculé une fois par relecture de configuration (pas une fois par fenêtre) et sur un fil à part ; le menu garde ses entrées précédentes en attendant. Test : `menu_refait_une_fois_par_relecture`.
- N4 : `Onglet::reouverture_en_cours` ; une seconde Entrée sur un onglet terminé pendant une réouverture est ignorée. Test : `deux_entrees_un_seul_shell` (le fond ne compte qu'une session de plus).

- [ ] **Step 1 :** tests. **Step 2 :** échec. **Step 3 :** implémenter. **Step 4 :** verts.
- [ ] **Step 5 : Commit** `fix(pupitre): restes de l'étape 3 (recherche des shells hors du fil GTK, double réouverture)`.

---

### Task 10 : Orca, isolement, VM

**Files :** Modify: `tests/a11y/orca/scenario.py`, `docs/orca-liste-de-controle.md`, `tests/isolement/verifier.py`, `tests/vm/commun.sh`.

- [ ] **Step 1 : Orca (automatisé, Orca muet)** : 22 — `printf '\e]52;c;QUJD\a'` → Orca dit « Un programme a copié 3 caractères » ; 23 — dans un onglet caché, `sleep 2; printf '\e]9;fini\a'`, revenir au premier onglet, Ctrl+Tab jusqu'aux onglets → Orca lit « …, notification » ; 24 — menu ▸ Préférences ▸ Réglages… → Orca dit « Réglages de Pupitre » puis le premier champ avec son étiquette ; Tab → champ suivant ; 25 — menu ▸ « À propos de Pupitre » → version lue ; 26 — bouton « + » atteint par Ctrl+Tab, lu « Nouvel onglet bouton ». 26/26 trois fois de suite.
- [ ] **Step 2 : Isolement** : session type avec copie OSC 52 et réglage changé dans la fenêtre → aucune écriture hors des dossiers de Pupitre (`pupitre.toml.bak` compris), aucune connexion réseau.
- [ ] **Step 3 : VM** : `tests/vm/commun.sh` gagne « bulle de fin de commande » (fenêtre non active, `sleep 11` → notification reçue par le démon du bureau) et « disposition lue » (bandeau non vide). Sway, GNOME, KDE → 17/17 chacun.
- [ ] **Step 4 : Commit** `test(a11y): notifications, réglages, À propos, bouton +, essais en VM`.

---

### Task 11 : Vérification de fin d'étape

- [ ] **Step 1 :** `cargo clippy --workspace --all-targets -- -D warnings` → 0 ; `cargo fmt --check`.
- [ ] **Step 2 :** `scripts/test-x11.sh --no-fail-fast` et `scripts/test-wayland.sh --no-fail-fast` → tout passe (machine peu chargée ; relancer une fois un échec connu pour être sensible à la charge et le signaler).
- [ ] **Step 3 :** fuzzing `trame` et `marques`, 5 min chacun → aucun plantage.
- [ ] **Step 4 :** `scripts/verifier-isolement.sh` → conforme ; `scripts/test-orca.sh` → 26/26 ; VM Sway, GNOME, KDE → 17/17 ; `scripts/construire-deb.sh` puis installation sur Debian 13 neuve (Docker) : `pupitre -v`, `pupitre polices`.
- [ ] **Step 5 : Essai sur le poste du porteur** (3 minutes, rien à écouter) : changer de disposition (le bandeau suit) ; menu ▸ Préférences ▸ Réglages… → choisir « contraste élevé sombre » (appliqué aussitôt), puis ouvrir `pupitre.toml` (commentaires toujours là) ; `sleep 12` dans un onglet, passer dans une autre application → bulle ; `printf '\e]52;c;QUJD\a'` puis coller ailleurs → « ABC » ; menu ▸ « À propos » ; `pupitre -v` ; bouton « + ».
- [ ] **Step 6 :** consigner dans `docs/verifications/etape-4.md`, cocher le plan, mettre à jour `suivi-tokens.md`, commit `docs: vérifications de l'étape 4` ; puis relecture finale indépendante de l'étape.

---

## Choix du concepteur (à valider par le porteur)

1. **Valeurs des thèmes à contraste élevé** : propositions de la Task 4 (fond noir pur / blanc pur, couleurs ajustées jusqu'à 7:1), **validées sur maquettes avant le code**.
2. **Bulles limitées à une toutes les 10 s par onglet** (une nouvelle bulle du même onglet remplace l'ancienne) ; l'indicateur de l'onglet, lui, n'est jamais limité.
3. **Fenêtre de réglages : chaque changement est enregistré aussitôt** (pas de bouton « Appliquer »), appliqué par le rechargement à chaud ; entrée « Réglages… » dans le sous-menu Préférences, **sans raccourci par défaut** (assignable : clé `reglages`).
4. **Raccourcis au format de l'aide** (`"Ctrl+Maj+T"`) dans une section `[raccourcis]` ; Ctrl+Tab, Ctrl+Maj+Tab, Alt+1…9 et Ctrl+PgSuiv/PgPréc **non modifiables** (toujours disponibles pour sortir du terminal et changer d'onglet) ; Tab, F1 et F10 jamais pris.
5. **Copie par un programme limitée à 1 Mio** ; au-delà, rien n'est copié (sans message : un `cat` binaire ne doit pas remplir le bandeau).
6. **Bouton « + » inclus dès l'étape 4** (Task 7), à gauche du bouton de menu.
7. **Restes de l'étape 3 inclus** : M7 (recherche des shells hors du fil GTK) et N4 (double Entrée) ; M5, N3 et les limites de fish restent en l'état.
8. **« Suivre le système » ne suit pas le contraste élevé du bureau** : GTK 4.18 ne donne pas cette information ; le contraste élevé se choisit dans la configuration ou la fenêtre de réglages.
