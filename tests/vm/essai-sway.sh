#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Essai sous Sway, lancé dans la VM par « scripts/vm.sh sway essayer ». Démarre Sway sans
# écran (clavier fr dvorak), définit les gestes pour tests/vm/commun.sh, puis déroule.
#
# wtype envoie son propre plan de clavier à chaque appel, et GTK (comme foot) perd alors
# la première touche : chaque appel commence par une touche neutre (Maj gauche).
set -uo pipefail
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/bus
export PUPITRE_ASSETS=$HOME/pupitre/assets
export LANG=fr_BE.UTF-8
. ~/commun.sh
rm -rf "$R" && mkdir -p "$R"

taper() { wtype -k Shift_L "$1" -k Return; sleep 1; }
ecrire() { wtype -k Shift_L "$1"; }
touches() { # ctrl+shift+t → -M ctrl -M shift -k t -m shift -m ctrl
  local args=(-k Shift_L) combo mods touche m
  for combo in "$@"; do
    IFS=+ read -ra mods <<< "$combo"
    touche="${mods[-1]}"; unset 'mods[-1]'
    for m in "${mods[@]}"; do args+=(-M "$m"); done
    case "$touche" in
      escape) touche=Escape ;; prior) touche=Prior ;; next) touche=Next ;; return) touche=Return ;;
    esac
    args+=(-k "$touche")
    for ((i=${#mods[@]}-1; i>=0; i--)); do args+=(-m "${mods[i]}"); done
  done
  wtype "${args[@]}"; sleep 1.5
}
fenetres() { swaymsg -t get_tree | jq -r '.. | objects | select(.app_id? != null) | "\(.app_id) — \(.name) — focus=\(.focused)"'; }
capture() { grim "$1"; }
presse_papiers() { printf '%s' "$1" | wl-copy; sleep 0.5; }

mkdir -p ~/.config/sway
cat > ~/.config/sway/config <<'CONF'
input * xkb_layout fr
input * xkb_variant dvorak
output HEADLESS-1 resolution 1280x800
default_border normal
CONF
systemctl --user stop pupitre sway 2>/dev/null
systemctl --user reset-failed sway 2>/dev/null
systemd-run --user -q --unit=sway --setenv=WLR_BACKENDS=headless --setenv=WLR_RENDERER=pixman \
  --setenv=WLR_LIBINPUT_NO_DEVICES=1 --setenv=LANG="$LANG" sway
sleep 3
export WAYLAND_DISPLAY=wayland-1
SWAYSOCK=$(ls "$XDG_RUNTIME_DIR"/sway-ipc.*.sock | head -1)
export SWAYSOCK
# Un clavier virtuel reste branché pendant tout l'essai : sans clavier, la fenêtre n'est jamais
# « active » pour GTK (un clavier wtype disparaît à la fin de chaque appel) et la fin de commande
# n'est pas annoncée (l'annonce n'est faite que par une fenêtre active).
setsid wtype -k Shift_L -s 900000 -k Shift_L >/dev/null 2>&1 &
sleep 1
lancer_pupitre
echo "Essais de Pupitre sous $(sway --version)"
derouler
code=$?
# wl-copy reste en fond pour servir le presse-papiers : il garderait la connexion SSH ouverte.
pkill -u "$(id -u)" wl-copy 2>/dev/null
pkill -u "$(id -u)" -f 'wtype -k Shift_L -s 900000' 2>/dev/null
exit $code
