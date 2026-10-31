#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Lance tous les tests sous un compositeur Wayland sans écran (Weston headless).
set -euo pipefail
cd "$(dirname "$0")/.."
# Pas de gvfs : il monterait un système de fichiers FUSE dans le dossier temporaire.
export GIO_USE_VFS=local
# Pas de portails : le portail de documents tenterait un montage FUSE.
export GDK_DEBUG=no-portals
command -v weston >/dev/null || { echo "weston manquant : sudo apt install weston" >&2; exit 1; }
RUNTIME="$(mktemp -d)"; chmod 700 "$RUNTIME"
export XDG_RUNTIME_DIR="$RUNTIME"
SOCKET="pupitre-test-$$"
weston --backend=headless --socket="$SOCKET" --idle-time=0 >"$RUNTIME/weston.log" 2>&1 &
WESTON=$!
nettoyer() {
  local code=$?  # le résultat des tests, pas celui du ménage
  set +e  # ménage au mieux, sans interrompre
  kill "$WESTON" 2>/dev/null; wait "$WESTON" 2>/dev/null
  # Montages FUSE éventuels (portail de documents, gvfs) laissés dans le dossier temporaire.
  for m in "$RUNTIME/doc" "$RUNTIME/gvfs"; do
    if mountpoint -q "$m" 2>/dev/null; then fusermount3 -u "$m" 2>/dev/null; fi
  done
  rm -rf "$RUNTIME" || echo "avertissement : $RUNTIME non supprimé" >&2
  exit "$code"
}
trap nettoyer EXIT
for _ in $(seq 50); do [ -S "$RUNTIME/$SOCKET" ] && break; sleep 0.1; done
[ -S "$RUNTIME/$SOCKET" ] || { echo "Weston n'a pas démarré :" >&2; cat "$RUNTIME/weston.log" >&2; exit 1; }
unset DISPLAY
export WAYLAND_DISPLAY="$SOCKET" GDK_BACKEND=wayland
dbus-run-session -- scripts/trousseau-test.sh cargo test --workspace "$@" -- --include-ignored
