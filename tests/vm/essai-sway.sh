#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Déroulé dans la VM Sway par scripts/vm-sway.sh essayer. Chaque essai écrit son résultat
# dans un fichier ; le script finit par « BILAN : n réussi(s), m échec(s) » et un code de
# sortie non nul en cas d'échec.
#
# wtype envoie son propre plan de clavier à chaque appel, et GTK (comme foot) perd alors
# la première touche : chaque appel commence par une touche neutre (Maj gauche).
set -uo pipefail

export XDG_RUNTIME_DIR=/run/user/$(id -u)
export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/bus
export PUPITRE_ASSETS=$HOME/pupitre/assets
export LANG=fr_BE.UTF-8
R=/tmp/essai
rm -rf "$R" && mkdir -p "$R"
REUSSIS=0
ECHECS=0
verdict() { # nom, attendu (motif grep -E), fichier
  if grep -qE "$2" "$3" 2>/dev/null; then
    echo "  ok      $1"; REUSSIS=$((REUSSIS + 1))
  else
    echo "  ÉCHEC   $1 (attendu /$2/, obtenu « $(head -c 200 "$3" 2>/dev/null) »)"; ECHECS=$((ECHECS + 1))
  fi
}
taper() { wtype -k Shift_L "$1" -k Return; sleep 1; }
touches() { wtype -k Shift_L "$@"; sleep 1.5; }
fenetres() { swaymsg -t get_tree | jq -r '.. | objects | select(.app_id? != null) | "\(.app_id) — \(.name) — focus=\(.focused)"'; }

# Sway sans écran, clavier fr dvorak (celui du porteur).
mkdir -p ~/.config/sway
cat > ~/.config/sway/config <<'EOF'
input * xkb_layout fr
input * xkb_variant dvorak
output HEADLESS-1 resolution 1280x800
default_border normal
EOF
systemctl --user stop pupitre sway 2>/dev/null
systemctl --user reset-failed pupitre sway 2>/dev/null
pkill -u "$(id -u)" -f pupitre-fond 2>/dev/null
rm -f ~/.ssh/id_test ~/.ssh/id_test.pub
systemd-run --user -q --unit=sway --setenv=WLR_BACKENDS=headless --setenv=WLR_RENDERER=pixman \
  --setenv=WLR_LIBINPUT_NO_DEVICES=1 --setenv=LANG="$LANG" sway
sleep 3
export WAYLAND_DISPLAY=wayland-1
SWAYSOCK=$(ls "$XDG_RUNTIME_DIR"/sway-ipc.*.sock | head -1)
export SWAYSOCK
systemd-run --user -q --unit=pupitre --setenv=WAYLAND_DISPLAY=$WAYLAND_DISPLAY \
  --setenv=PUPITRE_ASSETS="$PUPITRE_ASSETS" --setenv=LANG="$LANG" ~/pupitre/bin/pupitre
sleep 6

echo "Essais de Pupitre sous $(sway --version)"
fenetres > "$R/ouverture"
verdict "fenêtre ouverte, focalisée, identifiant fr.pupitre.Pupitre" '^fr\.pupitre\.Pupitre — .* — focus=true' "$R/ouverture"
grim "$R/1-ouverture.png"

taper "echo ok-42 > $R/saisie; tty > $R/tty1"
verdict "saisie et Entrée" '^ok-42$' "$R/saisie"

touches -M ctrl -M shift -k t -m shift -m ctrl
taper "tty > $R/tty2"
if [ -s "$R/tty2" ] && ! cmp -s "$R/tty1" "$R/tty2"; then echo autre > "$R/onglet"; fi
verdict "Ctrl+Maj+T : nouvel onglet (autre terminal)" '^autre$' "$R/onglet"

printf 'texte-colle-42' | wl-copy
sleep 0.5
wtype -k Shift_L "echo "; touches -M ctrl -M shift -k v -m shift -m ctrl; taper " > $R/colle"
verdict "Ctrl+Maj+V : collage du presse-papiers" '^texte-colle-42$' "$R/colle"

touches -M ctrl -k Prior -m ctrl
taper "tty > $R/tty-retour"
verdict "Ctrl+PgPréc : retour au premier onglet" "^$(cat "$R/tty1" 2>/dev/null)\$" "$R/tty-retour"

# Fenêtre de phrase de passe : elle doit recevoir le clavier sous Wayland.
taper "ssh-keygen -q -t ed25519 -N phrase-test -f ~/.ssh/id_test; ssh-add ~/.ssh/id_test; ssh-add -l > $R/agent"
sleep 4
fenetres > "$R/demande"
grim "$R/2-demande-ssh.png"
verdict "fenêtre de phrase de passe focalisée" '^fr\.pupitre\.Pupitre — Phrase de passe SSH — focus=true' "$R/demande"
touches "phrase-test" -k Return
sleep 3
verdict "phrase acceptée, clé chargée dans l'agent" 'ED25519' "$R/agent"
fenetres > "$R/apres-demande"
verdict "clavier rendu au terminal" '^fr\.pupitre\.Pupitre — .* — Pupitre — focus=true' "$R/apres-demande"

touches -M ctrl -M shift -k o -m shift -m ctrl
fenetres > "$R/connexions"
grim "$R/3-connexions.png"
verdict "Ctrl+Maj+O : Connexions SSH focalisée" '^fr\.pupitre\.Pupitre — Connexions SSH — focus=true' "$R/connexions"
touches -k Escape
fenetres > "$R/apres-connexions"
if grep -q "Connexions SSH" "$R/apres-connexions"; then echo encore > "$R/ferme"; else echo fermee > "$R/ferme"; fi
verdict "Échap ferme Connexions SSH" '^fermee$' "$R/ferme"

# Accessibilité : ce qu'Orca lirait.
python3 - > "$R/atspi" 2>&1 <<'EOF'
import pyatspi
def voir(n, p=0, prof=0):
    if n is None or prof > 8:
        return
    try:
        r, nom = n.getRoleName(), n.name
    except Exception:
        return
    if r not in ("panel", "filler") or nom:
        print("  " * p + f"{r} « {nom} »")
        p += 1
    for i in range(min(n.childCount, 20)):
        voir(n.getChildAtIndex(i), p, prof + 1)
for app in pyatspi.Registry.getDesktop(0):
    if app and app.name and "upitre" in app.name:
        voir(app)
EOF
verdict "accessibilité : liste d'onglets exposée" 'page tab list' "$R/atspi"
verdict "accessibilité : terminal exposé" '^ *terminal « ' "$R/atspi"
verdict "accessibilité : bouton Menu nommé" 'button « Menu »' "$R/atspi"

systemctl --user stop pupitre
# wl-copy reste en fond pour servir le presse-papiers : il garderait la connexion SSH ouverte.
pkill -u "$(id -u)" wl-copy 2>/dev/null
echo "BILAN : $REUSSIS réussi(s), $ECHECS échec(s)"
[ "$ECHECS" -eq 0 ]
