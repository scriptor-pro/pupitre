# SPDX-License-Identifier: GPL-3.0-or-later
# Intégration de Pupitre pour fish (lancé par `fish -C "source <ce fichier>"`, donc exécuté
# après config.fish : la configuration habituelle de l'utilisateur est déjà chargée).
# fish 4 émet lui-même OSC 133 A, C, D;code et OSC 7 : on n'ajoute que la marque B (fin de l'invite).
# Rien n'est écrit dans HOME.

# Starship, seulement si demandé et si le binaire est toujours là.
if test "$PUPITRE_INVITE" = starship; and test -n "$PUPITRE_STARSHIP"; and test -x "$PUPITRE_STARSHIP"
    $PUPITRE_STARSHIP init fish --print-full-init | source
end

# B : on enveloppe l'invite courante (celle de l'utilisateur ou celle de Starship).
if functions -q fish_prompt; and not functions -q __pupitre_fish_prompt_origine
    functions -c fish_prompt __pupitre_fish_prompt_origine
    function fish_prompt --description "Invite avec la marque de fin d'invite de Pupitre"
        # Premier appel : $status est encore celui de la dernière commande (invite d'origine).
        __pupitre_fish_prompt_origine
        printf '\e]133;B\a'
    end
end
