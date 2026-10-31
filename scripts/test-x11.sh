#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Lance tous les tests sur un écran X11 virtuel (Xvfb), avec un bus D-Bus et AT-SPI propres.
set -euo pipefail
cd "$(dirname "$0")/.."
command -v xvfb-run >/dev/null || { echo "xvfb-run manquant : sudo apt install xvfb" >&2; exit 1; }
unset WAYLAND_DISPLAY
export GDK_BACKEND=x11
exec xvfb-run -a -s "-screen 0 1920x1080x24" \
  dbus-run-session -- cargo test --workspace "$@" -- --include-ignored
