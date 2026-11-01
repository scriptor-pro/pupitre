# SPDX-License-Identifier: GPL-3.0-or-later
# Intégration de Pupitre pour zsh : relais vers le .zprofile de l'utilisateur (shell de connexion).
{
    local __pz_nous="$ZDOTDIR"
    local __pz_user="${PUPITRE_ZDOTDIR_ORIGINE:-$HOME}"
    ZDOTDIR="$__pz_user"
    [[ -r "$__pz_user/.zprofile" ]] && source "$__pz_user/.zprofile"
    ZDOTDIR="$__pz_nous"
}
