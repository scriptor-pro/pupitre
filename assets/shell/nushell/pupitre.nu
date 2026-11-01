# SPDX-License-Identifier: GPL-3.0-or-later
# Intégration de Pupitre pour nushell (lancé par `nu -e "source <ce fichier>"`, donc exécuté
# après env.nu et config.nu de l'utilisateur : sa configuration est déjà chargée).
# nushell émet lui-même OSC 133 (A, B, C, D;code) et OSC 7 : on ne les double pas, on s'assure
# seulement qu'ils sont actifs. Rien n'est écrit dans HOME.

# Les marques natives restent actives même si la configuration de l'utilisateur les avait coupées.
$env.config = ($env.config? | default {} | upsert shell_integration.osc133 true | upsert shell_integration.osc7 true)

# Starship, seulement si demandé et si le binaire est toujours là et exécutable (`which` ne
# trouve un chemin que s'il est exécutable). `starship init nu` produit un
# script à `source` (chemin connu à l'analyse) : on reprend ici son contenu, avec le binaire de
# Pupitre, sans écrire de fichier.
if (($env.PUPITRE_INVITE? | default "") == "starship") and (($env.PUPITRE_STARSHIP? | default "") != "") and ((which $env.PUPITRE_STARSHIP | length) > 0) {
    $env.STARSHIP_SHELL = "nu"
    $env.STARSHIP_SESSION_KEY = (random chars -l 16)
    $env.PROMPT_MULTILINE_INDICATOR = (^$env.PUPITRE_STARSHIP prompt --continuation)
    $env.PROMPT_INDICATOR = ""
    $env.PROMPT_COMMAND = {||
        with-env { PUPITRE_CODE: ($env.LAST_EXIT_CODE | into string) } {
            ^$env.PUPITRE_STARSHIP prompt --cmd-duration $env.CMD_DURATION_MS $"--status=($env.LAST_EXIT_CODE)" --terminal-width (term size).columns
        }
    }
    $env.config = ($env.config | upsert render_right_prompt_on_last_line true)
    $env.PROMPT_COMMAND_RIGHT = {||
        with-env { PUPITRE_CODE: ($env.LAST_EXIT_CODE | into string) } {
            ^$env.PUPITRE_STARSHIP prompt --right --cmd-duration $env.CMD_DURATION_MS $"--status=($env.LAST_EXIT_CODE)" --terminal-width (term size).columns
        }
    }
}
