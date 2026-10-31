# st (suckless simple terminal) — options, paramètres et features

Version de référence : **st 0.9.3**. Source : dépôt officiel `git.suckless.org/st`, commit `fdaadd5` (16/10/2026).

st **n'a pas de fichier de configuration** lu au lancement. On le règle en modifiant `config.h` (copie de `config.def.h`), puis en recompilant :

```sh
make clean install
```

Seules les options de ligne de commande changent quelque chose sans recompiler.

---

## 1. Options de ligne de commande

```
st [-aiv] [-c class] [-f font] [-g geometry] [-n name] [-o iofile]
   [-T title] [-t title] [-l line] [-w windowid] [[-e] command [args...]]

st [-aiv] [-c class] [-f font] [-g geometry] [-n name] [-o iofile]
   [-T title] [-t title] [-w windowid] -l line [stty_args...]
```

| Option | Effet |
|---|---|
| `-a` | Désactive l'écran alternatif (celui qu'utilisent vim, less…) |
| `-c class` | Classe de la fenêtre X (WM_CLASS). Par défaut : `$TERM` |
| `-f font` | Police, au format fontconfig (ex. `"Hack:pixelsize=14"`) |
| `-g geometry` | Taille et position : `[=][<cols>x<rows>][{+-}<x>{+-}<y>]` (voir `XParseGeometry(3)`) |
| `-i` | Fixe la position donnée par `-g` |
| `-n name` | Nom d'instance de la fenêtre. Par défaut : `$TERM` |
| `-o iofile` | Écrit toutes les entrées/sorties dans un fichier (`-` = stdout). Sert à enregistrer une session |
| `-T title` / `-t title` | Titre de la fenêtre (par défaut `st`) |
| `-w windowid` | Intègre st dans une autre fenêtre X (utilisé par `tabbed`) |
| `-l line` | Se connecte à une ligne série au lieu d'un pseudo-terminal (ex. `st -l /dev/ttyS0 115200`). Les arguments suivants sont passés à `stty` (ex. `st -l /dev/ttyS0 parenb parodd 115200`, `cs7`…). Par défaut : 8 bits, sans parité, 1 bit de stop, 38400 bauds |
| `-v` | Affiche la version |
| `-e command [args]` | Lance une commande au lieu du shell. Doit être la dernière option (les arguments restants sont de toute façon traités comme une commande) |

Ordre de choix du programme lancé :

1. commande passée avec `-e`
2. `scroll` et/ou `utmp` (si définis dans config.h)
3. variable `$SHELL`
4. shell de l'utilisateur dans `/etc/passwd`
5. `shell` dans config.h

---

## 2. Paramètres de `config.h` (compilation)

### Apparence

| Paramètre | Défaut | Rôle |
|---|---|---|
| `font` | `"Liberation Mono:pixelsize=12:antialias=true:autohint=true"` | Police |
| `borderpx` | `2` | Marge intérieure, en pixels |
| `cwscale` / `chscale` | `1.0` / `1.0` | Multiplicateurs de largeur et de hauteur des cellules (espacement) |
| `colorname[]` | 16 couleurs + entrées 256 à 259 | Palette : 8 normales, 8 vives, puis couleurs supplémentaires après l'index 255 (`#cccccc`, `#555555`, `gray90`, `black`) |
| `defaultfg` / `defaultbg` | `258` / `259` | Index des couleurs de texte et de fond |
| `defaultcs` / `defaultrcs` | `256` / `257` | Couleur du curseur et du curseur inversé |
| `defaultattr` | `11` | Couleur affichée quand fontconfig n'a pas trouvé la variante demandée (gras, italique) |
| `cursorshape` | `2` | Forme du curseur : 2 bloc █, 4 souligné _, 6 barre \|, 7 bonhomme de neige ☃ |
| `cursorthickness` | `2` | Épaisseur des curseurs souligné et barre |
| `cols` / `rows` | `80` / `24` | Taille par défaut |
| `mouseshape` | `XC_xterm` | Forme du pointeur de la souris |
| `mousefg` / `mousebg` | `7` / `0` | Couleurs du pointeur |
| `blinktimeout` | `800` ms | Période du clignotement (0 = désactivé) |

Palette par défaut (`colorname[]`) :

| Index | Normale | Index | Vive |
|---|---|---|---|
| 0 | black | 8 | gray50 |
| 1 | red3 | 9 | red |
| 2 | green3 | 10 | green |
| 3 | yellow3 | 11 | yellow |
| 4 | blue2 | 12 | #5c5cff |
| 5 | magenta3 | 13 | magenta |
| 6 | cyan3 | 14 | cyan |
| 7 | gray90 | 15 | white |

### Comportement

| Paramètre | Défaut | Rôle |
|---|---|---|
| `shell` | `"/bin/sh"` | Shell de dernier recours |
| `utmp` | `NULL` | Programme utmp à utiliser |
| `scroll` | `NULL` | Programme de défilement externe (ex. `"scroll"`) |
| `stty_args` | `"stty raw pass8 nl -echo -iexten -cstopb 38400"` | Réglages de la ligne série pour `-l` |
| `termname` | `"st-256color"` | Valeur de `$TERM` |
| `vtiden` | `"\033[?6c"` | Réponse d'identification (DA / DECID) |
| `tabspaces` | `8` | Largeur des tabulations (à accorder avec `it#` dans `st.info`, et vérifier `tab0` dans `stty -a`) |
| `worddelimiters` | `L" "` | Caractères qui coupent les mots au double-clic (exemple plus complet : ``L" `'\"()[]{}"``) |
| `doubleclicktimeout` | `300` ms | Délai du double-clic |
| `tripleclicktimeout` | `600` ms | Délai du triple-clic |
| `allowaltscreen` | `1` | Autorise l'écran alternatif |
| `allowwindowops` | `0` | Autorise des opérations peu sûres, comme l'écriture dans le presse-papiers via OSC 52 |
| `minlatency` / `maxlatency` | `2` / `33` ms | Plage de latence de l'affichage (compromis entre réactivité et scintillement) |
| `bellvolume` | `0` | Volume de la cloche, de -100 à 100 (0 = coupée) |

### Clavier et souris

| Paramètre | Rôle |
|---|---|
| `MODKEY` | `Mod1Mask` (Alt) |
| `TERMMOD` | `ControlMask\|ShiftMask` (Ctrl+Shift) |
| `shortcuts[]` | Raccourcis clavier internes (voir section 3) |
| `mshortcuts[]` | Raccourcis souris |
| `forcemousemod` | `ShiftMask` : maintenir Shift force la sélection quand une application capture la souris |
| `selmasks[]` | `[SEL_RECTANGULAR] = Mod1Mask` : Alt + glisser = sélection rectangulaire |
| `ignoremod` | `Mod2Mask\|XK_SWITCH_MOD` : modificateurs ignorés (Verr Num et changement de disposition clavier) |
| `mappedkeys[]` | Touches hors F-keys X11 (0xFD00–0xFFFF) à faire passer par la table `key[]` |
| `key[]` | Grande table touche + modificateur → séquence envoyée : flèches, Home/End, PgUp/PgDn, Insert/Delete, Backspace, Return, Shift+Tab, pavé numérique, F1 à F35 avec Shift/Ctrl/Alt/Super. Chaque entrée dépend des modes « keypad application » (`appkey`) et « cursor application » (`appcursor`). L'ordre compte : les entrées `XK_ANY_MOD` doivent être en dernier pour une touche |
| `ascii_printable` | Caractères servant à estimer la largeur d'une cellule |

---

## 3. Raccourcis par défaut

| Raccourci | Action |
|---|---|
| `Ctrl+Shift+C` | Copier dans le presse-papiers |
| `Ctrl+Shift+V` | Coller depuis le presse-papiers |
| `Ctrl+Shift+Y` / `Shift+Insert` / clic milieu | Coller la sélection primaire |
| `Ctrl+Shift+PgUp` / `Ctrl+Shift+PgDn` | Agrandir / réduire la police |
| `Ctrl+Shift+Home` | Taille de police par défaut |
| `Ctrl+Shift+Verr Num` | Basculer le mode du pavé numérique |
| `Impr` | Écrire la sélection dans l'iofile |
| `Shift+Impr` | Écrire tout l'écran dans l'iofile |
| `Ctrl+Impr` | Activer ou couper l'écriture continue dans l'iofile |
| `Break` (Ctrl+Pause) | Envoyer un break sur la ligne série |
| Molette | Envoie `Ctrl+Y` / `Ctrl+E` (défile dans less ou vim, **pas** dans st) |
| `Shift` + molette | Envoie PgUp / PgDn |
| Double / triple clic | Sélection d'un mot / d'une ligne |
| `Alt` + glisser | Sélection rectangulaire |

Fonctions disponibles pour ses propres raccourcis : `clipcopy`, `clippaste`, `selpaste`, `zoom`, `zoomreset`, `numlock`, `printscreen`, `printsel`, `toggleprinter`, `sendbreak`, `ttysend`.

---

## 4. Features (ce que le code gère)

### Rendu

- Xft, fontconfig et freetype : antialiasing, polices de repli automatiques (emoji, CJK…)
- Gras, italique, gras-italique, avec choix automatique de la variante de police
- Caractères larges (CJK) et UTF-8 complet
- 256 couleurs et **truecolor** 24 bits (`38;2;r;g;b` / `48;2;r;g;b`)
- Couleurs modifiables à la volée (OSC 4/10/11/12 et remises à zéro OSC 104/110/111/112)
- Zoom de la police pendant l'exécution
- Affichage adaptatif selon la latence, contre le scintillement

### Attributs de texte (SGR)

Gras (1), faible (2), italique (3), souligné (4), clignotant lent/rapide (5, 6), inversé (7), invisible (8), barré (9), et leurs annulations (22, 23, 24, 25, 27, 28, 29). Couleurs : 38/48 (indexée ou RGB), 39/49 (défaut). La couleur du soulignement (58) est lue mais ignorée.

### Séquences CSI

ICH, CUU, CUD, VPR, CUF, HPR, CUB, CNL, CPL, CHA, HPA, CUP, HVP, VPA, CHT, CBT, ED, EL, ECH, IL, DL, DCH, SU, SD, TBC, REP, MC (impression), DA, DSR (5 = état, 6 = position du curseur), SM/RM, SGR, DECSTBM, sauvegarde/restauration du curseur (`s`/`u`), DECSCUSR.

### Séquences ESC

IND, NEL, HTS, RI, DECID, RIS, DECPAM, DECPNM, DECSC, DECRC, ST, LS2, LS3, désignation des jeux G0 à G3 (`(`, `)`, `*`, `+`), `#` et `%`, DCS / APC / PM (lus puis ignorés), OSC, `ESC k` (titre, compatibilité).

### Modes DEC/ANSI

- DECCKM (touches curseur), DECSCNM (vidéo inversée), DECOM (origine), DECAWM (retour à la ligne auto), DECTCEM (afficher/masquer le curseur)
- IRM (insertion), SRM (écho local), LNM (saut de ligne)
- DECPAM/DECPNM (pavé numérique), DECSCUSR (forme du curseur), DECSTBM (zone de défilement)
- Mode 8 bits du clavier (1034)
- Ignorés : DECANM, DECCOLM, DECSCLM, DECARM, DECPFF, DECPEX, DECNRCM, clignotement du curseur (12)

### Écran alternatif

Modes 47, 1047, 1048 (sauvegarde du curseur), 1049.

### Souris

X10 (9), clics (1000), glisser (1002), tous les mouvements (1003), encodage SGR (1006). Non gérés : 1001, 1005, 1015.

### Autres

- Événements de focus (1004)
- **Bracketed paste** (2004)
- Titre de fenêtre (OSC 0/1/2 et `ESC k`)
- **Presse-papiers OSC 52** (seulement si `allowwindowops = 1`)
- Rapports d'état et de position du curseur (DSR 5/6), identification du terminal (DA, DECID)
- Jeux de caractères G0 à G3, avec graphismes DEC (dessin de cadres)
- Tabulations (HTS, TBC, CHT, CBT)
- Réinitialisation complète (RIS)

### Système

- Intégration dans d'autres fenêtres (`-w`, pour `tabbed`)
- Terminal série (`-l`)
- Enregistrement de session (`-o`) et impression vers l'iofile
- Compatible `utmp` et `scroll`
- Entrée terminfo fournie : `st.info`, à installer avec `tic -sx st.info`
- Dépendances de compilation : Xlib, Xft, fontconfig, freetype2

---

## 5. Ce que st n'a **pas** de base

C'est voulu par la philosophie suckless. Ces fonctions s'ajoutent avec des **patches** (<https://st.suckless.org/patches/>) ou des outils externes :

| Fonction absente | Solution |
|---|---|
| Historique de défilement (scrollback) | patches `scrollback`, `scrollback-mouse`, ou le programme `scroll` |
| Transparence | patch `alpha` |
| Configuration par Xresources | patch `xresources` |
| Ligatures | patch `ligatures` |
| Ouverture / copie des URL | patches `externalpipe`, `copyurl` |
| Onglets | programme `tabbed` |
| Multiplexage | tmux, screen, dvtm… |
| Double hauteur, diacritiques combinés | non disponibles (listés dans le TODO officiel) |

---

## Note sur cette machine

Deux versions de st y sont installées :

- **st 0.9.3** dans `/usr/local/bin/st`, compilée depuis les sources (celle qui est lancée par défaut)
- le paquet Debian **`stterm` 0.9**

Si la 0.9.3 locale a été compilée avec un `config.h` modifié ou des patches, ses valeurs peuvent différer de celles de ce document.
