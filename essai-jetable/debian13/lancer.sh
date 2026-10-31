#!/bin/bash
# ESSAI JETABLE — lance l'essai dans le conteneur Debian 13 sur un écran virtuel (Xvfb).
set -e
ICI="$(cd "$(dirname "$0")/.." && pwd)"
docker run --rm --user 1000:1000 \
  -v "$HOME/.local/share/fonts:/home/essai/.local/share/fonts:ro" \
  -e CARGO_HOME=/work/.cargo-trixie -e CARGO_TARGET_DIR=/work/target-trixie \
  -e ESSAI_POLICE -v "$ICI:/work" -w /work pupitre-essai-trixie \
  bash -c '
    Xvfb :99 -screen 0 1920x1080x24 >/dev/null 2>&1 & export DISPLAY=:99; sleep 1
    dbus-run-session -- bash -c "
      /usr/libexec/at-spi-bus-launcher --launch-immediately & sleep 1
      cargo build --release --features recent 2>&1 | grep -E \"^error|Finished\" -A6
      ./target-trixie/release/essai_vte"'
