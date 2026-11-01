# Étape 3, Task 0 : shells, Starship et OSC 133 réels

Mesures refaites par `./essai.sh` (Python, PTY réel, `HOME` et `XDG_*` temporaires supprimés à la fin ; le vrai `HOME` n'est jamais lu). Poste : bash 5.2.37, zsh 5.9, **fish 4.0.2**, **nushell 0.116.1**, elvish 0.21.0, Starship 1.22.1 (`/usr/bin/starship`). **xonsh et tcsh : non testés, absents** (pas installés, pas de sudo).

Notation : ESC = `^[`, BEL = `^G`. Les extraits sont ceux de `essai.sh`.

## Verdict global

| Critère du brief | Résultat |
|---|---|
| bash : config utilisateur puis la nôtre, sans écrire dans HOME | RÉUSSI |
| zsh : idem | RÉUSSI |
| fish : idem | RÉUSSI |
| nushell : idem | RÉUSSI |
| Starship n'ouvre pas `~/.config/starship.toml` quand `STARSHIP_CONFIG` est posé | RÉUSSI |

Aucun échec : pas de point de décision à soumettre. Points d'attention (section « Pièges ») à reporter dans les tâches suivantes.

## Synthèse : qui émet déjà de l'OSC 133 ?

| Shell | OSC 133 natif | OSC 7 natif | À poser par Pupitre |
|---|---|---|---|
| bash | non | non | A, B, C, D, OSC 7 |
| zsh | non | non | A, B, C, D, OSC 7 |
| fish 4.0.2 | **oui : A, C, D** (pas de B) | oui | rien (voire B, voir pièges) |
| nushell 0.116.1 | **oui : A, B, C, D** (+ P) | oui | rien (juste s'assurer que `osc133`/`osc7` sont à `true`) |
| elvish 0.21 | non | non | A, C, D, OSC 7 ; **B impossible** |
| Starship 1.22.1 | **non** (aucun « 133 » dans aucun `init`) | non | – |

Ne pas doubler : ne PAS injecter nos marques dans fish ni nushell ; Starship ne gêne pas (il n'en émet pas).

## 1. bash : RÉUSSI

Commande : `bash --rcfile <pupitre.bash> -i` (avec `HOME` = dossier de l'utilisateur).

- `/etc/bash.bashrc` **est lu** même avec `--rcfile` (strace : `openat("/etc/bash.bashrc")` puis le fichier `--rcfile`). Ensuite seul notre fichier est lu : `~/.bashrc` n'est lu que si notre fichier fait `. "$HOME/.bashrc"`. Mesuré : `VBR=1 PC=1`, donc `VU_BASHRC` posé et `PROMPT_COMMAND` de l'utilisateur exécuté.
- `PS0` est développé avant chaque commande (la marque C sort à chaque exécution, même pour `exit`).
- Fichiers créés dans HOME : seulement `.bash_history` (comportement normal de bash). Nos fichiers vivent ailleurs.

Octets observés (sans Starship, `PS1="USERPS1> "` de l'utilisateur) :

```
^[]7;file://B612/tmp^G^[]133;A^GUSERPS1> ^[]133;B^Gecho ...
^[]133;C^GVBR=1 PC=1 PS0=[set]
^[]133;D;0^G^[]7;file://B612/tmp^G^[]133;A^GUSERPS1> ^[]133;B^Gfalse
^[]133;C^G^[]133;D;1^G^[]7;file://B612/tmp^G^[]133;A^GUSERPS1> ^[]133;B^G...
```

Fichier d'intégration qui marche (squelette) :

```bash
[ -r "$HOME/.bashrc" ] && . "$HOME/.bashrc"
# (ici : eval "$(starship init bash --print-full-init)" si voulu)
__p_avant(){ local c=$?; [ -n "${__p_dej:-}" ] && printf '\e]133;D;%s\a' "$c"; __p_dej=1
  printf '\e]7;file://%s%s\a' "$HOSTNAME" "$PWD"; return $c; }
__p_B='\[\e]133;B\a\]'
__p_apres(){ local c=$?; PS1="${PS1//"$__p_B"/}$__p_B"; printf '\e]133;A\a'; return $c; }
PROMPT_COMMAND=(__p_avant "${PROMPT_COMMAND[@]}" __p_apres)
PS0="${PS0-}"$'\e]133;C\a'
```

Avec Starship (`STAR> ` dans le test) : mêmes marques, correctement ordonnées (A, `STAR> `, B, C, D;code).

## 2. zsh : RÉUSSI

Commande : `ZDOTDIR=<dossier pupitre> zsh -i` (aussi `-l`). Notre dossier contient `.zshenv`, `.zprofile`, `.zshrc`, `.zlogin` qui, chacun, remettent temporairement `ZDOTDIR` à la valeur d'origine (variable `PUPITRE_ZDOTDIR_ORIG` si l'utilisateur en avait une, sinon `unset ZDOTDIR` → `$HOME`), sourcent le fichier de l'utilisateur du même nom, puis remettent le nôtre. Notre `.zshrc` ajoute les marques puis **rend `ZDOTDIR` à l'origine** (les processus fils ne voient pas notre dossier).

- Mesuré : `ZE=1 ZR=1 PC=1 ZD=unset` : `.zshenv` et `.zshrc` de l'utilisateur chargés (`VU_ZSHRC=1`), son `precmd` exécuté. Avec un `ZDOTDIR` d'utilisateur : `ZD=<son dossier>` à la fin. Le mode login (`-l`) donne le même résultat.
- `precmd_functions` et `preexec_functions` : disponibles (`npre=1 nexec=1` ; 2 et 2 avec Starship qui s'y ajoute).
- Rien n'est écrit dans HOME (aucun fichier créé hors de ceux du test).

Octets :

```
^[]7;file://B612/tmp^G^[]133;A^GUSERPS> ^[]133;B^Gecho ...
^[]133;C^GZE=1 ZR=1 PC=1 ZD=unset npre=1 nexec=1
^[]133;D;0^G^[]7;file://B612/tmp^G^[]133;A^GUSERPS> ^[]133;B^G...
```

Pièges zsh : (1) la marque B s'ajoute à `PROMPT` avec un vrai ESC (`%{"$'\e]133;B\a'"%}`) ; `\e` seul dans `PROMPT` s'affiche littéralement ; (2) il ne faut PAS faire `unset ZDOTDIR` dans notre `.zshenv` sans le remettre, sinon zsh ne lit plus notre `.zshrc` (constaté lors de l'essai) ; (3) zsh écrit sa ligne `%` (PROMPT_EOL_MARK) avant les prompts : sans rapport avec nous.

## 3. fish 4.0.2 : RÉUSSI

- **OSC 133 émis nativement** : A (`133;A;special_key=1`), C (`133;C;cmdline_url=<commande encodée>`), D (`133;D;<code>`), plus OSC 7 et OSC 0 (titre). **Pas de B.** Extrait :

```
^[]7;file://B612/tmp^G ^[]133;A;special_key=1^G ... ^[]133;C;cmdline_url=false^G ... ^[]133;D;1^G
```

- **`-C` s'exécute après `config.fish`** : `fish -i -C 'set -gx VU_AFTER $VU_FISH'` donne `VF=1 AFTER=1` (`VU_FISH` posé par `config.fish` de l'utilisateur, déjà visible quand `-C` s'exécute). Les marques natives restent inchangées avec `-C` ; avec `-C 'starship init fish | source'` aussi (`STAR> `, pas de doublon).
- Ce que fish écrit dans HOME : `fish_history`, `fish_variables`, et surtout `.cache/fish/generated_completions/` (825 fichiers en une session, sous `XDG_CACHE_HOME`) : comportement de fish, pas le nôtre.
- Usage prévu : `fish -C 'source <fichier pupitre.fish>'` seulement si on a quelque chose à ajouter (Starship, éventuellement B). Sinon ne pas injecter de marques.

## 4. nushell 0.116.1 : RÉUSSI

- Version exacte : `0.116.1`.
- **OSC 133 natif, activé par défaut** (`$env.config.shell_integration` = `{osc2: true, osc7: true, osc8: true, osc133: true, osc633: true, ...}`). Il émet A (`133;A;k=i;click_events=1`), P (`133;P;k=r`, invite de droite), B, C, D;code, et OSC 7. **Terminateur ESC \ (ST)**, pas BEL (sauf OSC 2 en BEL). Extrait :

```
^[]133;A;k=i;click_events=1^[\/tmp> ^[]133;B^[\print ...
^[]133;C^[\^[]2;/tmp> print^GNU=1 ENV=1 AFTER= osc133=true
^[]133;D;0^[\^[]2;/tmp^G^[]7;file://B612/tmp^[\
```

- **Option de lancement : `nu -e "source <fichier>"`** (`--execute`). Elle s'exécute **après** `env.nu` et `config.nu` de l'utilisateur, puis entre en interactif : mesuré `NU=1 ENV=1 AFTER=1` (le fichier a vu `VU_NU` posé par la config de l'utilisateur). `--config`/`--env-config` remplaceraient les fichiers de l'utilisateur : inutile. Si l'utilisateur coupe `osc133 = false`, notre fichier peut le rétablir (`$env.config.shell_integration.osc133 = true`) : témoin sans notre fichier = aucune marque 133 (OSC 7 reste) ; avec = A/B/C/D reviennent.
- `source` exige un chemin connu à l'analyse : dans la chaîne passée à `-e`, on interpole le chemin absolu. Pour Starship : `starship init nu` (PAS `--print-full-init`, qui répond « Shell name detection failed ») produit un fichier à enregistrer puis à `source` ; fonctionne (`STAR> `, marques natives intactes). Ce fichier doit être écrit hors HOME (dossier d'exécution de Pupitre).
- Écrit dans HOME : `history.txt` dans le dossier de config de nushell (comportement normal).

## 5. Niveau 2

- **elvish 0.21.0 : partiel.** `elvish -rc <fichier>` remplace le `rc.elv` de l'utilisateur : notre fichier commence par `eval (slurp < $E:XDG_CONFIG_HOME/elvish/rc.elv)` (mesuré : `VE=1 AFTER=1`). Pas d'OSC 133 natif. Marques posables via `edit:before-readline` (D puis OSC 7 puis A) et `edit:after-readline` (C) : octets `^[]133;D^G^[]7;...^G^[]133;A^G` puis `^[]133;C^G`. **Limites : D sans code de sortie facile ; B impossible** (elvish affiche les caractères de contrôle de l'invite en clair, mesuré : `^[]133;B` visible dans l'invite). → à proposer : A/C/D sans B, ou elvish « tel quel ».
- **xonsh : non testé, absent** (`--rc` à vérifier plus tard). `starship init xonsh --print-full-init` a bien été produit (voir Starship).
- **tcsh : non testé, absent.** À noter par défaut : « passe tel quel » (pas de mécanisme connu sans toucher `~/.tcshrc`).

## 6. Starship 1.22.1 : RÉUSSI

- `starship init <shell> --print-full-init` pour bash (7425 o), zsh (4214), fish (3762), elvish (1083), xonsh (1380), tcsh (782) : **aucun ne contient « 133 »**. `nu` : `--print-full-init` répond une erreur de 179 octets ; `starship init nu` (1176 o) ne contient pas 133 non plus. Starship n'émet donc aucune marque : on les ajoute sans doublon.
- `strace -f -e trace=openat starship prompt`, `HOME` temporaire contenant `.config/starship.toml` (`USERTOML> `) :
  - avec `STARSHIP_CONFIG=<alt.toml>` : seul `<alt.toml>` est ouvert, invite `ENVTOML>` ; **`~/.config/starship.toml` n'est ni ouvert ni même tenté** → RÉUSSI.
  - témoin sans `STARSHIP_CONFIG` : `~/.config/starship.toml` ouvert, invite `USERTOML>`.

## Pièges à reporter dans les tâches suivantes

1. **bash, ordre** : l'init de Starship (bash) pose son propre `PS1` à chaque `PROMPT_COMMAND` et lit `$?` au début : notre première entrée doit faire `return $c`. B doit être ajoutée à `PS1` **après** Starship (dernière entrée de `PROMPT_COMMAND`, tableau, bash ≥ 5.1) et dédoublonnée (sinon B s'accumule si `PS1` est statique).
2. **bash, PS0** : Starship utilise aussi `PS0` : ajouter la nôtre à la suite (`PS0="${PS0-}$'\e]133;C\a'"`), ne pas écraser.
3. **bash, première invite** : ne pas émettre D avant la toute première invite (drapeau `__p_dej`).
4. **bash/zsh, Starship avec bash-preexec** : le `init` de Starship détecte `bash-preexec`/`preexec_functions` : nos propres hooks ne doivent pas déclarer `preexec_functions` en bash.
5. **fish n'émet pas B** (fin d'invite) : à décider si le terminal en a besoin ; sinon A suffit pour découper.
6. **nushell** : terminateur ESC \ (ST) et A avec paramètres (`k=i;click_events=1`), P en plus : l'analyseur OSC de Pupitre doit accepter ST, BEL et les paramètres après `;`. fish : paramètres aussi (`special_key=1`, `cmdline_url=…`).
7. **Fichiers de Pupitre** : à écrire dans le dossier d'exécution/données de Pupitre (jamais HOME) ; le `starship.nu` de nushell est généré à chaque lancement ou mis en cache hors HOME.
