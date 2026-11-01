# Vérifications de l'étape 1

## Texte atténué (SGR 2) — Task 8, 2026-10-31

Mesure : `crates/pupitre/tests/gui_fenetre.rs`, test `texte_attenue_lisible` (pavés pleins U+2588
rendus par VTE 0.80 / GTK 4.18, capture de la texture, X11 et Wayland).

VTE affiche le texte atténué en **multipliant la couleur par 2/3** (vers le noir).

| Thème | Texte atténué | Couleurs de palette sous 4,5:1 une fois atténuées |
|---|---|---|
| Clair | #4c4f69 → #333546 (10,68:1) | aucune (toutes ≈ 9:1 : l'atténuation assombrit, donc renforce) |
| Sombre | #cdd6f4 → #898fa3 (5,09:1) | 1 et 9 (3,39:1), 4 et 12 (3,66:1), 7 (4,24:1), 8 (2,59:1), 15 (3,89:1) |

**Décision** (conception, section 7 : « le remplacer par une couleur réelle conforme ou le
désactiver ») : l'atténuation est **désactivée pour tout thème où une couleur de texte atténuée
passe sous 4,5:1** — aujourd'hui le thème sombre. Le paramètre `2` est retiré des séquences SGR
avant `feed()` (`crates/pupitre/src/attenuation.rs`) ; le texte s'affiche dans sa couleur normale.
Le thème clair garde l'atténuation. Les thèmes de l'utilisateur sont jugés par le même calcul.

## Fin d'étape — Task 13, 2026-10-31

Machine : Debian 13 (trixie), GTK 4.18.6, VTE 0.80, Orca 48.1, écrans virtuels Xvfb
(X11, avec xfwm4) et Weston sans écran (Wayland).

| Vérification | Commande | Résultat |
|---|---|---|
| Lints | `cargo clippy --workspace --all-targets` | 0 avertissement |
| Tests X11 | `scripts/test-x11.sh` | tout réussi (5 groupes de tests graphiques) |
| Tests Wayland | `scripts/test-wayland.sh` | tout réussi (5 groupes de tests graphiques) |
| Fuzzing du protocole | `cargo +nightly fuzz run trame -- -max_total_time=300` | 1 294 029 essais en 301 s, aucun plantage |
| Lecteur d'écran | `scripts/test-orca.sh` (Orca muet, 14 vérifications) | 14/14, trois passages de suite |
| Réseau | `scripts/verifier-isolement.sh` (strace) | aucune connexion hors sockets Unix (16 connexions, toutes Unix) |
| Écritures | `scripts/verifier-isolement.sh` (strace) | rien hors `~/.config/pupitre`, `~/.local/state/pupitre` et le socket dans `$XDG_RUNTIME_DIR/pupitre` |
| Reprise après plantage | test `reprise_apres_plantage` (SIGKILL de l'interface, relance) | réussi sous X11 et sous Weston |

**Écritures tolérées** : au premier lancement, deux bibliothèques chargées par GTK
remplissent leur cache dans `~/.cache` : Mesa (`mesa_shader_cache`, `mesa_shader_cache_db`)
et fontconfig (`fontconfig`). Ce n'est pas le code de Pupitre ; le script les signale à
part, sans échec.

Essais sur machines virtuelles Sway, GNOME et KDE : faits le 2026-11-01, voir « Essais en
VM » plus bas. Le scénario « 3 onglets, `kill -9`, relance » est
couvert par le test automatique ci-dessus sur X11 et Weston.

### Défauts trouvés par le test Orca automatisé, et corrigés

1. **Rien n'était annoncé au lancement.** GTK 4.18 signale l'activation de la fenêtre
   avant d'avoir fini de s'inscrire sur le bus d'accessibilité : Orca cherche la fenêtre,
   ne la trouve pas et l'ignore. Correction : 500 ms après la première activation,
   Pupitre retire puis redonne le focus (`reannoncer_au_lancement`).
2. **Nouvel onglet, barre de recherche : rien n'était annoncé.** Un widget qui reçoit le
   focus juste après sa création n'est pas encore publié ; l'événement « focus » se perd.
   Correction : l'annonce est refaite 100 ms plus tard (`annoncer_focus_une_fois_affiche`).
3. **Messages (configuration invalide) et « Aucun résultat » non lus.** L'annonce partait
   d'un widget qui venait d'apparaître. Correction : toutes les annonces partent de la
   fenêtre (`messages::annoncer`).
4. **Ctrl+Tab vers les onglets : Orca disait seulement « groupement ».** Le carnet
   d'onglets n'avait pas de nom. Correction : nom « Onglets » et description
   « essai, onglet 2 sur 3 ; cloche dans : … ». Les flèches gauche et droite gardent
   maintenant le clavier sur les onglets (avant, chaque flèche renvoyait au terminal).
5. **Orca se figeait pendant la lecture ligne par ligne** (pavé 7/8/9). Le champ caché
   qui sert à renommer un onglet (enfant caché d'une GtkStack) était publié avec une
   largeur aberrante (−727 874 992) ; Orca calcule les intersections point par point et
   ne finissait jamais. Correction : boîte simple, champ vraiment masqué.
6. **« [0m » lu dans « Le shell s'est terminé ».** Le code de couleur passait dans
   l'annonce. Correction : annonce en texte simple.
7. **Question du dialogue de fermeture lue deux fois.** Relation « décrit par » retirée.

Reste connu : sur la barre d'onglets, Orca lit parfois la description deux fois
(« essai, onglet 2 sur 2, groupement, essai, onglet 2 sur 2 ») ; c'est sa façon de
présenter un groupement.

## Essais en VM — 2026-11-01

### Sway

`scripts/vm.sh sway tout` : VM Debian 13 (image officielle, empreinte vérifiée ; 2 cœurs,
2 Go ; KVM), Sway 1.10.1 sans écran (backend headless), clavier fr dvorak, Pupitre de
`target/release`. Touches simulées par `wtype`, résultats lus dans des fichiers, fenêtres
par `swaymsg`, accessibilité par AT-SPI. Déroulé commun : `tests/vm/commun.sh`.

**13 réussis sur 13** : fenêtre ouverte et focalisée (identifiant `fr.pupitre.Pupitre`) ;
saisie et Entrée ; Ctrl+Maj+T (nouvel onglet, autre terminal) ; Ctrl+Maj+V (collage) ;
Ctrl+PgPréc ; fenêtre de phrase de passe SSH focalisée, phrase acceptée, clé chargée dans
l'agent, clavier rendu au terminal ; Ctrl+Maj+O (Connexions SSH focalisée) et Échap ;
arbre d'accessibilité (liste d'onglets, terminal, bouton « Menu »). Orca lui-même n'est pas
lancé dans la VM : l'arbre AT-SPI est ce qu'il lit.

**Défauts trouvés et corrigés**

1. *Pupitre paniquait au lancement* (f84c667) : l'icône des fenêtres, ajoutée le jour même,
   était posée avant l'initialisation de GTK. Aucun test ne lançait le vrai programme ;
   nouveau test `gui_binaire` (X11 et Wayland).
2. *Les dialogues n'étaient pas rattachés à l'application* : sous Wayland, Sway les voyait
   sous l'identifiant `pupitre` au lieu de `fr.pupitre.Pupitre` (bureau : pas d'icône ni de
   regroupement). Les tests SSH vérifient désormais ce rattachement.

**Particularités de l'outil, pas de Pupitre** : `wtype` envoie son propre plan de clavier à
chaque appel et GTK, comme le terminal `foot`, perd alors la première touche (contourné par
une touche neutre). Sous `wtype`, « ( » est arrivé en « [ » dans Pupitre mais pas dans
`foot` : à confirmer avec un vrai clavier sous Sway ; le traitement des caractères est
celui de GTK et VTE, Pupitre n'y touche pas.

### GNOME 48

`scripts/vm.sh gnome tout` : même VM Debian 13 (3 Go), GNOME Shell 48.7 sans écran
(`--headless --virtual-monitor 1280x800`), clavier fr dvorak. Touches envoyées par
l'interface de bureau à distance de Mutter (`org.gnome.Mutter.RemoteDesktop`), état des
fenêtres et captures par GNOME Shell mis en mode « unsafe » par une petite extension
installée dans la VM seulement (`tests/vm/gnome.py`, `tests/vm/essai-gnome.sh`). La vue
d'ensemble (Activités), affichée au démarrage de GNOME, est refermée avant de lancer
Pupitre, comme le ferait un utilisateur.

**13 réussis sur 13**, mêmes essais que sous Sway. Aucun défaut propre à GNOME. Même
particularité d'outil que `wtype` : chaque nouveau clavier virtuel perd sa première touche
(contourné par une touche neutre).

### KDE Plasma 6.3 (KWin 6.3.6)

`scripts/vm.sh kde tout` : même VM Debian 13 (3 Go), avec le noyau Debian standard (le noyau
« cloud » de l'image n'a ni pilote graphique ni uinput) et une carte graphique virtio. KWin
tourne sur cette carte dans une session ouverte automatiquement sur la console, et lit par
libinput un clavier simulé au niveau du noyau (uinput, `tests/vm/kde.py`), comme un clavier
branché. Plan de clavier QWERTY américain dans cette VM (le simulateur traduit pour ce
plan). État des fenêtres par un petit script KWin (D-Bus `/Scripting`), captures par
`spectacle` (contrôle de permission levé pour KWin dans la VM d'essai seulement). Seul
KWin est lancé, pas le panneau Plasma : c'est KWin qui décide du focus et des dialogues.

**13 réussis sur 13**, mêmes essais que sous Sway et GNOME. Aucun défaut propre à KDE.

Limite : la VM KDE a été mise au point en partie à la main (noyau, carte graphique) ; ces
étapes sont reportées dans `scripts/vm.sh`, mais la création complète d'une VM KDE neuve
par le script n'a pas encore été rejouée.
