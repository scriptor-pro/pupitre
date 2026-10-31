#!/bin/bash
# ESSAI JETABLE — pilote l'essai feed() sous Xvfb : clavier, collage, souris, a11y, vim redimensionné.
S="$PWD/sortie"; P="$S/pilote.txt"
attendre() { for _ in $(seq 150); do [ -e "$S/etape-$1" ] && return 0; sleep 0.2; done; echo "délai dépassé : $1" >> "$P"; return 1; }
ecran() { echo ecran > "$S/commande"; sleep 0.6; mv "$S/ecran.txt" "$S/ecran-$1.txt"; }
W=""; for _ in $(seq 50); do W=$(xdotool search --name '^Essai feed$' | head -1); [ -n "$W" ] && break; sleep 0.2; done
echo "fenêtre : $W" >> "$P"
xdotool windowfocus --sync "$W" >> "$P" 2>&1
attendre clavier && { sleep 0.3; xdotool type --delay 30 'bonjour clavier'; xdotool key Return; }
attendre collage && { printf 'texte colle' | xclip -selection clipboard; sleep 0.3; xdotool key ctrl+shift+v; }
attendre souris  && { sleep 0.3; xdotool mousemove --window "$W" 100 60 click 1; }
attendre a11y    && { sleep 0.8; python3 -I sonde.py > "$S/sonde.txt" 2>&1; xdotool key Return; }
attendre vim     && {
  sleep 2; import -window root "$S/vim-avant.png"; ecran avant
  xdotool windowsize "$W" 640 400; sleep 1.5
  xdotool type --delay 20 ":call writefile([&columns.'x'.&lines], 'sortie/vim-taille-1.txt')"; xdotool key Return; sleep 0.5
  ecran apres-reduction; import -window root "$S/vim-apres-reduction.png"
  xdotool windowsize "$W" 1100 700; sleep 1.5
  xdotool type --delay 20 ":call writefile([&columns.'x'.&lines], 'sortie/vim-taille-2.txt')"; xdotool key Return; sleep 0.5
  ecran apres-agrandissement; import -window root "$S/vim-apres-agrandissement.png"
  xdotool type ':qa!'; xdotool key Return; }
