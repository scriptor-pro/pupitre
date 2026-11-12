# Résultats des essais jetables, étape 4 (Task 0), 2026-11-12

Poste du porteur : Debian 13, Xfce sous X11 (`DISPLAY=:0.0`), xfce4-notifyd 0.9.7, GTK 4.18, GLib 2.84.4,
Weston 14.0.2, Claude Code 2.1.311. Disposition du porteur : `fr` variante `dvorak`,
option `grp:rctrl_rshift_toggle` (une seule disposition déclarée).
Fichiers : `bulle.py`, `disposition.py`, `osc.sh`, `toml_edit_essai/` (+ `annote.toml`,
`sans_newline.toml`, `crlf.toml`). Python 3 + `python3-gi` (Gtk 4.18) utilisés à la place d'un
projet Rust pour GTK ; `toml_edit` essayé en Rust (0.25.19, même série que le `Cargo.lock` du dépôt).

| Critère | Résultat |
|---|---|
| Bulles visibles sous Xfce | OUI (acceptée par xfce4-notifyd, expirée après ~3 s, aucune erreur) ; confirmation visuelle et clic : à faire par le porteur |
| Bulles sous GNOME / KDE | NON ESSAYÉ (VM reportée à la Task 10) |
| Clic -> `app.montrer-onglet(42)` | NON OBSERVÉ (demande un clic) ; détail de l'action vérifié côté D-Bus, voir 2 |
| « Ne pas déranger » | NON ESSAYÉ (le changer est interdit) ; état lisible : voir 2 |
| Disposition lisible sous X11 | OUI |
| Signal de changement de disposition | PARTIEL : signaux reçus, mais pas de changement observable (une seule disposition ; Xvfb ignore) ; à refaire par le porteur |
| Disposition sous Weston | IMPOSSIBLE en headless : aucun siège clavier (voir 3) |
| `toml_edit` sans perte | OUI pour le fichier livré et le fichier annoté, SOUS RÉSERVE de recopier la décoration de la valeur ; deux écarts (CRLF, fin sans retour) voir 4 |

## 1. OSC 9, 777 et variantes (Claude Code)

Source : chaînes du binaire de Claude Code 2.1.311 (`strings`), pas d'exécution interactive.

Claude Code envoie ses notifications **uniquement si son canal est connu**. Réglage
`preferredNotifChannel` : `auto`, `iterm2`, `terminal_bell`, `iterm2_with_bell`, `kitty`,
`ghostty`, `notifications_disabled`. En `auto`, le canal se déduit du terminal détecté
(`iTerm.app` -> iterm2, `kitty` -> kitty, `ghostty` -> ghostty, Terminal.app -> bell si activée) ;
**tout autre terminal, donc Pupitre/VTE : `no_method_available`, rien n'est émis**. Pour que
l'utilisateur ait des notifications de Claude Code dans Pupitre, il doit choisir `iterm2` ou
`ghostty` dans `/config` (à documenter dans l'aide), à moins que Pupitre ne se déclare
(`TERM_PROGRAM`) comme l'un de ces terminaux : déconseillé, ce serait mentir.

Octets émis (terminateur BEL `\a` partout ; les caractères de contrôle du titre et du corps
sont remplacés par des espaces) :

- canal `iterm2` : `ESC ] 9 ; <titre>: <message> BEL` (titre et message fusionnés en un texte ;
  sans titre, seulement `<message>`) ;
- canal `ghostty` : `ESC ] 777 ; notify ; <titre> ; <message> BEL` (titre par défaut « Claude Code ») ;
- canal `kitty` : trois séquences, `ESC ] 99 ; i=<id>:d=0:p=title ; <titre> BEL`, puis
  `ESC ] 99 ; i=<id>:p=body ; <message> BEL`, puis `ESC ] 99 ; i=<id>:d=1:a=focus ; BEL`
  (OSC 99 : hors périmètre §9, à ignorer) ;
- `terminal_bell` : le caractère `BEL` seul (déjà géré par la cloche).

Types d'événements qui déclenchent une notification : `permission_prompt`, `idle_prompt`
(attente de réponse), `elicitation_dialog`, `agent_needs_input`, `agent_completed` (fin de
tâche), `auth_success`, etc. Le texte du message varie ; rien à exploiter côté Pupitre.

Séquences voisines émises par Claude Code (à **ignorer**, ne pas les traiter comme bulles) :

- progression ConEmu : `ESC ] 9 ; 4 ; <état> ; <pct> BEL`, état 0 effacer, 1 normal, 2 erreur,
  3 indéterminé (`ESC ] 9;4;3 BEL` sans pourcentage ; `ESC ] 9;4;0;` avec pourcentage vide à l'effacement),
  conditionnée chez Claude Code par le réglage `terminalProgressBarEnabled` (et la détection du terminal) ;
- OSC 7501 (`state=working:app=claude-code:...`, état de programme) et OSC 21337 (état d'onglet) :
  propriétaires, inconnus de Pupitre, à ignorer ;
- OSC 0/1/2 (titre) et OSC 8 (liens) habituels.

**Conséquence pour l'analyseur** : un `ESC ] 9 ;` dont le texte commence par `4;` (chiffre 4
suivi d'un point-virgule, ou `4` seul) est une progression : à ignorer. Tout autre OSC 9 est une
notification, et un texte comme `4 fichiers modifiés` (quatre suivi d'un espace) en est une
aussi : ne tester que `4;` et `4` exact. OSC 777 : ne traiter que `notify;titre;corps` (le corps
peut contenir des `;` : tout ce qui suit le deuxième `;`). Terminateur BEL ou ST (`ESC \`).
`osc.sh` émet toutes ces formes pour les essais de la Task suivante.

VTE 0.80 ne fait rien de ces séquences (essai de 2026-10-25) : c'est bien au processus de fond de
les intercepter.

## 2. Bulles (xfce4-notifyd)

`bulle.py` : `Gio.Application` d'identifiant `fr.pupitre.Pupitre` (aucun `.desktop` installé),
une `Gio.Notification` (titre « Essai Pupitre », corps « Essai Pupitre : notification », priorité
normale, action par défaut et bouton liés à `app.montrer-onglet` avec paramètre `u` 42),
`send_notification`, retrait après 8 s. Appel D-Bus relevé par `dbus-monitor` :

- GIO utilise **le backend `org.freedesktop.Notifications`** (pas de portail ici) :
  `Notify(app_name="python3", replaces_id=0, icon="", summary, body,
  actions=["default","", "app.montrer-onglet(uint32 42)", "Montrer"], hints={desktop-entry:
  "fr.pupitre.Pupitre", urgency: 1}, expire_timeout=-1)`. Sans `.desktop`, GIO retombe sur ce
  chemin et **le nom d'application affiché est le nom du programme** (`g_get_prgname`, `python3`
  ici ; pour Pupitre il faudra qu'il soit `pupitre`) ; la **clé `desktop-entry` reste l'identifiant**,
  et c'est elle qu'xfce4-notifyd enregistre dans sa liste d'applications connues
  (`/applications/known_applications` : `fr.pupitre.Pupitre` y est apparue, modification
  inévitable du profil du porteur, rien d'autre n'a été touché). Le nom d'action D-Bus est la
  forme détaillée `app.montrer-onglet(uint32 42)` : GIO la relit avec
  `g_action_parse_detailed_name` (vérifié : nom `app.montrer-onglet`, paramètre `u` 42). L'action
  `app.montrer-onglet` doit donc être déclarée avec le type `u` (`VariantType "u"`) ; un type
  différent (par exemple `i`) ne recevrait pas le paramètre.
- Capacités du serveur : `actions`, `body`, `body-markup`, `body-hyperlinks`, `icon-static`,
  `action-icons`, `x-canonical-private-icon-only` ; pas de `sound` ni `persistence` (convient :
  tout silencieux). Serveur : « Xfce Notify Daemon » 0.9.7, spec 1.2.
- `NotificationClosed(id 54, raison 1)` après ~3 s : raison 1 = expirée, donc affichée puis
  expirée par le démon (réglage `/expire-timeout = 3` du porteur), pas rejetée ni retenue.
- « Ne pas déranger » : xfce4-notifyd le range dans xfconf (`xfce4-notifyd` `/do-not-disturb`) ;
  la propriété **n'existe pas** chez le porteur (équivaut à désactivé). Pupitre n'a pas à le lire
  et ne le contourne pas (aucune urgence critique : l'urgence est bien « normale »). Le
  comportement avec le mode actif n'a pas été essayé (le porteur peut le faire en activant
  l'option dans le plugin de notifications du panneau puis en lançant `python3 -I
  essai-jetable/etape4/bulle.py`).

**À faire par le porteur** : lancer `python3 -I essai-jetable/etape4/bulle.py 30`, **cliquer** la
bulle (ou le bouton « Montrer ») dans les 3 s, et vérifier que la ligne
`ACTION app.montrer-onglet reçue, paramètre = uint32 42` s'affiche. (La bulle expirant en 3 s,
il peut falloir porter à 0 le délai dans les réglages d'xfce4-notifyd ou être rapide.) Le clic
n'a pas pu être automatisé : le signal `ActionInvoked` ne peut venir que du démon.

GNOME / KDE : non essayé (VM, Task 10).

## 3. Disposition du clavier (GDK 4.18)

`disposition.py` lit `Gdk.Display.get_default().get_default_seat().get_keyboard()` (sous GDK 4.18
l'API est sur `GdkDevice` : `get_layout_names()` -> `layout_names`, `get_active_layout_index()`,
propriété `has_bidi_layouts`, signaux `notify::layout-names` et `notify::active-layout-index` ;
**pas de méthode `get_has_bidi_layouts` dans les liaisons Python**, `props.has_bidi_layouts`).

- Poste du porteur (X11, :0.0, périphérique `Virtual core keyboard`, `X11DeviceXI2`) :
  `layout_names = ['French (Dvorak)']`, `active_layout_index = 0`, `has_bidi_layouts = False`.
  **Les noms sont les descriptions lisibles de xkb (anglais : « French (Dvorak) », « English
  (US) »), pas les codes `fr`/`us`** : à présenter tels quels ou à localiser via
  `pupitre-i18n`, jamais à comparer à une chaîne. Une seule disposition déclarée chez le porteur
  (les `grp:rctrl_rshift_toggle` n'ont donc rien à basculer ici).
- Xvfb de test (disposition `fr,us` demandée avec `setxkbmap`) : le serveur reste sur
  `['English (US)']` ; changer de groupe avec `xdotool key ISO_Next_Group` n'est pas suivi
  (connu). **Observation utile** : pendant ces essais, `notify::layout-names` est émis en
  **rafale** (des dizaines de fois en quelques secondes, valeur inchangée) à chaque événement
  XKB (touches synthétiques). Le gestionnaire doit donc **comparer à la valeur précédente** avant
  d'annoncer quoi que ce soit.
- Si `notify::active-layout-index` arrive réellement à un vrai changement de disposition : non
  observé, il n'y avait pas de seconde disposition ; **à faire par le porteur** : ajouter un
  seconde disposition (par exemple via le plugin Disposition clavier d'Xfce, ce que je ne
  fais pas) et lancer `python3 -I essai-jetable/etape4/disposition.py 30` pendant qu'il bascule.
- Weston 14.0.2 : `--backend=headless` (seul backend utilisable ici : pas de backend x11, pas de
  compositeur Wayland parent) n'a **aucun périphérique d'entrée**, donc
  `Gdk.Display.get_default_seat()` est **`None`** sous GDK Wayland (l'essai plante sur
  `None.get_keyboard()`). Conséquence de conception : prévoir `Option` (aucun clavier ->
  disposition inconnue, ne pas planter). Essai Wayland réel : VM (Task 10).

## 4. `toml_edit` 0.25.19

`toml_edit_essai` : lit un fichier, vérifie (a) `to_string()` sans modification, (b) le
changement de `theme` comparé à un remplacement textuel, (c) le retour à la valeur d'origine.

| Fichier | (a) sans modif. | (b) `theme` : `doc["theme"] = value(..)` | (b) avec la décoration recopiée | (c) retour |
|---|---|---|---|---|
| `assets/config/pupitre.toml` | identique | seule la valeur change | idem | identique |
| `annote.toml` (espaces irréguliers, commentaire de fin de ligne, table inconnue, tableau, chaîne littérale, accents, pas de retour final après un commentaire) | identique | **le commentaire de fin de ligne de `theme` est PERDU** (et les espaces qui le précèdent) | seule la valeur change | identique |
| `sans_newline.toml` (dernière ligne `theme = ...` sans retour à la ligne) | **un `\n` est ajouté** | idem | idem | idem |
| `crlf.toml` | **tous les `\r\n` deviennent `\n`, même sans modification** | idem | idem | idem |

Conclusions :

1. Remplacer une valeur par `value(..)` **efface la décoration de l'ancienne valeur** (espaces et
   commentaire de fin de ligne). Pour garder le fichier de l'utilisateur intact : copier
   `decor()` de l'ancienne valeur sur la nouvelle (`*nouvelle.decor_mut() = ancienne.decor().clone()`),
   comme fait dans l'essai. Sous cette condition, **aucune perte** sur les deux fichiers de test
   (commentaires, ordre, clés inconnues, tables inconnues, indentation, chaînes littérales).
2. Écarts à connaître, sans commentaire perdu : fin de fichier **sans retour à la ligne** -> un
   retour est ajouté ; fichier en **CRLF** -> converti en LF dès l'analyse. Acceptable à signaler
   ou à traiter (détecter `\r\n` dans la source et reconvertir la sortie) ; le fichier livré est
   en LF.
3. Le fichier livré (`pupitre.toml`) se rejoue sans perte, y compris la section `[[shells]]`
   commentée.
