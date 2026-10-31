#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Liste de contrôle Orca automatisée : Orca muet sur un écran X11 virtuel.
# Rien n'est prononcé ; le rapport et le journal d'Orca sont dans target/orca/.
# Options transmises au scénario, ex. : scripts/test-orca.sh --seulement 3 4
set -euo pipefail
cd "$(dirname "$0")/.."
for outil in xvfb-run xdotool xfwm4 orca ssh-keygen gnome-keyring-daemon; do
  command -v "$outil" >/dev/null || { echo "$outil manquant" >&2; exit 2; }
done
cargo build -q -p pupitre -p pupitre-fond --bins
rm -rf "${PUPITRE_ORCA_SORTIE:-target/orca}"
exec xvfb-run -a -s "-screen 0 1920x1080x24" dbus-run-session -- scripts/trousseau-test.sh \
  python3 tests/a11y/orca/scenario.py \
    --pupitre target/debug/pupitre --fond target/debug/pupitre-fond \
    --sortie "${PUPITRE_ORCA_SORTIE:-target/orca}" "$@"
