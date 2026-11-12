# Aide de Pupitre

Pupitre est un terminal à onglets conçu pour être utilisable entièrement au clavier et avec
le lecteur d'écran Orca. Il fonctionne sous X11 et Wayland (XFCE, GNOME, KDE, Sway…).

Cette aide décrit tout ce que Pupitre sait faire. Les fonctions marquées **(à venir)** sont
prévues mais pas encore disponibles.

## Sommaire

1. Démarrer
2. Raccourcis clavier
3. Onglets et fenêtres
4. Se déplacer au clavier et avec Orca
5. Copier, coller, rechercher
6. Taille du texte et plein écran
7. Messages de Pupitre
8. Connexions SSH, phrases de passe et trousseau
9. Shells et invite
10. Fin des commandes et navigation entre commandes
11. Configuration : `pupitre.toml`
12. Thèmes et couleurs
13. Polices
14. Commandes en ligne de commande
15. Si quelque chose se passe mal
16. Vie privée
17. Fichiers utilisés par Pupitre
18. Prévu plus tard

---

## 1. Démarrer

- Lancez Pupitre depuis le menu des applications (catégorie « Système »), ou tapez
  `pupitre` dans un terminal.
- Si Pupitre est déjà ouvert, le relancer ouvre simplement une nouvelle fenêtre.
- À l'ouverture, la fenêtre occupe la moitié de la largeur de l'écran. Vous pouvez ensuite la
  redimensionner librement.
- Au premier lancement, Pupitre crée son fichier de configuration commenté :
  `~/.config/pupitre/pupitre.toml`.

## 2. Raccourcis clavier

Pupitre utilise Ctrl+Maj pour ses propres raccourcis. Ctrl seul reste aux programmes :
Ctrl+C interrompt une commande, Ctrl+W efface un mot, etc. Aucun raccourci n'entre en conflit
avec ceux d'Orca.

| Action | Raccourci |
|---|---|
| Nouvel onglet | Ctrl+Maj+T |
| Fermer l'onglet | Ctrl+Maj+W |
| Nouvelle fenêtre | Ctrl+Maj+N |
| Connexions SSH | Ctrl+Maj+O |
| Copier | Ctrl+Maj+C |
| Coller | Ctrl+Maj+V |
| Rechercher dans l'historique | Ctrl+Maj+F |
| Onglet suivant | Ctrl+Page suivante |
| Onglet précédent | Ctrl+Page précédente |
| Aller à l'onglet 1 à 9 | Alt+1 … Alt+9 |
| Agrandir le texte | Ctrl+plus |
| Réduire le texte | Ctrl+moins |
| Taille normale | Ctrl+0 |
| Plein écran | F11 |
| Quitter le terminal vers l'élément suivant | Ctrl+Tab |
| Revenir à l'élément précédent | Ctrl+Maj+Tab |
| Commande précédente | Ctrl+Maj+↑ |
| Commande suivante | Ctrl+Maj+↓ |
| Ouvrir cette aide | Ctrl+Maj+H |

Les raccourcis suivent la lettre de votre disposition de clavier : Ctrl+Maj+T fonctionne là
où se trouve la lettre T, en AZERTY, BÉPO ou Dvorak. Pour Alt+1…9 et Ctrl+0, c'est la
position de la touche qui compte : la rangée des chiffres, même si elle produit d'autres
caractères sans Maj. Sur les dispositions où « + » demande Maj, Ctrl+Maj+plus fonctionne
aussi.

Touches laissées volontairement aux programmes :
- **Tab** reste au shell (complétion) ;
- **F1** et **F10** restent aux programmes (aide et sortie de mc, htop…) ;
- **Ctrl+lettre** reste aux programmes.

## 3. Onglets et fenêtres

- **Ouvrir** : Ctrl+Maj+T ou menu ▸ « Nouvel onglet ». Le nouvel onglet reprend le shell de
  l'onglet actif (fish depuis un onglet fish) ; depuis une connexion SSH, ou dans une nouvelle
  fenêtre, il ouvre le shell des réglages.
- **Ouvrir avec un autre shell** : menu ▸ « Nouvel onglet avec », puis le shell voulu (voir la
  section 9).
- **Renommer** : double-clic sur le nom de l'onglet, ou clic droit ▸ « Renommer l'onglet »,
  ou menu ▸ « Renommer l'onglet ». Laisser le nom vide revient au titre automatique.
- **Titre automatique** : le titre envoyé par le programme en cours, sinon le dossier courant.
- **Réorganiser** : glisser l'onglet avec la souris.
- **Détacher dans une nouvelle fenêtre** : clic droit sur l'onglet, ou menu ▸ « Détacher dans
  une nouvelle fenêtre ». On peut aussi faire glisser l'onglet hors de la fenêtre.
- **Fermer** : Ctrl+Maj+W. Si une commande tourne encore, Pupitre demande confirmation :
  « Une commande est en cours dans cet onglet. Fermer quand même ? ». « Annuler » est le
  choix par défaut, Échap aussi annule. Fermer une fenêtre pose la même question pour
  l'ensemble de ses onglets.
- **Shell terminé** : quand vous tapez `exit`, l'onglet reste ouvert avec le message « Le shell
  s'est terminé (code N). Appuyez sur Entrée pour en ouvrir un nouveau. »
- **Cloche** : quand un programme fait sonner la cloche, Pupitre ne fait aucun bruit. L'écran
  clignote brièvement (au plus 3 fois par seconde ; remplacé par un simple marqueur si le
  bureau demande de réduire les animations). Dans un onglet caché, le nom de l'onglet reçoit
  le marqueur ● et Orca le lit « titre, cloche ».

## 4. Se déplacer au clavier et avec Orca

Tout Pupitre s'utilise au clavier.

- **Sortir du terminal** : Ctrl+Tab passe à l'élément suivant, Ctrl+Maj+Tab au précédent. Le
  tour comprend, selon ce qui est affiché : le bandeau de messages, la barre de recherche, la
  barre d'onglets, le bouton de menu, puis le terminal.
- **Ouvrir le menu** : Ctrl+Tab jusqu'au bouton « Menu », puis Entrée.
- **Aide** : Ctrl+Maj+H, ou menu ▸ « Aide », ouvre cette aide dans une fenêtre « Aide de
  Pupitre » ; elle est comprise dans Pupitre et marche sans réseau. À gauche, la liste des
  sections et le champ « Rechercher dans l'aide » ; à droite, le texte de la section choisie,
  que vous lisez avec le curseur comme n'importe quel texte. Ctrl+Tab passe d'une zone à
  l'autre, Échap ferme la fenêtre. `pupitre aide` affiche la même aide dans le terminal.
- **Orca** lit le texte du terminal, le nom des onglets, les menus, les dialogues et chaque
  message de Pupitre. Quand le focus arrive sur la barre d'onglets, Orca indique aussi les
  onglets où la cloche a sonné.
- **Dialogues** : Échap annule toujours ; le bouton par défaut est le choix sans risque.
- **Focus visible** : l'élément actif est toujours encadré.
- Les informations ne passent jamais par un seul canal : une couleur ou un symbole est
  toujours doublé d'un texte lu par Orca.

## 5. Copier, coller, rechercher

### Copier et coller
- **Copier** la sélection : Ctrl+Maj+C.
- **Coller** : Ctrl+Maj+V. Le collage est sécurisé : un texte collé ne s'exécute pas tout seul,
  même s'il contient un retour à la ligne, dans les shells qui le permettent.
- **Sélection primaire** : sélectionner du texte puis cliquer avec le bouton du milieu le
  colle, comme partout sous Linux.
- **Copie automatique** : du texte sélectionné à la souris est copié dans le presse-papiers
  dès que le bouton est relâché ; Ctrl+Maj+V le colle. Pour la désactiver :
  `copie_selection = false` dans `pupitre.toml`. La recherche ne touche pas au presse-papiers.
  Attention : un mot de passe ou un secret sélectionné à la souris est copié lui aussi, et
  d'autres applications peuvent lire le presse-papiers.

### Liens
Les adresses web écrites dans le terminal sont reconnues. **Ctrl+clic** les ouvre dans votre
navigateur. Pupitre ne les ouvre jamais de lui-même.

### Rechercher dans l'historique
- Ctrl+Maj+F ouvre la barre de recherche au-dessus du terminal.
- Tapez le texte cherché ; la case « Expression régulière » permet une recherche avancée.
- **Entrée** : occurrence précédente (en remontant). **Maj+Entrée** : occurrence suivante.
  Les boutons « Précédent » et « Suivant » font la même chose.
- **Échap** ferme la recherche et revient au terminal.
- Une expression régulière incorrecte est signalée dans l'infobulle du champ.

### Historique
Chaque onglet garde les 10 000 dernières lignes, uniquement en mémoire (réglable par
`historique_lignes`). Rien n'est écrit sur le disque.

## 6. Taille du texte et plein écran

- **Ctrl+plus** agrandit, **Ctrl+moins** réduit, **Ctrl+0** revient à la taille normale.
  Également dans le menu.
- Le zoom va jusqu'à 200 % par défaut (réglable jusqu'à 500 % avec `zoom_max_pourcent`).
- **F11** passe en plein écran et en sort.

## 7. Messages de Pupitre

Les messages s'affichent dans un bandeau entre la barre d'onglets et le terminal. Chaque
message est lu par Orca dès son apparition.

- Un message **temporaire** disparaît seul après quelques secondes.
- Un message **persistant** (par exemple une erreur de configuration) reste jusqu'à ce que le
  problème soit corrigé, ou jusqu'à ce que vous le fermiez avec son bouton « Fermer ce
  message ».
- Pour y accéder au clavier : Ctrl+Tab depuis le terminal.

## 8. Connexions SSH, phrases de passe et trousseau

### La fenêtre « Connexions SSH »
Ctrl+Maj+O, ou menu ▸ « Connexions SSH… ».

La liste montre d'abord les connexions enregistrées dans Pupitre, puis les serveurs décrits
dans votre `~/.ssh/config` (en lecture seule : Pupitre ne modifie jamais ce fichier).

- **Se connecter** (ou Entrée sur une ligne) : ouvre un nouvel onglet qui lance `ssh` vers ce
  serveur. Pupitre utilise OpenSSH tel quel.
- **Ajouter…** : nom, hôte, utilisateur (facultatif), port (facultatif), clé (facultatif).
- **Modifier…** et **Supprimer** : seulement pour les connexions enregistrées dans Pupitre.
  La suppression demande confirmation.
- **Fermer** ou Échap ferme la fenêtre.

Les connexions sont enregistrées dans `~/.config/pupitre/connexions.toml`. Ce fichier peut aussi
être modifié à la main :

```toml
[[connexion]]
nom = "Serveur du bureau"
hote = "serveur.exemple.org"
utilisateur = "moi"
port = 2222
cle = "~/.ssh/id_ed25519"
```

Un hôte ou un utilisateur ne peut pas commencer par « - » ni contenir d'espace : c'est une
protection contre les options cachées.

### L'agent SSH
Dans Pupitre, un agent SSH est toujours disponible : Pupitre utilise celui de votre session
s'il fonctionne, sinon il retrouve un agent à vous déjà en marche, sinon il en lance un. Tous
les shells de Pupitre en profitent, `git` compris. L'environnement de votre bureau n'est
jamais modifié : un `git` lancé hors de Pupitre n'est pas concerné.

Si aucun agent n'est possible, Pupitre l'indique : « Agent SSH indisponible : les phrases de
passe seront demandées à chaque connexion. »

### Les phrases de passe
Quand une clé SSH demande sa phrase de passe (par `ssh`, `git fetch`, `git push`…), Pupitre
ouvre une petite fenêtre accessible au lieu de la question en anglais dans le terminal.

- Tapez la phrase, puis Entrée ou « Valider ».
- La case **« Mémoriser dans le trousseau »** est cochée quand un trousseau est disponible.
  La phrase est alors rangée dans le trousseau de votre bureau (GNOME Keyring, KWallet… via le
  Secret Service), jamais dans un fichier. Les fois suivantes, la clé est chargée toute seule
  dans l'agent : plus aucune question.
- Échap refuse, sans rien mémoriser.
- Une phrase incorrecte est redemandée : « Phrase incorrecte. … ».

Messages possibles :
- « Trousseau verrouillé : la phrase ne peut pas être mémorisée. » — déverrouillez votre
  trousseau (en général à l'ouverture de session) ;
- « Aucun trousseau disponible : la phrase sera demandée à chaque fois. » ;
- « Le trousseau ne répond pas pour l'instant : la phrase ne peut pas être mémorisée cette
  fois. » — la connexion fonctionne, la phrase sera redemandée la prochaine fois ;
- « La phrase de passe mémorisée pour … ne fonctionne plus ; elle a été oubliée. Elle sera
  redemandée. » — par exemple après un changement de phrase de passe.

Pour faire oublier une phrase mémorisée : ouvrez le gestionnaire de votre trousseau
(« Mots de passe et clés » / Seahorse sous GNOME et XFCE, KWalletManager sous KDE) et supprimez
l'entrée « Pupitre — phrase de passe de … ».

### Autres questions de SSH
- **Serveur inconnu** : à la première connexion, OpenSSH demande si vous faites confiance au
  serveur. Pupitre affiche la question avec l'empreinte ; choisissez « Faire confiance et se
  connecter » ou « Refuser ». La vérification reste celle d'OpenSSH.
- **Confirmer l'usage de la clé** : pour une clé ajoutée avec confirmation, « Autoriser » ou
  « Refuser ».
- **Autres questions** (mot de passe d'un serveur, identifiant git en HTTPS…) : une fenêtre
  « Question de SSH » avec un champ de saisie.

## 9. Shells et invite

- **Shell utilisé** : par défaut, votre shell de connexion. Le réglage `shell` permet d'en
  choisir un autre (`shell = "fish"`).
- **Menu « Nouvel onglet avec »** : liste les shells installés, ainsi que les commandes que
  vous déclarez vous-même (section `[[shells]]` de `pupitre.toml`).
- **Shells pris en charge** :
  - complètement : bash, zsh, fish, nushell (invite, fin de commande, dossier courant) ;
  - invite seulement : elvish, xonsh, tcsh, ion, PowerShell ;
  - tout autre shell : lancé tel quel.
- **Vos fichiers ne sont jamais modifiés** : `~/.bashrc`, `~/.zshrc`, la configuration de fish
  et de nushell restent intacts. Pupitre les charge d'abord, puis ajoute son intégration.
- **Invite** : si Starship est installé (`sudo apt install starship`), Pupitre affiche son
  invite sur deux lignes :

  ```
  14:02:37 charge 0.42 git:main Δ●○
  ⌁⌁⌁utilisateur@machine ~/Projets/pupitre λ
  ```

  Première ligne : heure, charge de la machine, branche git et son état (Δ modifié,
  ● indexé, ○ non suivi). Deuxième ligne : ⌁⌁⌁ si vous êtes connecté par SSH, ⚙⚙⚙ sur un
  serveur de production, puis utilisateur, machine, dossier et le symbole λ (rouge si la
  commande précédente a échoué, sauf avec elvish ; `#` pour root, `»` en SSH).
- **Modifier l'invite** : éditez `~/.config/pupitre/starship.toml`, au format de Starship.
  Ce fichier est créé au premier lancement et n'est jamais écrasé. Votre
  `~/.config/starship.toml` n'est pas utilisé par Pupitre, ce qui laisse intacts vos autres
  terminaux. Utilisez des noms de couleurs (`green`, `red`, `blue`…) pour que l'invite suive
  le thème et garde un bon contraste.
- **Garder votre propre invite** : `invite = "shell"` dans `pupitre.toml`.
- **Sans Starship**, chaque shell garde votre invite habituelle ; un message unique indique
  comment installer Starship.
- **Shell absent** : l'onglet s'ouvre avec votre shell de connexion et un message donne la
  commande d'installation, par exemple « sudo apt install fish ».

## 10. Fin des commandes et navigation entre commandes

Ces fonctions marchent avec bash, zsh, fish et nushell.

### Annonce de la fin d'une commande
Quand une commande dure **5 secondes ou plus**, Orca annonce sa fin et son résultat :
- « Commande terminée en 12 s. » si elle a réussi ;
- « Commande en échec, code 2, en 12 s. » si elle a échoué ;
- au-delà d'une minute : « … en 2 min 5 s » (« en 2 min » pile) ;
- si le shell ne donne pas la durée : « Commande terminée. » ou « Commande en échec, code 2. »
  (avec le réglage `"toutes"` seulement : sans durée, une commande n'est jamais « longue ») ;
- avec `"toutes"`, une commande de moins d'une seconde : « Commande terminée en moins d'une
  seconde. »

Les commandes plus courtes (`ls`, `cd`…) ne sont pas annoncées : Orca lit déjà ce qu'elles
affichent, et une annonce après chaque commande deviendrait pénible.

Réglage `annonce_fin_commande` dans `pupitre.toml` :
- `"longues"` (par défaut) : commandes de 5 secondes ou plus ;
- `"toutes"` : chaque commande ;
- `"aucune"` : jamais.

Pas d'annonce pour une commande qui se termine dans un onglet caché ou dans une fenêtre qui
n'est pas la fenêtre active, ni pour les commandes déjà terminées quand Pupitre est rouvert
après un incident. L'annonce est « polie » : Orca la dit sans couper ce qu'il est en train de
lire.

### Passer d'une commande à l'autre
- **Ctrl+Maj+↑** : commande précédente. **Ctrl+Maj+↓** : commande suivante.
- Le terminal défile pour mettre la ligne de l'invite de la commande en haut, et Orca lit :
  « Commande : git fetch », suivi de « , en échec, code 1 » si elle a échoué. Le texte lu est
  celui que vous avez tapé, sans l'invite. Si le shell ne signale pas où finit l'invite, la
  ligne entière est lue, invite comprise. Une commande vide est annoncée « Commande vide. »
- Au bout de la liste : « Aucune commande avant. » ou « Aucune commande après. »
- La navigation vise les commandes déjà lancées, pas l'invite du bas, où vous êtes déjà ; une
  commande sortie de l'historique du terminal (voir `historique_lignes`) est oubliée. Les
  commandes restent retrouvées après un changement de largeur de la fenêtre ou de taille du
  texte.
- Quand un programme plein écran est affiché (`vim`, `less`, `htop`, `mc`…), Ctrl+Maj+↑ et
  Ctrl+Maj+↓ n'envoient rien au programme et Orca annonce « Navigation indisponible pendant
  un programme plein écran. » Quittez le programme pour naviguer de nouveau.
- Si un programme est en train d'écrire au moment de la touche, la navigation attend un
  instant (au plus une seconde) un endroit sûr de sa sortie. Si elle ne peut pas se faire
  (par exemple un programme laisse une séquence de contrôle inachevée), Orca annonce
  « Navigation impossible pour l'instant. » : réessayez un peu plus tard.
- Un redimensionnement de la fenêtre ou un changement de taille du texte retient l'affichage
  de la sortie des programmes environ 200 millisecondes après la fin du zoom ou du
  redimensionnement (jusqu'à 2 secondes pendant un redimensionnement continu), le temps de
  retrouver les lignes des commandes ; rien n'est perdu.
- Après une reprise (Pupitre rouvert), les commandes déjà faites servent à la navigation,
  mais seulement celles que le processus de fond a retenues ; si une commande manque, la
  navigation passe simplement à la voisine.
- Les flèches seules (↑ et ↓) restent au shell et aux programmes : dans le shell, elles
  rappellent les commandes déjà tapées ; dans `less`, `htop`, `vim`…, elles déplacent le
  curseur.

## 11. Configuration : `pupitre.toml`

Le fichier `~/.config/pupitre/pupitre.toml` est créé au premier lancement, avec un commentaire
pour chaque réglage. Il se modifie avec n'importe quel éditeur de texte.

**Menu ▸ Préférences** ouvre directement les fichiers dans votre éditeur de texte :
« Modifier la configuration… » (`pupitre.toml`), « Modifier l'invite… » (`starship.toml`),
« Modifier les connexions… » (`connexions.toml`). Si aucune application n'est associée aux
fichiers `.toml` sur votre système, Pupitre prend votre éditeur de texte par défaut (celui des
fichiers `.txt`), sans vous demander lequel utiliser. Un fichier absent est d'abord créé, avec des
commentaires en français ; un fichier existant n'est jamais modifié. Pupitre annonce « Les
modifications s'appliquent dès l'enregistrement ». Si aucun éditeur ne s'ouvre, Pupitre affiche
un message avec le chemin complet du fichier, à ouvrir vous-même.

- **Pas besoin de redémarrer** : chaque enregistrement est appliqué aussitôt à toutes les
  fenêtres.
- **En cas d'erreur**, Pupitre garde la configuration précédente et affiche un message avec la
  ligne fautive et la raison. Il ne plante jamais à cause du fichier.
- Supprimer une ligne revient à la valeur par défaut.
- `pupitre check` vérifie le fichier sans ouvrir de fenêtre.

| Réglage | Par défaut | Valeurs |
|---|---|---|
| `police` | `"B612 Mono 11"` | nom de police et taille, ex. `"Iosevka 13"` |
| `theme` | `"systeme"` | `"systeme"`, `"sombre"`, `"clair"`, ou chemin d'un fichier de thème |
| `curseur_clignote` | `false` | `true` ou `false` |
| `historique_lignes` | `10000` | de 0 à 1 000 000 |
| `zoom_max_pourcent` | `200` | de 100 à 500 |
| `shell` | `""` (shell de connexion) | nom ou chemin, ex. `"fish"` |
| `invite` | `"starship"` | `"starship"` ou `"shell"` |
| `annonce_fin_commande` | `"longues"` | `"longues"`, `"toutes"`, `"aucune"` |
| `copie_selection` | `true` | `true` ou `false` |
| `langue` (à venir) | `""` (langue du système) | `"fr"`, `"en"`, `"es"`, `"it"`, `"de"`, `"nl"` ; appliqué au prochain lancement |

Exemple de shells déclarés (le premier élément de `commande` est le programme, sans `-` au
début) :

```toml
[[shells]]
nom = "Serveur"
commande = ["ssh", "serveur"]
```

Le curseur est un bloc, fixe par défaut. S'il est réglé pour clignoter, il reste fixe quand le
bureau demande de réduire les animations.

## 12. Thèmes et couleurs

- `theme = "systeme"` suit le réglage clair ou sombre de votre bureau, en direct.
- `"sombre"` : thème sombre (Catppuccin Mocha ajusté). `"clair"` : thème clair (Catppuccin
  Latte ajusté).
- **Contraste garanti** : les 16 couleurs de chaque thème atteignent au moins 4,5:1 (niveau
  WCAG AA, exigé par le RGAA) sur leur fond.
- **Texte atténué** : si un programme demande du texte atténué et que cela ferait passer le
  contraste sous 4,5:1, Pupitre l'affiche dans sa couleur normale.

### Créer son thème
Créez un fichier, par exemple `~/.config/pupitre/themes/mon-theme.toml`, puis indiquez son
chemin : `theme = "~/.config/pupitre/themes/mon-theme.toml"`. Tout est facultatif : le thème
part d'un thème intégré et ne change que ce que vous indiquez.

```toml
nom = "mon thème"          # sinon : nom du fichier
base = "sombre"            # ou "clair"
fond = "#1e1e2e"
texte = "#cdd6f4"
curseur = "#f5e0dc"
selection_fond = "#45475a"
selection_texte = "#cdd6f4"
palette = ["#...", …]      # exactement 16 couleurs
[couleurs]                 # ou couleur par couleur
8 = "#8a8ea6"
[roles]                    # usage d'une couleur : "texte", "fond", "texte et fond"
7 = "fond"
```

Le thème est relu à chaque enregistrement. Pupitre vérifie son contraste : un problème donne
un avertissement précis (couleurs, contraste obtenu, minimum), mais le thème est quand même
appliqué. `pupitre check --theme CHEMIN` fait la même vérification sans ouvrir de fenêtre.

## 13. Polices

- Par défaut : **B612 Mono**, 11 points, fournie avec Pupitre.
- Aussi fournie : **Atkinson Hyperlegible Mono**, dessinée pour les personnes malvoyantes :
  `police = "Atkinson Hyperlegible Mono 12"`.
- Toute police à chasse fixe installée sur votre système peut servir : indiquez son nom et sa
  taille dans `police`, par exemple `police = "Iosevka 13"`. Une police installée pendant que
  Pupitre est ouvert est prise en compte sans redémarrer.
- Si la police demandée est introuvable : « Police « … » introuvable : B612 Mono utilisée. »
- Pupitre ne télécharge jamais de police.

## 14. Commandes en ligne de commande

| Commande | Effet |
|---|---|
| `pupitre` | ouvre une fenêtre (dans le Pupitre déjà lancé s'il y en a un) |
| `pupitre check` | vérifie la configuration et le contraste des thèmes |
| `pupitre check --theme CHEMIN` | vérifie un fichier de thème |
| `pupitre aide`, `pupitre help` | affiche cette aide complète dans le terminal |
| `pupitre --help`, `pupitre -h` | affiche le résumé des commandes |
| `pupitre shells` | liste les shells installés, leur niveau de prise en charge, et la commande d'installation de ceux qui manquent |

Codes de sortie de `pupitre check` : 0 aucun problème, 1 problème trouvé, 2 commande mal
formée.

## 15. Si quelque chose se passe mal

Pupitre est fait de deux programmes : la fenêtre (`pupitre`) et un processus de fond
(`pupitre-fond`) qui fait tourner les shells.

- **Si la fenêtre plante**, vos shells continuent de tourner. Relancez Pupitre : il rouvre
  vos fenêtres et onglets et réaffiche leur contenu récent.
- **Si le processus de fond s'arrête**, les shells ouverts sont perdus. Pupitre l'indique —
  « Le processus de fond s'est arrêté : les shells ouverts sont perdus. » — et propose le
  bouton « Ouvrir un nouveau terminal ».
- **Le shell ne démarre pas** : « Le shell n'a pas pu être lancé : … », avec la raison.
- **Configuration ou thème invalide** : la configuration précédente est gardée, la ligne
  fautive est indiquée.
- Après un redémarrage de l'ordinateur, Pupitre repart de zéro : rien n'est gardé sur le
  disque.

## 16. Vie privée

- Pupitre ne se connecte jamais à Internet de lui-même : pas de statistiques, pas de
  vérification de mise à jour, pas de rapport de plantage.
- L'historique des terminaux et les sessions ne sont jamais écrits sur le disque.
- Les phrases de passe ne sont jamais écrites en clair : seulement dans le trousseau, et
  seulement si vous l'avez demandé.
- Les liens ne s'ouvrent que sur votre Ctrl+clic.

## 17. Fichiers utilisés par Pupitre

| Fichier | Rôle |
|---|---|
| `~/.config/pupitre/pupitre.toml` | configuration |
| `~/.config/pupitre/connexions.toml` | connexions SSH enregistrées dans Pupitre |
| `~/.config/pupitre/themes/*.toml` | vos thèmes (emplacement conseillé) |
| `~/.config/pupitre/starship.toml` | invite |
| `~/.ssh/config` | lu seulement, jamais modifié |
| `/run/user/<numéro>/pupitre/` | communication entre la fenêtre et le processus de fond (effacé à l'arrêt) |

Les fichiers de configuration sont enregistrés de façon sûre : une coupure pendant
l'enregistrement ne laisse jamais un fichier à moitié écrit.

## 18. Prévu plus tard

- Coche ✓ ou ✗ sur l'onglet et notification du bureau quand une commande se termine dans un
  onglet caché.
- Bandeau indiquant la disposition du clavier, avec annonce de chaque changement.
- Thèmes à contraste élevé (7:1), sombre et clair.
- Copie dans le presse-papiers par un programme (avec message « Un programme a copié N
  caractères »).
- Notifications envoyées par les programmes.
- Raccourcis modifiables.
- Fenêtre de réglages (police, taille, thème, shell, invite, annonces), accessible au
  clavier et avec Orca.
- Paquet `.deb`.
- Pupitre en anglais, espagnol, italien, allemand et néerlandais, selon la langue du système.
