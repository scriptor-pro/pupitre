#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Lance tous les tests sous un compositeur Wayland sans écran (Weston headless).
set -euo pipefail
cd "$(dirname "$0")/.."
command -v weston >/dev/null || { echo "weston manquant : sudo apt install weston" >&2; exit 1; }
RUNTIME="$(mktemp -d)"; chmod 700 "$RUNTIME"
export XDG_RUNTIME_DIR="$RUNTIME"
SOCKET="pupitre-test-$$"
weston --backend=headless --socket="$SOCKET" --idle-time=0 >"$RUNTIME/weston.log" 2>&1 &
WESTON=$!
trap 'kill "$WESTON" 2>/dev/null; wait "$WESTON" 2>/dev/null; rm -rf "$RUNTIME"' EXIT
for _ in $(seq 50); do [ -S "$RUNTIME/$SOCKET" ] && break; sleep 0.1; done
[ -S "$RUNTIME/$SOCKET" ] || { echo "Weston n'a pas démarré :" >&2; cat "$RUNTIME/weston.log" >&2; exit 1; }
unset DISPLAY
export WAYLAND_DISPLAY="$SOCKET" GDK_BACKEND=wayland
dbus-run-session -- cargo test --workspace "$@" -- --include-ignored
