#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Essai sous KWin (KDE Plasma 6, Wayland), lancé dans la VM par « scripts/vm.sh kde essayer ».
# KWin tourne dans la session ouverte automatiquement sur tty1 (carte graphique virtio) :
# il y lit par libinput le clavier simulé (uinput) de tests/vm/kde.py. Clavier QWERTY
# américain (le simulateur traduit les caractères pour ce plan). Le panneau Plasma n'est
# pas lancé : seul KWin, le gestionnaire de fenêtres, décide du focus et des dialogues.
# Déroulé : tests/vm/commun.sh.
set -uo pipefail
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/bus
export PUPITRE_ASSETS=$HOME/pupitre/assets
export LANG=fr_BE.UTF-8
. ~/commun.sh
rm -rf "$R" && mkdir -p "$R"
JOURNAL=/tmp/essai-kwin.log

k() { sudo -E python3 ~/kde.py "$@"; }
taper() { k taper "$1"; sleep 1; }
ecrire() { k ecrire "$1"; }
touches() { k touches "$@"; sleep 1.5; }
fenetres() { python3 ~/kde.py fenetres "$JOURNAL"; }
capture() { spectacle -b -n -f -o "$1" >/dev/null 2>&1; }
presse_papiers() { printf '%s' "$1" | wl-copy; sleep 0.5; }

sudo modprobe uinput
mkdir -p ~/.config
printf '[Layout]\nLayoutList=us\nUse=true\n' > ~/.config/kxkbrc
# La session de tty1 attend ce signal pour lancer KWin (voir ~/.bash_profile).
cat > ~/.bash_profile <<'PROFIL'
if [ "$(tty)" = /dev/tty1 ]; then
  while [ ! -e /tmp/essai-kwin-lancer ]; do sleep 1; done
  rm -f /tmp/essai-kwin-lancer
  export XDG_SESSION_TYPE=wayland QT_LOGGING_RULES="js=true;qml=true" LANG=fr_BE.UTF-8
  # VM d'essai : captures d'écran permises à spectacle lancé par SSH.
  export KWIN_SCREENSHOT_NO_PERMISSION_CHECKS=1
  exec kwin_wayland --drm --no-lockscreen > /tmp/essai-kwin.log 2>&1
fi
PROFIL
systemctl --user stop pupitre 2>/dev/null
sudo systemctl restart getty@tty1
sleep 3
touch /tmp/essai-kwin-lancer
for _ in $(seq 60); do
  [ -S "$XDG_RUNTIME_DIR/wayland-0" ] && gdbus introspect --session --dest org.kde.KWin \
    --object-path /Scripting >/dev/null 2>&1 && break
  sleep 2
done
sleep 3
export WAYLAND_DISPLAY=wayland-0
lancer_pupitre
echo "Essais de Pupitre sous $(kwin_wayland --version 2>/dev/null | head -1)"
derouler
code=$?
pkill -u "$(id -u)" wl-copy 2>/dev/null
exit $code
