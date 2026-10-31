#!/bin/bash
# ESSAI JETABLE — compile et lance l'essai feed() dans le conteneur Debian 13, sur Xvfb.
set -e
ICI="$(cd "$(dirname "$0")/.." && pwd)"
rm -rf "$ICI/feed/sortie"; mkdir -p "$ICI/feed/sortie"
docker run --rm --user 1000:1000 \
  -v "$HOME/.local/share/fonts:/home/essai/.local/share/fonts:ro" \
  -e CARGO_HOME=/work/.cargo-trixie -e CARGO_TARGET_DIR=/work/target-trixie-feed -e LANG=C.UTF-8 \
  -v "$ICI:/work" -w /work/feed pupitre-essai-trixie \
  bash -c '
    Xvfb :99 -screen 0 1920x1080x24 >/dev/null 2>&1 & export DISPLAY=:99; sleep 1
    dbus-run-session -- bash -c "
      /usr/libexec/at-spi-bus-launcher --launch-immediately & sleep 1
      cargo build --release 2>&1 | grep -E \"^(error|warning: unused)|Finished\" -A6
      GTK_A11Y=atspi ../target-trixie-feed/release/essai_feed 2> sortie/app-stderr.txt &
      APP=\$!; bash pilote.sh; wait \$APP; echo \"code de sortie app : \$?\" >> sortie/pilote.txt"'
