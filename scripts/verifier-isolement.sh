#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Vérifie sous strace qu'une session type de Pupitre n'ouvre aucune connexion réseau et
# n'écrit rien hors de ses dossiers. Rapport et trace dans target/isolement/.
set -euo pipefail
cd "$(dirname "$0")/.."
for outil in xvfb-run xdotool xfwm4 strace ssh-keygen gnome-keyring-daemon; do
  command -v "$outil" >/dev/null || { echo "$outil manquant" >&2; exit 2; }
done
cargo build -q -p pupitre -p pupitre-fond --bins
rm -rf target/isolement
exec xvfb-run -a -s "-screen 0 1920x1080x24" dbus-run-session -- scripts/trousseau-test.sh \
  python3 tests/isolement/verifier.py \
    --pupitre target/debug/pupitre --fond target/debug/pupitre-fond --sortie target/isolement
