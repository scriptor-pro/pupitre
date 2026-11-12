# SPDX-License-Identifier: GPL-3.0-or-later
# Intégration de Pupitre pour fish (lancé par `fish -C "source <ce fichier>"`, donc exécuté
# après config.fish : la configuration habituelle de l'utilisateur est déjà chargée).
# fish 4 émet lui-même OSC 133 A, C, D;code et OSC 7 : on n'ajoute que la marque B (fin de l'invite).
# Rien n'est écrit dans HOME.

# Starship, seulement si demandé et si le binaire est toujours là.
if test "$PUPITRE_INVITE" = starship; and test -n "$PUPITRE_STARSHIP"; and test -x "$PUPITRE_STARSHIP"
    $PUPITRE_STARSHIP init fish --print-full-init | source
end

# Rétablit `$status` (un `set` le remet à 0).
function __pupitre_rendre
    return $argv[1]
end

# B : on enveloppe l'invite courante (celle de l'utilisateur ou celle de Starship).
if functions -q fish_prompt; and not functions -q __pupitre_fish_prompt_origine
    functions -c fish_prompt __pupitre_fish_prompt_origine
    function fish_prompt --description "Invite avec la marque de fin d'invite de Pupitre"
        # $status et $pipestatus sont ceux de la dernière commande : on les lit ensemble, en
        # premier ; on publie le code pour l'invite Starship (symbole rouge après un échec),
        # puis on les rétablit pour l'invite d'origine (qui les lit à son tour).
        set -l s $status $pipestatus
        set -gx PUPITRE_CODE $s[1]
        # $pipestatus ne traverse ni une fonction ni `eval` : le pipeline qui le rétablit (un
        # étage par code) est écrit ici. Au-delà de 8 étages, ou si $status n'est pas le dernier
        # code (commande niée par `not`), seul $status est rétabli.
        set -l p $s[2..-1]
        if test "$s[1]" != "$p[-1]"
            set p $s[1]
        end
        switch (count $p)
            case 2
                __pupitre_rendre $p[1] | __pupitre_rendre $p[2]
            case 3
                __pupitre_rendre $p[1] | __pupitre_rendre $p[2] | __pupitre_rendre $p[3]
            case 4
                __pupitre_rendre $p[1] | __pupitre_rendre $p[2] | __pupitre_rendre $p[3] | __pupitre_rendre $p[4]
            case 5
                __pupitre_rendre $p[1] | __pupitre_rendre $p[2] | __pupitre_rendre $p[3] | __pupitre_rendre $p[4] | __pupitre_rendre $p[5]
            case 6
                __pupitre_rendre $p[1] | __pupitre_rendre $p[2] | __pupitre_rendre $p[3] | __pupitre_rendre $p[4] | __pupitre_rendre $p[5] | __pupitre_rendre $p[6]
            case 7
                __pupitre_rendre $p[1] | __pupitre_rendre $p[2] | __pupitre_rendre $p[3] | __pupitre_rendre $p[4] | __pupitre_rendre $p[5] | __pupitre_rendre $p[6] | __pupitre_rendre $p[7]
            case 8
                __pupitre_rendre $p[1] | __pupitre_rendre $p[2] | __pupitre_rendre $p[3] | __pupitre_rendre $p[4] | __pupitre_rendre $p[5] | __pupitre_rendre $p[6] | __pupitre_rendre $p[7] | __pupitre_rendre $p[8]
            case '*'
                __pupitre_rendre $s[1]
        end
        __pupitre_fish_prompt_origine
        printf '\e]133;B\a'
    end
end
