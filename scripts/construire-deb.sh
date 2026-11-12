#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Construit le paquet Debian de Pupitre : target/deb/pupitre_<version>-<révision>_<arch>.deb
#
# Usage : scripts/construire-deb.sh [--sans-compiler]
#   --sans-compiler  réutilise target/release (déjà compilé avec `cargo build --release --locked`)
#
# Besoins : cargo, dpkg-dev (dpkg-shlibdeps), binutils (strip). Facultatif : lintian, appelé
# à la fin s'il est installé.
#
# Contenu du paquet :
#   /usr/bin/pupitre, pupitre-fond, pupitre-askpass   (pupitre cherche les deux autres à côté de lui)
#   /usr/share/pupitre/fonts, /usr/share/pupitre/shell (cherchés là par le programme installé)
#   /usr/share/applications, /usr/share/icons/hicolor  (lanceur et icônes)
#   /usr/share/doc/pupitre                             (copyright, changelog, README)
# L'aide, la configuration par défaut et l'invite par défaut sont intégrées aux exécutables.
set -euo pipefail

DEPOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$DEPOT"

PAQUET=pupitre
REVISION=1
MAINTENEUR="scriptor-pro <bvh@scriptor.pro>"
SITE="https://github.com/scriptor-pro/pupitre"
ID=fr.pupitre.Pupitre
BINAIRES=(pupitre pupitre-fond pupitre-askpass)

VERSION="$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml)"
[ -n "$VERSION" ] || { echo "pupitre: version introuvable dans Cargo.toml" >&2; exit 1; }
ARCH="$(dpkg --print-architecture)"
NOM="${PAQUET}_${VERSION}-${REVISION}_${ARCH}"
SORTIE="$DEPOT/target/deb"
RACINE="$SORTIE/$NOM"

if [ "${1:-}" != "--sans-compiler" ]; then
  cargo build --release --locked
fi
for b in "${BINAIRES[@]}"; do
  [ -x "target/release/$b" ] || { echo "pupitre: target/release/$b absent" >&2; exit 1; }
done

rm -rf "$RACINE" "$SORTIE/$NOM.deb"
mkdir -p "$RACINE/DEBIAN" "$RACINE/usr/bin" "$RACINE/usr/share/$PAQUET" \
  "$RACINE/usr/share/applications" "$RACINE/usr/share/doc/$PAQUET"

# Exécutables, sans symboles de débogage.
for b in "${BINAIRES[@]}"; do
  install -m 0755 "target/release/$b" "$RACINE/usr/bin/$b"
  strip --strip-unneeded --remove-section=.comment --remove-section=.note "$RACINE/usr/bin/$b"
done

# Ressources lues à l'exécution.
cp -r assets/fonts assets/shell "$RACINE/usr/share/$PAQUET/"
install -m 0644 "assets/$ID.desktop" "$RACINE/usr/share/applications/$ID.desktop"
mkdir -p "$RACINE/usr/share/icons"
cp -r assets/icones/hicolor "$RACINE/usr/share/icons/"

# Documentation.
install -m 0644 README.md "$RACINE/usr/share/doc/$PAQUET/README.md"
cat > "$RACINE/usr/share/doc/$PAQUET/copyright" <<EOF
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: Pupitre
Upstream-Contact: $MAINTENEUR
Source: $SITE

Files: *
Copyright: 2026 scriptor-pro
License: GPL-3.0-or-later
 This program is free software: you can redistribute it and/or modify
 it under the terms of the GNU General Public License as published by
 the Free Software Foundation, either version 3 of the License, or
 (at your option) any later version.
 .
 On Debian systems, the full text of the GNU General Public License
 version 3 can be found in the file '/usr/share/common-licenses/GPL-3'.

Files: usr/share/pupitre/fonts/B612Mono-*
Copyright: 2012 The B612 Project Authors (https://github.com/polarsys/b612)
License: OFL-1.1
 The B612 Mono font is licensed under the SIL Open Font License, Version 1.1.
 The full text is in /usr/share/pupitre/fonts/OFL-B612.txt.

Files: usr/share/pupitre/fonts/AtkinsonHyperlegibleMono-*
Copyright: 2020-2024 The Atkinson Hyperlegible Mono Project Authors
 (https://github.com/googlefonts/atkinson-hyperlegible-next-mono)
License: OFL-1.1
 The Atkinson Hyperlegible Mono font is licensed under the SIL Open Font
 License, Version 1.1. The full text is in
 /usr/share/pupitre/fonts/OFL-AtkinsonHyperlegibleMono.txt.
EOF
DATE="$(date -R -d "@$(git log -1 --format=%ct 2>/dev/null || date +%s)")"
cat > "$SORTIE/changelog.Debian" <<EOF
$PAQUET ($VERSION-$REVISION) unstable; urgency=medium

  * Paquet construit depuis les sources.

 -- $MAINTENEUR  $DATE
EOF
gzip -9n -c "$SORTIE/changelog.Debian" > "$RACINE/usr/share/doc/$PAQUET/changelog.Debian.gz"
rm "$SORTIE/changelog.Debian"

# Droits uniformes : dossiers 0755, fichiers 0644, sauf les exécutables.
find "$RACINE" -type d -exec chmod 0755 {} +
find "$RACINE/usr/share" -type f -exec chmod 0644 {} +

# Dépendances des bibliothèques, calculées par dpkg-shlibdeps (dans un dossier de travail qui
# imite l'arborescence debian/ qu'il attend).
TRAVAIL="$(mktemp -d)"
trap 'rm -rf "$TRAVAIL"' EXIT
mkdir -p "$TRAVAIL/debian"
printf 'Source: %s\n\nPackage: %s\nArchitecture: any\n' "$PAQUET" "$PAQUET" > "$TRAVAIL/debian/control"
SHLIBS="$(cd "$TRAVAIL" && dpkg-shlibdeps -O "${BINAIRES[@]/#/$RACINE/usr/bin/}" 2>/dev/null \
  | sed -n 's/^shlibs:Depends=//p')"
[ -n "$SHLIBS" ] || { echo "pupitre: dpkg-shlibdeps n'a rien rendu" >&2; exit 1; }
# GTK et VTE : les versions minimales de Pupitre (4.18, 0.80) remplacent celles, plus basses,
# déduites des symboles.
SHLIBS="$(printf '%s' "$SHLIBS" | tr ',' '\n' | sed 's/^ *//' \
  | grep -vE '^(libgtk-4-1|libvte-2\.91-gtk4-0) ' | paste -sd, - | sed 's/,/, /g')"

TAILLE="$(du -sk --exclude=DEBIAN "$RACINE" | cut -f1)"
cat > "$RACINE/DEBIAN/control" <<EOF
Package: $PAQUET
Version: $VERSION-$REVISION
Architecture: $ARCH
Maintainer: $MAINTENEUR
Installed-Size: $TAILLE
Depends: $SHLIBS, libgtk-4-1 (>= 4.18), libvte-2.91-gtk4-0 (>= 0.80), openssh-client
Recommends: starship, gnome-keyring | kwalletmanager | keepassxc
Section: x11
Priority: optional
Homepage: $SITE
Description: accessible tabbed terminal emulator
 Pupitre is a terminal emulator for Linux (X11 and Wayland) built with GTK 4
 and VTE, designed first for accessibility: everything works from the
 keyboard, the Orca screen reader reads the output, announces finished
 commands and can jump from one command to the next, and every colour theme
 meets WCAG AA contrast.
 .
 Shells run in a separate background process, so they survive a crash of the
 window. Pupitre keeps an SSH agent for its shells, remembers key
 passphrases in the system keyring, and gives bash, zsh, fish, nushell and
 elvish a Starship prompt without touching the user's own configuration
 files.
 .
 The interface is in French; other languages are planned.
EOF
# Somme de contrôle de chaque fichier installé.
(cd "$RACINE" && find usr -type f -print0 | sort -z | xargs -0 md5sum > DEBIAN/md5sums)
chmod 0644 "$RACINE/DEBIAN/control" "$RACINE/DEBIAN/md5sums"

dpkg-deb --root-owner-group -Zxz --build "$RACINE" "$SORTIE/$NOM.deb" >/dev/null
rm -rf "$RACINE"
echo "$SORTIE/$NOM.deb"

if command -v lintian >/dev/null; then
  lintian --tag-display-limit 0 "$SORTIE/$NOM.deb" || true
fi
