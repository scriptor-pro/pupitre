#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Préfixe pour les tests du trousseau : à lancer DANS un bus D-Bus déjà privé
# (dbus-run-session). Démarre un gnome-keyring-daemon jetable, déverrouillé, avec
# un HOME temporaire (seulement pour lui : la commande garde le vrai HOME).
# Usage : dbus-run-session -- scripts/trousseau-test.sh commande...
set -euo pipefail
[ -n "${DBUS_SESSION_BUS_ADDRESS:-}" ] || { echo "trousseau-test.sh : lancer sous dbus-run-session" >&2; exit 1; }
command -v gnome-keyring-daemon >/dev/null || { echo "gnome-keyring manquant : sudo apt install gnome-keyring" >&2; exit 1; }

TMP="$(mktemp -d)"
DEMON=""
nettoyer() {
  local code=$?
  set +e
  [ -n "$DEMON" ] && kill "$DEMON" 2>/dev/null
  rm -rf "$TMP" || echo "avertissement : $TMP non supprimé" >&2
  exit "$code"
}
trap nettoyer EXIT

mkdir -p "$TMP/run" "$TMP/data" "$TMP/cfg" "$TMP/cache"; chmod 700 "$TMP/run"
# --daemonize n'imprime rien ici : on n'exporte rien, le démon est trouvé via le bus.
echo -n test | HOME="$TMP" XDG_DATA_HOME="$TMP/data" XDG_RUNTIME_DIR="$TMP/run" \
  XDG_CONFIG_HOME="$TMP/cfg" XDG_CACHE_HOME="$TMP/cache" \
  gnome-keyring-daemon --unlock --components=secrets --daemonize >/dev/null

# Le propriétaire de org.freedesktop.secrets sur ce bus doit être CE démon
# (reconnu à son HOME temporaire), jamais un autre trousseau.
PROPRIETAIRE=""
for _ in $(seq 50); do
  PROPRIETAIRE="$(busctl --user status org.freedesktop.secrets 2>/dev/null | sed -n 's/^PID=//p')" || true
  [ -n "$PROPRIETAIRE" ] && break
  sleep 0.1
done
if [ -z "$PROPRIETAIRE" ] || ! tr '\0' '\n' <"/proc/$PROPRIETAIRE/environ" 2>/dev/null | grep -qx "HOME=$TMP"; then
  echo "trousseau-test.sh : le Secret Service du bus n'est pas le démon jetable" >&2
  exit 1
fi
DEMON="$PROPRIETAIRE"
"$@"
