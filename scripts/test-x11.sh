#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Lance tous les tests sur un écran X11 virtuel (Xvfb), avec un bus D-Bus et AT-SPI propres.
set -euo pipefail
cd "$(dirname "$0")/.."
# Les phrases attendues par les tests sont françaises, quelle que soit la langue de la machine.
export LANGUAGE=fr
# Pas de gvfs : il monterait un système de fichiers FUSE dans le dossier temporaire.
export GIO_USE_VFS=local
# Pas de portails : le portail de documents tenterait un montage FUSE.
export GDK_DEBUG=no-portals
command -v xvfb-run >/dev/null || { echo "xvfb-run manquant : sudo apt install xvfb" >&2; exit 1; }
unset WAYLAND_DISPLAY
export GDK_BACKEND=x11
exec xvfb-run -a -s "-screen 0 1920x1080x24" \
  dbus-run-session -- scripts/trousseau-test.sh cargo test --workspace "$@" -- --include-ignored
