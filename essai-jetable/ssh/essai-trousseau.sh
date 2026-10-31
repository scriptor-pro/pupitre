#!/bin/bash
# Essai du trousseau sous bus de session jetable et HOME jetable. Argument : chemin du binaire.
BIN=$1
T=$(mktemp -d); export HOME=$T XDG_DATA_HOME=$T/data XDG_RUNTIME_DIR=$T/run XDG_CONFIG_HOME=$T/cfg XDG_CACHE_HOME=$T/cache
mkdir -p $XDG_DATA_HOME $XDG_RUNTIME_DIR $XDG_CONFIG_HOME $XDG_CACHE_HOME; chmod 700 $XDG_RUNTIME_DIR
unset DISPLAY WAYLAND_DISPLAY SSH_AUTH_SOCK
cat > $T/bus-nu.conf <<XML
<busconfig><type>session</type><listen>unix:tmpdir=$T</listen>
<auth>EXTERNAL</auth><policy context="default"><allow send_destination="*" eavesdrop="true"/><allow eavesdrop="true"/><allow own="*"/></policy></busconfig>
XML
echo "=== bus sans AUCUN service activable (aucun org.freedesktop.secrets)"
dbus-run-session --config-file=$T/bus-nu.conf -- bash -c "time timeout 20 $BIN absent; echo code=\$?" 2>&1
echo "=== bus normal, activation automatique de gnome-keyring (sans déverrouillage, HOME neuf)"
dbus-run-session -- bash -c "time timeout 20 $BIN verrou; echo code=\$?" 2>&1
echo "=== gnome-keyring-daemon déverrouillé par mot de passe sur stdin (HOME neuf)"
T2=$(mktemp -d); export HOME=$T2 XDG_DATA_HOME=$T2/data; mkdir -p $XDG_DATA_HOME
dbus-run-session -- bash -c "
  echo -n motdepasse | gnome-keyring-daemon --unlock --components=secrets >/dev/null 2>&1
  sleep 1
  time timeout 20 $BIN ok; echo code=\$?" 2>&1
echo "=== même HOME, nouveau bus, daemon démarré SANS déverrouillage : collection verrouillée"
dbus-run-session -- bash -c "
  gnome-keyring-daemon --start --components=secrets >/dev/null 2>&1
  sleep 1
  time timeout 20 $BIN verrou; echo code=\$?" 2>&1
