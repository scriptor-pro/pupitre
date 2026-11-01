#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Essai sous GNOME Shell (Wayland), lancé dans la VM par « scripts/vm.sh gnome essayer ».
# GNOME Shell tourne sans écran (moniteur virtuel), en mode « unsafe » (extension
# installée ci-dessous) pour que l'essai lise l'état des fenêtres et fasse des captures
# (VM d'essai seulement). Clavier
# fr dvorak. Gestes : tests/vm/gnome.py ; déroulé : tests/vm/commun.sh.
set -uo pipefail
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/bus
export PUPITRE_ASSETS=$HOME/pupitre/assets
export LANG=fr_BE.UTF-8
. ~/commun.sh
rm -rf "$R" && mkdir -p "$R"

g() { python3 ~/gnome.py "$@"; }
taper() { g taper "$1"; sleep 1; }
ecrire() { g ecrire "$1"; }
touches() { g touches "$@"; sleep 1.5; }
fenetres() { g fenetres; }
capture() { g capture "$1"; }
presse_papiers() { g eval "essai.presse_papiers('$1')" >/dev/null; sleep 0.5; }

gsettings set org.gnome.desktop.input-sources sources "[('xkb', 'fr+dvorak')]"
# Mode « unsafe » par une petite extension (GNOME 48 n'a plus d'option de lancement) :
# il ouvre org.gnome.Shell.Eval et la capture d'écran à l'essai. VM d'essai seulement.
EXT=~/.local/share/gnome-shell/extensions/unsafe@pupitre
mkdir -p "$EXT"
cat > "$EXT/metadata.json" <<'JSON'
{"uuid": "unsafe@pupitre", "name": "Mode unsafe (essais Pupitre)", "description": "VM d'essai",
 "shell-version": ["48"]}
JSON
cat > "$EXT/extension.js" <<'JS'
import St from 'gi://St';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
export default class Unsafe extends Extension {
    enable() {
        global.context.unsafe_mode = true;
        // Pour org.gnome.Shell.Eval : presse-papiers et vue d'ensemble.
        globalThis.essai = {
            presse_papiers: t => St.Clipboard.get_default().set_text(St.ClipboardType.CLIPBOARD, t),
            fermer_vue: () => Main.overview.hide(),
        };
    }
    disable() { global.context.unsafe_mode = false; delete globalThis.essai; }
}
JS
gsettings set org.gnome.shell disable-user-extensions false
gsettings set org.gnome.shell enabled-extensions "['unsafe@pupitre']"
systemctl --user stop pupitre gnome-essai 2>/dev/null
systemctl --user reset-failed gnome-essai 2>/dev/null
systemd-run --user -q --unit=gnome-essai --setenv=XDG_SESSION_TYPE=wayland --setenv=LANG="$LANG" \
  gnome-shell --headless --wayland --no-x11 --virtual-monitor 1280x800
for _ in $(seq 60); do
  ls "$XDG_RUNTIME_DIR"/wayland-0 >/dev/null 2>&1 && g fenetres >/dev/null 2>&1 && break
  sleep 2
done
sleep 5
# GNOME démarre sur la vue d'ensemble (Activités) ; un utilisateur lance Pupitre depuis
# celle-ci, qui se referme. On la referme avant le lancement.
g eval "essai.fermer_vue()" >/dev/null
sleep 2
export WAYLAND_DISPLAY=wayland-0
lancer_pupitre
echo "Essais de Pupitre sous $(gnome-shell --version)"
derouler
code=$?
exit $code
