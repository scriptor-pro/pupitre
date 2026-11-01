# SPDX-License-Identifier: GPL-3.0-or-later
# Intégration de Pupitre pour zsh : relais vers le .zshenv de l'utilisateur.
# Pupitre lance zsh avec ZDOTDIR=<ce dossier> et PUPITRE_ZDOTDIR_ORIGINE=<ZDOTDIR d'origine>
# (vide : l'utilisateur n'en avait pas, donc $HOME). On rend ZDOTDIR à l'origine le temps de
# lire le fichier de l'utilisateur, puis on remet le nôtre pour que zsh lise nos fichiers suivants.
if [[ -z ${PUPITRE_DANS_ZSHENV-} ]]; then
    typeset -g PUPITRE_DANS_ZSHENV=1
    typeset -g __pupitre_zd_nous="$ZDOTDIR"
    typeset -g __pupitre_zd_user="${PUPITRE_ZDOTDIR_ORIGINE:-$HOME}"
    ZDOTDIR="$__pupitre_zd_user"
    [[ -r "$__pupitre_zd_user/.zshenv" ]] && source "$__pupitre_zd_user/.zshenv"
    # Le .zshenv de l'utilisateur a pu déplacer ZDOTDIR : on le suit pour les fichiers suivants.
    if [[ "$ZDOTDIR" != "$__pupitre_zd_user" ]]; then
        __pupitre_zd_user="$ZDOTDIR"
        PUPITRE_ZDOTDIR_ORIGINE="$ZDOTDIR"
    fi
    ZDOTDIR="$__pupitre_zd_nous"
    unset PUPITRE_DANS_ZSHENV
fi
