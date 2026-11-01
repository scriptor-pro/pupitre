# SPDX-License-Identifier: GPL-3.0-or-later
# Intégration de Pupitre pour zsh. Ordre : .zshrc de l'utilisateur, puis Starship (si demandé),
# puis les marques OSC 133 et OSC 7. Rien n'est écrit dans HOME.

# 1. Configuration habituelle de l'utilisateur.
{
    local __pz_nous="$ZDOTDIR"
    local __pz_user="${PUPITRE_ZDOTDIR_ORIGINE:-$HOME}"
    ZDOTDIR="$__pz_user"
    [[ -r "$__pz_user/.zshrc" ]] && source "$__pz_user/.zshrc"
    # Si l'utilisateur a déplacé ZDOTDIR, les fichiers suivants (.zlogin) le suivent.
    [[ "$ZDOTDIR" != "$__pz_user" ]] && PUPITRE_ZDOTDIR_ORIGINE="$ZDOTDIR"
    ZDOTDIR="$__pz_nous"
}

# 2. Starship, seulement si demandé, présent, et pas déjà initialisé par l'utilisateur.
if [[ "${PUPITRE_INVITE-}" == "starship" && -n "${PUPITRE_STARSHIP-}" && -x "$PUPITRE_STARSHIP" ]] \
    && (( ! ${+functions[starship_precmd]} )); then
    eval "$("$PUPITRE_STARSHIP" init zsh --print-full-init)"
fi

# 3. Marques, posées après tout le reste.
typeset -g __pupitre_dej=
# `%{ ... %}` : séquence de largeur nulle. ESC et BEL réels (pas de `\e` dans PROMPT).
typeset -g __pupitre_fin_invite=$'%{\e]133;B\a%}'

# D (code de la commande finie, pas avant la première invite), OSC 7, puis A.
# `$?` est conservé par zsh pour chaque crochet precmd.
__pupitre_precmd() {
    local c=$?
    if [[ -n $__pupitre_dej ]]; then
        printf '\e]133;D;%s\a' "$c"
    fi
    __pupitre_dej=1
    printf '\e]7;file://%s%s\a' "${HOST-}" "$PWD"
    printf '\e]133;A\a'
    PROMPT="${PROMPT//"$__pupitre_fin_invite"/}$__pupitre_fin_invite"
    return $c
}

# C juste avant l'exécution.
__pupitre_preexec() {
    printf '\e]133;C\a'
}

# En dernier dans les listes : après les crochets de l'utilisateur et de Starship.
precmd_functions=(${precmd_functions:#__pupitre_precmd} __pupitre_precmd)
preexec_functions=(${preexec_functions:#__pupitre_preexec} __pupitre_preexec)

# ZDOTDIR rendu à sa valeur d'origine (ou retiré) pour les processus lancés ensuite. Pour un
# shell de connexion, .zlogin (lu après) a encore besoin de notre dossier : il s'en charge.
if [[ -o login ]]; then
    __pupitre_rendre_zdotdir=1
else
    if [[ -n ${PUPITRE_ZDOTDIR_ORIGINE-} ]]; then
        ZDOTDIR="$PUPITRE_ZDOTDIR_ORIGINE"
    else
        unset ZDOTDIR
    fi
fi
