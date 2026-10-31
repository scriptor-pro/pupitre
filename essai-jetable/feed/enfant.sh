#!/bin/bash
# ESSAI JETABLE — tourne dans le PTY de l'essai ; chaque étape note ce qu'elle reçoit dans sortie/enfant.txt.
S="$PWD/sortie"; ESC=$'\e'
etape() { touch "$S/etape-$1"; }   # signal pour le pilote
note() { printf '%-22s %q\n' "$1" "$2" >> "$S/enfant.txt"; }
lire_jusqua() { local rep="" c; while IFS= read -r -s -n1 -t "${2:-3}" c; do rep+="$c"; [[ "$rep" =~ $1 ]] && break; done; printf '%s' "$rep"; }
echo "MARQUEUR-A11Y essai feed : éàü"
old=$(stty -g)
stty raw -echo
printf '%s[c' "$ESC";        note "réponse DA1" "$(lire_jusqua 'c$')"
printf '%s[?2004$p' "$ESC";  note "réponse DECRQM 2004" "$(lire_jusqua 'y$')"
stty "$old"
echo "tapez une ligne :"; etape clavier
IFS= read -r -t 20 ligne;    note "ligne tapée" "$ligne"
printf '%s[?2004h' "$ESC"; stty raw -echo; etape collage
note "collage reçu" "$(lire_jusqua '201~$' 20)"
printf '%s[?2004l' "$ESC"
printf '%s[?1000h%s[?1006h' "$ESC" "$ESC"; etape souris
note "clic souris reçu" "$(lire_jusqua '[0-9]M$' 20)"
note "relâchement reçu" "$(lire_jusqua '[0-9]m$' 3)"
printf '%s[?1006l%s[?1000l' "$ESC" "$ESC"; stty "$old"
echo "MARQUEUR-A11Y sonde en cours"; etape a11y
IFS= read -r -t 30 _
etape vim
vim -u NONE -N -c 'set nomore laststatus=2' "$PWD/enfant.sh"
note "fin" "ok"
