# Pupitre — document de conception

*Version 1 — 2026-10-25. Issu de la revue des caractéristiques menée avec le porteur du projet.
Sources : `description.md` (cahier des charges), `essai-jetable/RESULTATS.md` (mesures de VTE),
maquettes dans `maquettes/`. Le document `st-options-parametres-features.md` est hors périmètre.*

Les décisions sont de deux sortes :
- **[Décidé]** : tranché avec le porteur du projet ;
- **[Choix du concepteur]** : fixé par Claude faute de préférence exprimée, modifiable à la relecture.

---

## 1. Intention

Pupitre est un émulateur de terminal pour Linux. Il doit :
- être **accessible** (WCAG 2.2 niveau AA, RGAA par analogie / EN 301 549), par principe et non pour
  un utilisateur particulier ;
- **respecter strictement la vie privée** : rien ne sort de l'application, sauf par une commande de
  l'utilisateur (ssh, git…) ;
- être **robuste** : « ne jamais planter et ne perdre aucune donnée » ;
- rendre le travail quotidien agréable : invite configurable, palette lisible, SSH sans saisie répétée
  de phrase de passe, bonnes conditions pour les agents IA en ligne de commande (Claude Code…).

Le porteur du projet ne code pas : Claude écrit et entretient le code.

## 2. Objectifs, non-objectifs, anti-objectifs

**Objectifs (version 1)**
- Terminal à onglets, plusieurs fenêtres, pour bash, zsh, fish, nushell (et autres shells détectés).
- Invite configurable élément par élément ; palette sémantique vérifiée en contraste.
- Processus de fond qui garde les shells en vie si l'interface plante.
- Client SSH bâti sur OpenSSH, avec phrases de passe de clés gardées dans le trousseau du système.
- Accessibilité complète au lecteur d'écran Orca et au clavier.

**Non-objectifs (version 1)**
- Panneaux divisés (*splits*) — tmux suffit.
- Fenêtre de réglages graphique (prévue plus tard, sur le même fichier de configuration).
- Assistant IA intégré (local possible plus tard ; en ligne exclu).
- Indication de mode par onglet (root, SSH, conteneur, saisie masquée, affichage figé) : prévue
  dans une version ultérieure [demandé par le porteur, 2026-11-11]. Esquisse retenue pour plus
  tard : détection par le processus de fond (programme au premier plan, OSC 7, titre `root@`),
  information doublée sur quatre canaux (onglet avec symbole et mot, cadre coloré — rayé pour
  root, titre de fenêtre, annonce Orca), couleurs tirées du thème.
- Changement de nom envisagé (« a11tty ») : écarté par le porteur le 2026-11-12, le nom reste
  « pupitre ». Le paquet `.deb` est avancé avant les autres fonctions de l'étape 4.
- Bouton « + » dans la barre de menu, qui ouvre un nouvel onglet au clic : prévu plus tard
  [demandé par le porteur, 2026-11-12]. Il doublera Ctrl+Maj+T ; nom accessible
  « Nouvel onglet », atteignable au clavier.
- Restauration des sessions après redémarrage de l'ordinateur.
- Ligatures, images (sixel/kitty), emoji composés, protocole clavier kitty, synchronisation de
  l'affichage : absents de VTE (voir §4.2).
- Mémorisation des mots de passe de serveurs (seules les phrases de passe des clés sont gardées).
- macOS, Windows, BSD.

**Anti-objectifs (échec même si « ça marche »)**
- Toute connexion réseau initiée par Pupitre **de lui-même** : télémétrie, vérification de
  nouvelle version, téléchargement de ressources (polices, thèmes, icônes, Starship…), aperçu de
  lien (chargement automatique d'une page pour en afficher le titre ou une vignette), rapport de
  plantage. Ne sont **pas** concernées les commandes lancées par l'utilisateur dans le terminal
  (`apt upgrade`, `ssh`, `git`, `curl`…), ni l'ouverture d'un lien sur son clic, ni la mise à jour
  de Pupitre par le gestionnaire de paquets.
- Un plantage de l'interface qui tue les shells ou fait perdre une commande en cours.
- Un fichier de l'utilisateur modifié sans son action (`~/.bashrc`, `~/.config/starship.toml`,
  `~/.ssh/config`…).
- Une information transmise par un seul canal (seulement une couleur, seulement un son).
- Un fichier de configuration à moitié écrit ou corrompu.

## 3. Contraintes

| Contrainte | Valeur |
|---|---|
| Système | Linux uniquement, tous bureaux, **X11 et Wayland dès le début** [Décidé]. Aucune API propre à X11 dans le cœur ; les fonctions X11-only sont des bonus facultatifs. |
| Pile technique | **Rust + GTK4 + VTE** [Décidé] |
| Versions minimales | **GTK ≥ 4.18, VTE (gtk4) ≥ 0.80** — soit Debian 13, Ubuntu 25.04+, Fedora 42+ [Décidé]. En dessous : pas de texte pour le lecteur d'écran ni d'API de disposition clavier (mesuré). |
| Liaisons Rust | crates `gtk4` (feature `v4_18`) et `vte4` (feature ≥ `v0_80`). Attention : l'essai a montré des incompatibilités entre versions de crates (`vte4 0.10` + `gtk4 0.11` ne compilait pas) — fixer des versions compatibles. |
| Licence | **GPL-3.0** [Décidé] |
| Distribution | **Paquet `.deb` + compilation depuis les sources** [Décidé] |
| Machine du porteur | Debian 12 / XFCE / X11 aujourd'hui ; passage à Debian 13 envisagé. Le développement a lieu sur Debian 13 (voir §15). |

## 4. Architecture

### 4.1 Deux processus [Décidé : principe ; Choix du concepteur : détails]

```
 ┌───────────────────────┐   socket Unix (protocole versionné)  ┌───────────────────────────┐
 │ pupitre (interface)   │ ◄──────────────────────────────────► │ pupitre-fond (processus   │
 │ GTK4 + VTE            │  données écran + événements, ordonnés│ de fond, sans interface)  │
 │ - fenêtres, onglets   │  frappes clavier, taille, commandes  │ - possède les PTY/shells  │
 │ - VTE.feed() / commit │                                      │ - analyse le flux (OSC)   │
 │ - notifications, a11y │                                      │ - tampon de relecture     │
 └───────────────────────┘                                      │ - agent SSH si besoin     │
                                                                └───────────────────────────┘
```

- **`pupitre-fond`** possède les pseudo-terminaux et les processus des shells. Il est lancé à la
  demande par l'interface (un par utilisateur ; socket dans `$XDG_RUNTIME_DIR/pupitre/`), et se
  termine quand il n'a plus ni session ni interface connectée.
- **L'interface** ne possède pas les PTY : elle reçoit le flux de chaque session et le donne à VTE
  par `feed()` ; les frappes de VTE (signal `commit`) et les changements de taille sont renvoyés au
  processus de fond.
- **Plantage de l'interface** : le processus de fond garde les sessions. L'interface relancée se
  reconnecte, recrée fenêtres et onglets, rejoue le **tampon de relecture** de chaque session (flux
  brut récent, en mémoire uniquement, borné — ordre de grandeur 8 Mio par session), puis signale un
  redimensionnement pour que les programmes plein écran se redessinent.
- **Plantage du processus de fond** : les shells sont perdus ; l'interface l'affiche clairement et
  propose de rouvrir. Ce processus doit donc être minimal, sans dépendance graphique, et testé
  intensivement (§13).
- **Fermeture volontaire** d'un onglet ou d'une fenêtre où un processus tourne : confirmation, puis
  fin des processus concernés. Les sessions ne survivent pas à une fermeture volontaire.
- **Après redémarrage de l'ordinateur** : on repart de zéro, rien n'est écrit sur disque [Décidé].

**Risque à lever en premier** : le fonctionnement de VTE en mode `feed()` + `commit` (sans PTY
propre) doit être vérifié par un essai (souris, collage sécurisé, réponses aux requêtes du
terminal, accessibilité). Si un point bloque, alternative : passer le descripteur du PTY à
l'interface et dupliquer le flux vers le processus de fond — à arbitrer par le constructeur.

### 4.2 Capacités de VTE et compensation

Mesuré sur VTE 0.80 (détail : `essai-jetable/RESULTATS.md`) :

| Fonction | VTE 0.80 | Traitement dans Pupitre |
|---|---|---|
| Texte exposé au lecteur d'écran, API disposition clavier (GTK) | ✅ | utilisés tels quels |
| Truecolor, 256 couleurs, OSC 8, OSC 7, titres, collage sécurisé, souris SGR | ✅ | tels quels |
| OSC 52 écriture (presse-papiers) | ❌ | **intercepté par `pupitre-fond`** → l'interface écrit le presse-papiers |
| OSC 52 lecture | ignoré | **bloqué** (requête jetée) [Décidé] |
| OSC 9 / OSC 777 (notifications) | ❌ | **intercepté par `pupitre-fond`** |
| OSC 133 (marques de commandes) | ❌ | **intercepté par `pupitre-fond`** ; l'interface note la ligne VTE au moment où elle reçoit la marque |
| Ligatures, sixel/kitty, graphèmes, clavier kitty, synchro 2026 | ❌ | non pris en charge (non-objectifs) |

Le processus de fond relaie le flux tel quel à VTE (VTE ignore les séquences qu'il ne connaît pas),
sauf la lecture OSC 52 qu'il supprime. Les événements extraits sont transmis **dans l'ordre** du flux.

## 5. Fenêtres et onglets

- **Taille à l'ouverture** [Décidé] : largeur = **50 % de la largeur de l'écran**, hauteur =
  largeur / 1,618 (pixels, pas cellules). Uniquement à l'ouverture ; librement redimensionnable.
  Centrée sous X11 ; sous Wayland le bureau place la fenêtre. Exemple 1920×1080 → 960×593.
  Maquette : `maquettes/ratio-moitie.png`.
- **Onglets** [Décidé] : renommage, réorganisation clavier et souris, détachement dans une nouvelle
  fenêtre, titre annoncé au lecteur d'écran. Indicateurs : ✓ (commande réussie), ✗ (échec),
  ● (activité / notification / cloche), toujours doublés d'un texte accessible.
- **Plusieurs fenêtres** : oui. **Panneaux divisés** : non (v1).
- **Menu « nouveau terminal avec… »** listant les shells détectés.
- **Bandeau de disposition clavier** [Décidé] : entre la barre d'onglets et la zone de texte ;
  affiche la disposition active (nom complet, ex. « Français (BÉPO) », ou code court, au choix) ;
  masquable, **affiché par défaut** ; indicateur seul (pas d'action au clic) ; chaque changement est
  **annoncé** au lecteur d'écran. Source : `gdk::Device::layout_names()` / `active_layout_index()`.

## 6. Shells et invite

### 6.1 Shells [Décidé]
- « Disponibles d'origine » = **détection** (`/etc/shells` + recherche des noms connus dans `PATH`)
  ; pour un shell absent, Pupitre **affiche la commande d'installation** adaptée à la distribution
  (`apt install fish`…), sans rien exécuter ni télécharger lui-même.
- **Shell par défaut** : le shell de connexion de l'utilisateur ; modifiable.
- **Niveaux de prise en charge** :
  1. *Complet* — bash, zsh, fish, nushell : invite Starship, intégration (OSC 133 et dossier
     courant), couleurs de Pupitre ; testés à chaque version.
  2. *Invite gérée* — elvish, xonsh, tcsh, ion, PowerShell : invite Starship ; intégration partielle.
  3. *Tel quel* — tout autre shell, lancé sans personnalisation.
- **Shells déclarés à la main** dans la configuration (chemin + arguments, ex. `ssh serveur`,
  `tmux attach`).
- **Intégration sans toucher aux fichiers de l'utilisateur** : Pupitre lance chaque shell avec ses
  propres fichiers d'initialisation (installés dans `/usr/share/pupitre/shell/`) qui **chargent
  d'abord la configuration habituelle de l'utilisateur**, puis ajoutent l'intégration Pupitre. Le
  mécanisme exact par shell (`--rcfile`, `ZDOTDIR`, `vendor_conf.d`, options de nushell) est laissé
  au constructeur, sous ces deux exigences.

### 6.2 Invite [Décidé]
- **Starship embarqué** (licence ISC) : binaire fourni avec Pupitre, utilisé même si Starship n'est
  pas installé sur le système.
- **Configuration propre à Pupitre** via `STARSHIP_CONFIG` ; `~/.config/starship.toml` n'est jamais
  lu ni modifié.
- Les couleurs de l'invite utilisent les **noms de la palette** de Pupitre (succès, erreur,
  dossier, branche git…) : changer de thème change l'invite.
- Configuration par défaut [Choix du concepteur] : reproduit l'invite actuelle du porteur —
  `utilisateur@machine dossier git:(branche) état $`, symboles d'état git `Δ ● ○`, marqueur SSH
  `⌁⌁⌁`, marqueur `⚙⚙⚙` et symbole `≠` si `ENVIRONMENT=prod`, charge système optionnelle.
  Aucune icône Nerd Font obligatoire.
- Réglage `invite = "shell"` [Choix du concepteur] pour garder l'invite native de l'utilisateur.
- Si Starship échoue (absent, trop lent) : repli sur une invite simple intégrée, jamais d'erreur
  bloquante.

## 7. Apparence

### 7.1 Polices [Décidé]
- **Embarquées** : B612 Mono, IBM Plex Mono, Nordwand Mono, Ioskeley Mono — toutes sous SIL OFL 1.1
  (texte de licence joint). Utiliser les **versions officielles** des dépôts
  (Nordwand : github.com/tywr/Nordwand-Mono).
- **Polices supplémentaires** [Choix du concepteur] : toute police installée sur le système
  (fontconfig : `~/.local/share/fonts`, paquets `fonts-*`…) est utilisable en donnant son nom dans
  la configuration ; une nouvelle police est prise en compte sans redémarrer Pupitre. La commande
  `pupitre polices` liste les polices à chasse fixe installées. Pupitre ne télécharge jamais de
  police lui-même. Si la police demandée est introuvable : message clair et repli sur B612 Mono.
- **Par défaut : B612 Mono**, taille 11 pt [Choix du concepteur pour la taille].
- **Secours** : *Symbols Nerd Font Mono* embarquée pour les icônes ; emoji et écritures non latines
  via les polices du système.
- **Zoom** : Ctrl + / Ctrl − / Ctrl 0, jusqu'à au moins 200 %.
- **Ligatures** : abandonnées (VTE ne les gère pas).

### 7.2 Couleurs et thèmes [Décidé]
- Niveau **WCAG AA** : contraste ≥ 4,5:1.
- Pupitre affiche 16 couleurs, 256 couleurs et couleurs exactes (`TERM=xterm-256color`,
  `COLORTERM=truecolor`). **Le thème règle et garantit les 16 couleurs** ; les couleurs 16–255
  gardent leurs valeurs standard ; les couleurs exactes sont la responsabilité des programmes.
- **Thème sombre (validé)** — base Catppuccin Mocha retouchée :

  | Rôle | Valeur |
  |---|---|
  | Fond / texte | `#1e1e2e` / `#cdd6f4` |
  | 0 noir (**rôle fond**) | `#252630` |
  | 1, 9 rouge (erreur) | `#f38ba8` |
  | 2, 10 vert (succès) | `#a6e3a1` |
  | 3, 11 jaune (avertissement) | `#f9e2af` |
  | 4, 12 bleu (dossier) | `#89b4fa` |
  | 5, 13 magenta (branche git) | `#f5c2e7` |
  | 6, 14 cyan (lien / information) | `#94e2d5` |
  | 7 blanc | `#bac2de` |
  | 8 noir brillant (**rôle texte + fond**) | `#8a8ea6` |
  | 15 blanc brillant | `#b2b8cf` |
  | Sélection (fond / texte) | `#45475a` / `#cdd6f4` |
  | Curseur | `#f5e0dc` |

  Maquettes : `maquettes/palette-couleurs-v2.png`, `maquettes/couleur0-fond.png`.
- **Thème clair (validé, AA vérifié)** — base Catppuccin Latte, couleurs assombries pour passer
  4,5:1 sur le fond **et** sur les panneaux de couleur 7 ; texte sur couleurs vives = blanc (15) :

  | Rôle | Valeur |
  |---|---|
  | Fond / texte | `#eff1f5` / `#4c4f69` (7,1:1) |
  | 0 noir (rôle texte) | `#4c4f69` |
  | 1, 9 rouge | `#c30e35` |
  | 2, 10 vert | `#2d701e` |
  | 3, 11 jaune (ocre) | `#895712` |
  | 4, 12 bleu | `#0a54e7` |
  | 5, 13 magenta | `#b31b8a` |
  | 6, 14 cyan | `#116c71` |
  | 7 blanc (**rôle fond**, panneaux) | `#dce0e8` |
  | 8 gris (**rôle texte + fond**) | `#5c5f77` |
  | 15 blanc brillant (**rôle fond**) | `#ffffff` |
  | Sélection (fond / texte) | `#ccd0da` / `#4c4f69` (5,2:1) |
  | Curseur | `#4c4f69` (7,1:1) |

  Couleurs 1–6 : 5,4:1 sur le fond, ≥ 4,6:1 sur les panneaux, 6,1:1 sous du texte blanc.
  Seule faiblesse, comme en sombre : un panneau de couleur 7 se distingue peu du fond (1,17:1),
  sans effet sur la lisibilité. Maquette : `maquettes/theme-clair.png`.
- **Autres thèmes** : contraste élevé sombre et clair (cible 7:1 [Choix du concepteur]), et
  **suivi automatique** clair/sombre selon le bureau. Leurs valeurs sont à concevoir par le
  constructeur selon les mêmes règles.
- **Vérificateur de contraste** : chaque couleur porte un **rôle** (texte, fond, texte + fond).
  Il teste les couleurs « texte » sur le fond, les couleurs « fond » sous le texte qu'elles portent,
  et les **paires** 0/8, 7/15, 0/7, 0/15, et la vidéo inversée. Il tourne à chaque rechargement de
  la configuration et via `pupitre check` ; un échec donne un avertissement précis (couleurs,
  ratio obtenu, minimum), jamais un refus.
- **Texte « atténué »** (SGR 2) : à vérifier dans VTE ; s'il réduit le contraste sous 4,5:1, le
  remplacer par une couleur réelle conforme ou le désactiver.
- **Curseur** : bloc, **non clignotant par défaut** ; paramètre pour le faire clignoter, ignoré si le
  bureau demande de réduire les animations [Décidé].
- Amélioration future (hors v1) : ajustement automatique du contraste des couleurs exactes.

## 8. Accessibilité [Décidé]

- **Orca** : lecture du texte du terminal (VTE 0.80) ; **annonce de fin de commande** avec son
  résultat (via OSC 133, `gtk_accessible_announce`) ; navigation de commande en commande ;
  annonce des changements de disposition clavier et des messages de Pupitre.
- **Tout au clavier** : onglets, menus, recherche, dialogues ; focus toujours visible.
- **Agrandissement** jusqu'à 200 % sans perte de contenu.
- **Cloche visuelle** (pas de son par défaut), ≤ 3 flashs par seconde.
- **Raccourcis modifiables**, sans conflit avec ceux d'Orca.
- **Jamais un seul canal** pour une information.
- **Tests** : sonde automatique de l'arbre AT-SPI (comme `essai-jetable/sonde_a11y.py`) dans la
  suite de tests ; séance manuelle avec Orca avant chaque version (liste de contrôle à rédiger).

## 9. Notifications [Décidé]

Tout est **silencieux par défaut** ; chaque case est réglable.

| Événement | Onglet | Message dans la fenêtre | Bulle du bureau | Son |
|---|---|---|---|---|
| Commande terminée en arrière-plan | ✓ / ✗ | — | si Pupitre n'est pas au premier plan **et** durée > 10 s | non |
| Programme qui demande à notifier (OSC 9/777) | ● | — | oui | non |
| Cloche | ● | — | non | non |
| Presse-papiers écrit par un programme | — | « Un programme a copié N caractères » (3 s) | non | non |
| Problème de Pupitre (configuration…) | — | persistant jusqu'à correction | non | non |

Bulles via les notifications de GIO (locales). Respect du mode « Ne pas déranger ».

## 10. SSH et secrets [Décidé]

- **OpenSSH** est utilisé tel quel (aucun client réécrit). Ouvrir une connexion = nouvel onglet
  lançant `ssh <hôte>`. La vérification des empreintes de serveur reste celle d'OpenSSH.
- **Agent SSH garanti, dans Pupitre seulement** [Décidé] : au démarrage, si `SSH_AUTH_SOCK`
  désigne un agent joignable, il est utilisé ; sinon Pupitre cherche un agent déjà en marche
  appartenant à l'utilisateur (sockets usuels dans `$XDG_RUNTIME_DIR` et `/tmp/ssh-*`) et le
  réutilise [Choix du concepteur] ; à défaut `pupitre-fond` lance un `ssh-agent`. `SSH_AUTH_SOCK`
  est exporté à tous les shells de Pupitre (git y en bénéficie). **L'environnement de la session de
  bureau n'est jamais modifié**, même sur demande : git lancé hors de Pupitre n'est pas concerné. Constat chez le porteur : un agent tourne mais `SSH_AUTH_SOCK` est absent de
  son environnement, d'où la phrase de passe redemandée à chaque commande git.
- **Phrase de passe** : demandée dans une **fenêtre de Pupitre** accessible (programme `SSH_ASKPASS`
  fourni par Pupitre), avec une case « Mémoriser ». Mémorisée dans le **trousseau du système**
  (Secret Service : GNOME Keyring, KWallet…), puis la clé est chargée automatiquement dans l'agent.
  Résultat attendu : plus aucune saisie après la première.
- **Connexions mémorisées** [Choix du concepteur] : la liste affiche les hôtes de `~/.ssh/config`
  (lecture seule) et ceux enregistrés par Pupitre dans son propre fichier
  (`$XDG_CONFIG_HOME/pupitre/connexions.toml` : nom, hôte, utilisateur, port, clé). Usage actuel du
  porteur : un seul serveur — l'interface reste minimale.
- Trousseau verrouillé ou absent : message clair, repli sur la saisie à chaque fois, jamais de
  stockage en clair.

## 11. Vie privée [Décidé]

- Aucune connexion réseau initiée par Pupitre : pas de télémétrie, pas de vérification de mise à
  jour (le gestionnaire de paquets s'en charge), pas de rapport de plantage (journal local
  uniquement, `$XDG_STATE_HOME/pupitre/`), rien de téléchargé (polices, thèmes, Starship embarqués),
  aucun aperçu de lien.
- Liens : ouverts dans le navigateur uniquement sur action de l'utilisateur (Ctrl + clic).
- Historique de défilement et sessions : **jamais écrits sur disque**.
- OSC 52 : lecture bloquée, écriture autorisée avec message.
- **Téléchargement à la demande** [Décidé : principe ; hors v1] : le porteur souhaite pouvoir
  télécharger des ressources depuis Pupitre, presque certainement des polices. Règles fixées dès
  maintenant : uniquement sur action explicite de l'utilisateur ; l'adresse source est affichée
  avant le téléchargement ; installation dans le dossier de polices de l'utilisateur
  (`~/.local/share/fonts/`) avec sa licence ; aucune consultation automatique d'un catalogue.
  Forme (adresse donnée par l'utilisateur, ou catalogue d'un site précis) à décider plus tard.
  Source envisagée par le porteur : **fonts.bunny.net** (UE, sans journaux). Limite constatée :
  ses fichiers sont des polices web `.woff2` découpées par alphabet, sans licence intégrée — il
  faudrait les réassembler et récupérer la licence à part. Alternatives à fichiers complets :
  paquets Debian `fonts-*`, dépôt GitHub `google/fonts`, Nerd Fonts.
- **Aide-mémoire de commandes** [Décidé : à prévoir ; hors v1] : panneau d'exemples courts pour
  une commande, à partir des **tldr pages** stockées sur la machine (consultation hors ligne).
  Leur téléchargement et leurs mises à jour suivent les mêmes règles (action explicite, source
  affichée). Pas de service interrogé en ligne à chaque recherche (type cheat.sh).

## 12. Configuration [Décidé : principe ; Choix du concepteur : emplacements]

- **Fichier texte TOML** : `$XDG_CONFIG_HOME/pupitre/pupitre.toml` ; thèmes dans
  `$XDG_CONFIG_HOME/pupitre/themes/*.toml` ; invite dans `$XDG_CONFIG_HOME/pupitre/starship.toml`.
  Fichier par défaut commenté, créé au premier lancement.
- **Rechargement à chaud** : toute sauvegarde est appliquée aux fenêtres ouvertes.
- **Erreur dans le fichier** : Pupitre garde la configuration précédente et affiche la ligne fautive
  et la raison ; jamais de plantage.
- **Fenêtre de réglages** : étape 4, lisant et écrivant le même fichier ; dès l'étape 3, un menu
  « Préférences » ouvre les fichiers de configuration dans l'éditeur de texte.

### 12.1 Réglages du quotidien [Choix du concepteur]

| Réglage | Valeur par défaut |
|---|---|
| Historique de défilement | 10 000 lignes par onglet, en mémoire uniquement |
| Recherche dans l'historique | Ctrl+Maj+F, texte simple ou expression régulière |
| Copier / coller | Ctrl+Maj+C / Ctrl+Maj+V ; sélection primaire (clic du milieu) active |
| Copie automatique à la sélection | oui, au relâchement du bouton de la souris (réglage `copie_selection`, désactivable) ; la recherche ne copie pas [demandé par le porteur, 2026-11-11] |
| Nouvel onglet / fermer / fenêtre | Ctrl+Maj+T / Ctrl+Maj+W / Ctrl+Maj+N |
| Connexions SSH | Ctrl+Maj+O |
| Onglet suivant / précédent | Ctrl+PgSuiv / Ctrl+PgPréc ; Alt+1…9 |
| Plein écran | F11 |
| Sortir du terminal au clavier | Ctrl+Tab / Ctrl+Maj+Tab (messages, recherche, onglets, menu, terminal) ; Tab reste au shell ; F10 reste aux programmes [Choix du concepteur, 2026-10-31] |
| URL en texte brut | détectées, ouvertes par Ctrl+clic |
| Titre d'onglet | titre envoyé par le programme, sinon dossier courant |
| Défilement à la frappe | oui ; défilement à la sortie : non |

## 13. Robustesse [Décidé : exigence ; Choix du concepteur : moyens]

Définition du porteur : « qu'il ne plante jamais et ne perde aucune donnée ».

- Architecture à deux processus (§4.1).
- **Écritures atomiques** (fichier temporaire + renommage) pour la configuration, les connexions et
  toute donnée persistante, avec copie de la version précédente.
- **Aucune erreur prévisible ne provoque d'arrêt** : configuration invalide, police manquante,
  Starship absent, agent SSH indisponible, trousseau verrouillé, shell introuvable → message et
  repli.
- **Isolation** : une session qui reçoit des données malformées n'affecte pas les autres.
- Code Rust : pas de `unwrap`/`expect`/`panic` hors tests (règle de lint) ; panique attrapée aux
  frontières de chaque session.
- **Fuzzing** de l'analyseur de flux de `pupitre-fond` et du protocole entre processus.
- Protocole interface ↔ processus de fond **versionné** (une interface mise à jour peut parler à un
  processus de fond plus ancien ou le relancer proprement).

## 14. IA [Décidé]

Version 1 : Pupitre **accueille bien les agents** en ligne de commande, sans assistant intégré.
Concrètement : notifications OSC 9/777 (« l'agent attend votre réponse »), liens OSC 8, marques de
commandes, couleurs exactes, collage sécurisé, bonnes performances sur les gros affichages.
Hors de portée (VTE) : Maj+Entrée distinct d'Entrée (protocole clavier kitty), synchronisation
de l'affichage. Assistant local : piste future. Assistant en ligne : exclu.

## 15. Tests, environnement et livraison

- **Environnement de développement** [Décidé] : Debian 13 (GTK 4.18 / VTE 0.80). **Le
  développement commence après le passage du laptop du porteur à Debian 13**, pour qu'il puisse
  essayer chaque étape au quotidien. Le plan peut être écrit avant.
- **Ordre de livraison** [Décidé : par étapes ; Choix du concepteur : contenu des étapes] — chaque
  étape est utilisable au quotidien :
  1. Terminal à onglets, accessible (Orca, clavier) et robuste (architecture à deux processus
     complète dès cette étape), palette sombre validée, B612 Mono, configuration TOML avec
     rechargement et vérificateur de contraste, taille d'ouverture au nombre d'or.
  2. SSH : agent garanti, askpass Pupitre, trousseau, connexions mémorisées.
  3. Shells et invite : détection, niveaux de prise en charge, Starship du système, intégration
     OSC 133 (fin de commande, navigation, annonces), aide intégrée, menu « Préférences » ouvrant
     les fichiers de configuration [Décidé par le porteur, 2026-11-01].
  4. Bandeau de disposition clavier, notifications complètes, OSC 52, thèmes clair et contraste
     élevé, autres polices, **fenêtre de réglages** (lit et écrit `pupitre.toml` en gardant ses
     commentaires) [Décidé par le porteur, 2026-11-01], paquet `.deb`.
  5. **Traductions** [Décidé par le porteur, 2026-11-01] : anglais, espagnol, italien, allemand,
     néerlandais — interface, annonces d'Orca, aide, commandes, commentaires de la configuration,
     fichier `.desktop`. Langue du système par défaut, réglage `langue` pour la forcer. Système
     (Fluent, textes intégrés à l'exécutable, repli anglais puis français) mis en place dès
     l'étape 3. Relecture de chaque langue par une personne qui la parle, avec Orca dans cette
     langue, avant publication.
- **Disposition clavier du porteur** : français Dvorak (`fr`, variante `dvorak`), testée en priorité.
  Raccourcis [Choix du concepteur] : reconnus par le caractère produit, avec repli sur la position
  physique de la touche quand le caractère n'existe pas sans Maj (ex. Alt+1…9 = rangée des chiffres).
- **Tests automatiques** : unitaires ; intégration sous écran virtuel X11 (Xvfb) **et** Wayland
  (Weston sans écran) ; sonde AT-SPI ; vérificateur de contraste sur tous les thèmes fournis ;
  fuzzing (§13).
- **Tests manuels** avant chaque version [Décidé] : le **porteur** déroule une liste de contrôle
  Orca d'environ 15 minutes sur son laptop (XFCE/X11) ; **Claude** teste GNOME/Wayland,
  KDE/Wayland et Sway dans des machines virtuelles automatisées.
- **Livraison** : paquet `.deb` (dépendances : GTK 4.18+, VTE 0.80+, OpenSSH client, Secret Service
  recommandé) + instructions de compilation depuis les sources. Texte de licence GPL-3.0, licences
  OFL des polices et ISC de Starship inclus.

## 16. Suivi du projet

- Tenir à jour `suivi-tokens.md` : tokens consommés et modèles utilisés (exigence du cahier des
  charges).

## 17. Laissé au constructeur

- Découpage des modules Rust et format exact du protocole entre processus.
- Mécanisme d'initialisation propre à chaque shell (§6.1), dans les exigences données.
- Valeurs précises des thèmes clair et contraste élevé (selon les règles du §7.2).
- Taille exacte du tampon de relecture (§4.1).
- Contenu de la liste de contrôle Orca.
