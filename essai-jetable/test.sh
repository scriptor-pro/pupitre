#!/bin/bash
# ESSAI JETABLE — interroge le terminal hôte (VTE) et note ses réponses.
OUT="$PWD/resultats-sequences.txt"; : > "$OUT"
ESC=$'\e'; BEL=$'\a'; ST=$'\e\\'
old=$(stty -g); stty raw -echo
# Envoie une requête suivie de DA1 (que tout terminal comprend), lit jusqu'à la réponse DA1.
q() { local nom="$1" req="$2" rep="" c
  printf '%s%s[c' "$req" "$ESC" > /dev/tty
  while IFS= read -r -s -n1 -t 2 c < /dev/tty; do rep+="$c"; [[ "$rep" =~ \[\?[0-9\;]*c$ ]] && break; done
  rep="${rep%$ESC\[\?*c}"   # retire la réponse DA1 finale
  printf '%-34s %s\n' "$nom" "$(printf '%q' "$rep")" >> "$OUT"; }
q "DA2 (version)"                  "${ESC}[>c"
q "XTVERSION"                      "${ESC}[>0q"
q "sync output DECRQM 2026"        "${ESC}[?2026\$p"
q "bracketed paste DECRQM 2004"    "${ESC}[?2004\$p"
q "focus events DECRQM 1004"       "${ESC}[?1004\$p"
q "graphemes DECRQM 2027"          "${ESC}[?2027\$p"
q "souris SGR DECRQM 1006"         "${ESC}[?1006\$p"
q "protocole clavier kitty CSI ?u" "${ESC}[?u"
q "modifyOtherKeys XTQMODKEYS"     "${ESC}[?4m"
q "OSC 52 lecture presse-papiers"  "${ESC}]52;c;?${BEL}"
q "OSC 10 couleur texte"           "${ESC}]10;?${BEL}"
q "OSC 11 couleur fond"            "${ESC}]11;?${BEL}"
q "OSC 4 couleur 8"                "${ESC}]4;8;?${BEL}"
q "truecolor DECRQSS après SGR"    "${ESC}[38;2;255;136;0m${ESC}P\$qm${ST}${ESC}[0m"
q "sixel (DA1 attribut 4 ?)"       ""
q "kitty graphics"                 "${ESC}_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA${ST}"
stty "$old"
# Effets sans réponse : on les déclenche, l'application observe.
printf '%s]2;titre-osc-ok%s' "$ESC" "$BEL"
printf '%s]7;file://%s%s%s' "$ESC" "$(hostname)" "$PWD" "$BEL"
printf '%s' "$BEL"
printf '%s]52;c;%s%s' "$ESC" "$(printf 'pupitre-osc52-ok' | base64)" "$BEL"
printf '%s]9;notification OSC 9%s' "$ESC" "$BEL"
printf '%s]777;notify;titre;notification OSC 777%s' "$ESC" "$BEL"
printf '%s]133;A%s' "$ESC" "$BEL"; printf '%s]133;B%s' "$ESC" "$BEL"; printf '%s]133;C%s' "$ESC" "$BEL"; printf '%s]133;D;0%s' "$ESC" "$BEL"
printf '%s]666;vte.shell.preexec!%s' "$ESC" "$BEL"; printf '%s]666;vte.shell.precmd!%s' "$ESC" "$BEL"
clear
printf '%s]8;;https://example.org/pupitre%s>> lien OSC 8 <<%s]8;;%s\n' "$ESC" "$ST" "$ESC" "$ST"
printf 'Dégradé truecolor : '; for i in $(seq 0 4 255); do printf '%s[48;2;%d;%d;200m ' "$ESC" "$i" "$((255-i))"; done; printf '%s[0m\n' "$ESC"
printf 'Palette 16 : '; for i in $(seq 0 15); do printf '%s[48;5;%dm %2d %s[0m' "$ESC" "$i" "$i" "$ESC"; done; echo
echo 'Ligatures B612 : -> => != === >= <= :: ||'
echo 'Unicode : é ü ñ — 中文 日本語 😀 👍🏽 🇫🇷'
sleep 0.5
xclip -selection clipboard -o 2>/dev/null | head -c 60 | sed 's/^/presse-papiers après OSC 52 écriture : /' >> "$OUT"; echo >> "$OUT"
# Accessibilité : que voit AT-SPI de cette application ?
python3 -I "$PWD/sonde_a11y.py" >> "$OUT" 2>&1
import -window "$(xdotool search --name "^Essai VTE$" | head -1)" "$PWD/capture.png" 2>/dev/null
sleep 0.3
