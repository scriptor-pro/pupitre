# SPDX-License-Identifier: GPL-3.0-or-later
# Intégration de Pupitre pour bash (lancé par `bash --rcfile <ce fichier> -i`).
# Ordre : configuration habituelle de l'utilisateur, puis Starship (si demandé), puis les
# marques de commandes (OSC 133) et le dossier courant (OSC 7). Rien n'est écrit dans HOME.

# 1. Configuration habituelle (/etc/bash.bashrc est déjà lu par bash, même avec --rcfile).
if [ -r "$HOME/.bashrc" ]; then
    . "$HOME/.bashrc"
fi

# 2. Starship, seulement si demandé, si le binaire est toujours là, et si la configuration de
#    l'utilisateur ne l'a pas déjà initialisé (sinon ses crochets seraient doublés).
if [ "${PUPITRE_INVITE-}" = "starship" ] && [ -n "${PUPITRE_STARSHIP-}" ] \
    && [ -x "$PUPITRE_STARSHIP" ] && ! declare -F starship_precmd >/dev/null 2>&1; then
    eval "$("$PUPITRE_STARSHIP" init bash --print-full-init)"
fi

# 3. Marques, posées après tout le reste.
__pupitre_dej=
__pupitre_fin_invite=$'\\[\e]133;B\a\\]'

# Première entrée de PROMPT_COMMAND : D (code de la commande qui vient de finir) puis OSC 7.
# Doit préserver `$?` pour les entrées suivantes (Starship, crochets de l'utilisateur).
__pupitre_avant() {
    local c=$?
    # Code de la dernière commande, pour l'invite Starship (symbole rouge après un échec). Les
    # entrées de PROMPT_COMMAND passent avant l'évaluation de PS1, donc avant Starship.
    export PUPITRE_CODE="$c"
    if [ -n "$__pupitre_dej" ]; then
        printf '\e]133;D;%s\a' "$c"
    fi
    __pupitre_dej=1
    printf '\e]7;file://%s%s\a' "${HOSTNAME-}" "$PWD"
    return $c
}

# Dernière entrée : B à la fin de PS1 (sans doublon si PS1 n'est pas recalculé), puis A.
__pupitre_apres() {
    local c=$?
    PS1="${PS1//"$__pupitre_fin_invite"/}$__pupitre_fin_invite"
    printf '\e]133;A\a'
    return $c
}

# A doit précéder l'invite : on l'émet dans la dernière entrée, juste avant l'affichage.
if [ "${BASH_VERSINFO[0]}" -gt 5 ] || { [ "${BASH_VERSINFO[0]}" -eq 5 ] && [ "${BASH_VERSINFO[1]}" -ge 1 ]; }; then
    PROMPT_COMMAND=(__pupitre_avant "${PROMPT_COMMAND[@]}" __pupitre_apres)
else
    # bash < 5.1 : pas de tableau pour PROMPT_COMMAND.
    PROMPT_COMMAND="__pupitre_avant${PROMPT_COMMAND:+;$PROMPT_COMMAND};__pupitre_apres"
fi

# C juste avant l'exécution : PS0 est développé avant chaque commande (Starship l'utilise
# aussi : on ajoute à la suite sans l'écraser). Pas de preexec_functions ici (bash-preexec).
PS0="${PS0-}"$'\e]133;C\a'
