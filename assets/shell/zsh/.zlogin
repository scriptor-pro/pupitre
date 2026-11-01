# SPDX-License-Identifier: GPL-3.0-or-later
# Intégration de Pupitre pour zsh : relais vers le .zlogin de l'utilisateur (shell de connexion),
# puis ZDOTDIR est rendu à sa valeur d'origine (ou retiré) : notre .zshrc l'a laissé en place
# pour qu'on lise ce fichier.
{
    local __pz_nous="$ZDOTDIR"
    local __pz_user="${PUPITRE_ZDOTDIR_ORIGINE:-$HOME}"
    ZDOTDIR="$__pz_user"
    [[ -r "$__pz_user/.zlogin" ]] && source "$__pz_user/.zlogin"
    ZDOTDIR="$__pz_nous"
}
if [[ -n ${__pupitre_rendre_zdotdir-} ]]; then
    if [[ -n ${PUPITRE_ZDOTDIR_ORIGINE-} ]]; then
        ZDOTDIR="$PUPITRE_ZDOTDIR_ORIGINE"
    else
        unset ZDOTDIR
    fi
    unset __pupitre_rendre_zdotdir
fi
