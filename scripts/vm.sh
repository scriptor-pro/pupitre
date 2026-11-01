#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Essai de Pupitre sous un bureau Wayland (Sway, GNOME) dans une VM Debian 13, entièrement
# automatisé : aucun écran à regarder, rien à écouter. La VM tourne sans fenêtre, le bureau
# sans écran ; les touches sont simulées (wtype sous Sway, bureau à distance de Mutter sous
# GNOME), les résultats lus dans des fichiers, l'accessibilité par AT-SPI (pyatspi).
# Déroulé dans la VM : tests/vm/essai-<bureau>.sh.
#
# Usage : scripts/vm.sh sway|gnome  creer | demarrer | deployer | essayer | arreter | tout
#   creer     télécharge l'image Debian 13 (empreinte vérifiée), crée disque, clé et cloud-init
#   demarrer  lance la VM (2 cœurs ; 2 Go pour Sway, 3 Go pour GNOME ; SSH en local) et attend
#   deployer  copie target/release (pupitre, pupitre-fond, pupitre-askpass) et assets/
#   essayer   lance Sway puis Pupitre et déroule les essais ; captures dans $VM/captures
#   arreter   éteint la VM
# Besoins : qemu-system-x86, qemu-utils, genisoimage, accès à /dev/kvm.
# La clé SSH de la VM lui est réservée ; l'agent SSH de l'utilisateur n'est jamais utilisé.
set -euo pipefail

DEPOT="$(cd "$(dirname "$0")/.." && pwd)"
BUREAU="${1:-}"
shift || true
COMMUNS="grim jq libgtk-4-1 libvte-2.91-gtk4-0 at-spi2-core python3-pyatspi dbus-user-session
  gnome-keyring libsecret-tools fonts-dejavu-core locales-all"
case "$BUREAU" in
  sway) PORT=2222; MEMOIRE=2048; PAQUETS="sway foot wtype wl-clipboard $COMMUNS" ;;
  gnome) PORT=2223; MEMOIRE=3072
    PAQUETS="gnome-shell gnome-session-bin python3-gi gir1.2-glib-2.0 wl-clipboard
      libgl1-mesa-dri xdg-desktop-portal-gnome $COMMUNS" ;;
  *) sed -n 3,17p "$0"; exit 2 ;;
esac
PAQUETS="$(echo $PAQUETS | sed 's/ /, /g')"
VM="${PUPITRE_VM:-$HOME/VM/pupitre-$BUREAU}"
IMAGE=debian-13-genericcloud-amd64.qcow2
SOURCE=https://cloud.debian.org/images/cloud/trixie/latest
OPTS=(-q -i "$VM/cle-vm" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null
      -o IdentitiesOnly=yes -o IdentityAgent=none -o ConnectTimeout=5)
vm() { ssh "${OPTS[@]}" -p "$PORT" essai@127.0.0.1 "$@"; }
copier() { scp "${OPTS[@]}" -P "$PORT" "$@"; }

creer() {
  mkdir -p "$VM/seed" "$VM/captures"
  cd "$VM"
  if [ ! -f "$IMAGE" ]; then
    curl -fsSLO "$SOURCE/SHA512SUMS"
    curl -fsSLO "$SOURCE/$IMAGE"
    grep " $IMAGE\$" SHA512SUMS | sha512sum -c -
  fi
  [ -f disque.qcow2 ] || qemu-img create -q -f qcow2 -F qcow2 -b "$IMAGE" disque.qcow2 12G
  [ -f cle-vm ] || ssh-keygen -q -t ed25519 -N "" -C "vm-pupitre-$BUREAU" -f cle-vm
  printf 'instance-id: pupitre-%s-1\nlocal-hostname: pupitre-%s\n' "$BUREAU" "$BUREAU" > seed/meta-data
  cat > seed/user-data <<EOF
#cloud-config
users:
  - name: essai
    shell: /bin/bash
    sudo: ALL=(ALL) NOPASSWD:ALL
    lock_passwd: true
    ssh_authorized_keys:
      - $(cat cle-vm.pub)
package_update: true
packages: [$PAQUETS]
runcmd:
  - [loginctl, enable-linger, essai]
  - [touch, /var/lib/pret-pour-pupitre]
EOF
  genisoimage -quiet -output seed.iso -volid cidata -joliet -rock seed/user-data seed/meta-data
  echo "VM prête à démarrer dans $VM"
}

demarrer() {
  cd "$VM"
  if [ -f qemu.pid ] && kill -0 "$(cat qemu.pid)" 2>/dev/null; then
    echo "VM déjà en marche"
  else
    qemu-system-x86_64 -name "pupitre-$BUREAU" -enable-kvm -cpu host -smp 2 -m "$MEMOIRE" \
      -drive file=disque.qcow2,if=virtio -drive file=seed.iso,media=cdrom,readonly=on \
      -netdev user,id=n0,hostfwd=tcp:127.0.0.1:$PORT-:22 -device virtio-net-pci,netdev=n0 \
      -display none -serial file:console.log -pidfile qemu.pid -daemonize
  fi
  for _ in $(seq 120); do
    vm test -f /var/lib/pret-pour-pupitre 2>/dev/null && { echo "VM prête"; return; }
    sleep 10
  done
  echo "VM pas prête après 20 min (voir $VM/console.log)" >&2
  return 1
}

deployer() {
  # Un binaire en cours d'exécution ne peut pas être remplacé : Pupitre et son fond
  # sont arrêtés d'abord.
  vm 'systemctl --user stop pupitre 2>/dev/null; pkill -u "$(id -u)" -f '[p]upitre-fond'; sleep 1
      mkdir -p ~/pupitre/bin' || true
  copier "$DEPOT"/target/release/{pupitre,pupitre-fond,pupitre-askpass} essai@127.0.0.1:pupitre/bin/
  copier -r "$DEPOT/assets" essai@127.0.0.1:pupitre/
  echo "Pupitre copié dans la VM"
}

essayer() {
  mkdir -p "$VM/captures"
  copier "$DEPOT"/tests/vm/{essai-$BUREAU.sh,commun.sh,gnome.py} essai@127.0.0.1:
  # stdin fermé : wl-copy (lancé par l'essai) ne garde pas la connexion ouverte.
  local code=0
  vm "bash ~/essai-$BUREAU.sh" </dev/null || code=$?
  for c in $(vm 'ls /tmp/essai/*.png' 2>/dev/null); do copier "essai@127.0.0.1:$c" "$VM/captures/"; done
  echo "Captures : $VM/captures"
  return "$code"
}

arreter() {
  vm 'sudo systemctl poweroff' 2>/dev/null || true
  echo "VM éteinte"
}

case "${1:-}" in
  creer|demarrer|deployer|essayer|arreter) "$1" ;;
  tout) creer; demarrer; deployer; essayer ;;
  *) sed -n 3,17p "$0"; exit 2 ;;
esac
