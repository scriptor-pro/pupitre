#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Installe l'icône et le lanceur de Pupitre pour l'utilisateur courant
# (~/.local/share), sans droits d'administration.
#
# Usage :
#   scripts/installer-bureau.sh [CHEMIN_DU_BINAIRE]   installe
#   scripts/installer-bureau.sh --retirer             désinstalle
#
# Binaire lancé par le menu : CHEMIN_DU_BINAIRE s'il est donné, sinon `pupitre`
# s'il est dans le PATH, sinon target/release/pupitre du dépôt. pupitre-fond et
# pupitre-askpass doivent se trouver dans le même dossier que lui.
set -euo pipefail

ID=fr.pupitre.Pupitre
DEPOT="$(cd "$(dirname "$0")/.." && pwd)"
DONNEES="${XDG_DATA_HOME:-$HOME/.local/share}"
ICONES="$DONNEES/icons/hicolor"
LANCEURS="$DONNEES/applications"

rafraichir() {
  if command -v gtk-update-icon-cache >/dev/null && [ -f "$ICONES/index.theme" ]; then
    gtk-update-icon-cache -q -t -f "$ICONES" || true
  fi
  if command -v update-desktop-database >/dev/null; then
    update-desktop-database -q "$LANCEURS" || true
  fi
}

if [ "${1:-}" = "--retirer" ]; then
  rm -f "$LANCEURS/$ID.desktop"
  find "$ICONES" -path "*/apps/$ID.*" -type f -delete 2>/dev/null || true
  rafraichir
  echo "Lanceur et icône de Pupitre retirés."
  exit 0
fi

if [ -n "${1:-}" ]; then
  BINAIRE="$1"
elif command -v pupitre >/dev/null; then
  BINAIRE="$(command -v pupitre)"
else
  BINAIRE="$DEPOT/target/release/pupitre"
fi
BINAIRE="$(realpath -- "$BINAIRE" 2>/dev/null || echo "$BINAIRE")"
if [ ! -x "$BINAIRE" ]; then
  echo "Binaire introuvable : $BINAIRE" >&2
  echo "Compiler d'abord : cargo build --release" >&2
  exit 1
fi
for annexe in pupitre-fond pupitre-askpass; do
  [ -x "$(dirname "$BINAIRE")/$annexe" ] || echo "Attention : $annexe absent à côté de $BINAIRE" >&2
done
case "$BINAIRE" in
  *[\"\`\$\\\|\&]*) echo "Chemin du binaire non pris en charge (caractère spécial) : $BINAIRE" >&2; exit 1 ;;
esac

# Icônes (arborescence hicolor du dépôt, copiée telle quelle).
( cd "$DEPOT/assets/icones/hicolor" && find . -type f -name "$ID.*" ) | while read -r f; do
  install -D -m 644 "$DEPOT/assets/icones/hicolor/$f" "$ICONES/$f"
done

# Lanceur : Exec pointe vers le binaire choisi, toujours entre guillemets (espaces permises).
mkdir -p "$LANCEURS"
EXEC="\"$BINAIRE\""
sed "s|^Exec=pupitre\$|Exec=$EXEC|" "$DEPOT/assets/$ID.desktop" > "$LANCEURS/$ID.desktop.tmp"
chmod 644 "$LANCEURS/$ID.desktop.tmp"
mv "$LANCEURS/$ID.desktop.tmp" "$LANCEURS/$ID.desktop"
rafraichir

echo "Pupitre installé dans le menu des applications."
echo "  Lanceur : $LANCEURS/$ID.desktop"
echo "  Icône   : $ICONES/scalable/apps/$ID.svg (et PNG de 16 à 512 px)"
echo "  Lance   : $BINAIRE"
