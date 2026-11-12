#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Essai jetable (Task 0, étape 4) : émet les séquences de notification que Claude Code
# (canaux iterm2 / ghostty / kitty) et les outils courants envoient à un terminal.
# À lancer DANS un terminal (par ex. Pupitre) pour voir ce que le terminal en fait.
# Usage : osc.sh [9|777|99|progres|tout]
cas=${1:-tout}
if [ "$cas" = 9 ] || [ "$cas" = tout ]; then
  printf '\033]9;Essai Pupitre : OSC 9 (iTerm2)\007'
  printf '\033]9;Titre: corps (forme émise par Claude Code, canal iterm2)\007'
fi
if [ "$cas" = 777 ] || [ "$cas" = tout ]; then
  printf '\033]777;notify;Titre;Corps (OSC 777 Ghostty/rxvt)\007'
  printf '\033]777;notify;Titre;Corps avec ST\033\\'
fi
if [ "$cas" = 99 ] || [ "$cas" = tout ]; then
  # Kitty, tel qu'émis par Claude Code : titre, corps, puis demande de focus.
  printf '\033]99;i=1234:d=0:p=title;Titre kitty\007'
  printf '\033]99;i=1234:p=body;Corps kitty\007'
  printf '\033]99;i=1234:d=1:a=focus;\007'
fi
if [ "$cas" = progres ] || [ "$cas" = tout ]; then
  # ConEmu / Windows Terminal : 9;4;état;pourcentage (0 effacer, 1 normal, 2 erreur,
  # 3 indéterminé, 4 pause). Ce n'est PAS un texte à annoncer : à ignorer.
  printf '\033]9;4;1;40\007'
  printf '\033]9;4;3\007'
  printf '\033]9;4;0\007'
fi
