#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Essai de Pupitre sous Sway (Wayland) dans une VM Debian 13, entièrement automatisé :
# aucun écran à regarder, rien à écouter. La VM tourne sans fenêtre, Sway sans écran
# (backend headless) ; les touches sont simulées par wtype, les résultats lus dans des
# fichiers, l'état des fenêtres par swaymsg, l'accessibilité par AT-SPI (pyatspi).
#
# Usage : scripts/vm-sway.sh creer | demarrer | deployer | essayer | arreter | tout
#   creer     télécharge l'image Debian 13 (empreinte vérifiée), crée disque, clé et cloud-init
#   demarrer  lance la VM (2 cœurs, 2 Go, SSH sur 127.0.0.1:2222) et attend qu'elle soit prête
#   deployer  copie target/release (pupitre, pupitre-fond, pupitre-askpass) et assets/
#   essayer   lance Sway puis Pupitre et déroule les essais ; captures dans $VM/captures
#   arreter   éteint la VM
# Besoins : qemu-system-x86, qemu-utils, genisoimage, accès à /dev/kvm.
# La clé SSH de la VM lui est réservée ; l'agent SSH de l'utilisateur n'est jamais utilisé.
set -euo pipefail

DEPOT="$(cd "$(dirname "$0")/.." && pwd)"
VM="${PUPITRE_VM:-$HOME/VM/pupitre-sway}"
IMAGE=debian-13-genericcloud-amd64.qcow2
SOURCE=https://cloud.debian.org/images/cloud/trixie/latest
OPTS=(-q -i "$VM/cle-vm" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null
      -o IdentitiesOnly=yes -o IdentityAgent=none -o ConnectTimeout=5)
vm() { ssh "${OPTS[@]}" -p 2222 essai@127.0.0.1 "$@"; }
copier() { scp "${OPTS[@]}" -P 2222 "$@"; }

creer() {
  mkdir -p "$VM/seed" "$VM/captures"
  cd "$VM"
  if [ ! -f "$IMAGE" ]; then
    curl -fsSLO "$SOURCE/SHA512SUMS"
    curl -fsSLO "$SOURCE/$IMAGE"
    grep " $IMAGE\$" SHA512SUMS | sha512sum -c -
  fi
  [ -f disque.qcow2 ] || qemu-img create -q -f qcow2 -F qcow2 -b "$IMAGE" disque.qcow2 12G
  [ -f cle-vm ] || ssh-keygen -q -t ed25519 -N "" -C vm-pupitre-sway -f cle-vm
  printf 'instance-id: pupitre-sway-1\nlocal-hostname: pupitre-sway\n' > seed/meta-data
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
packages: [sway, foot, grim, wtype, wl-clipboard, jq, libgtk-4-1, libvte-2.91-gtk4-0,
           at-spi2-core, python3-pyatspi, dbus-user-session, gnome-keyring, libsecret-tools,
           fonts-dejavu-core, locales-all]
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
    qemu-system-x86_64 -name pupitre-sway -enable-kvm -cpu host -smp 2 -m 2048 \
      -drive file=disque.qcow2,if=virtio -drive file=seed.iso,media=cdrom,readonly=on \
      -netdev user,id=n0,hostfwd=tcp:127.0.0.1:2222-:22 -device virtio-net-pci,netdev=n0 \
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
  vm 'systemctl --user stop pupitre 2>/dev/null; pkill -u "$(id -u)" -f pupitre-fond; sleep 1
      mkdir -p ~/pupitre/bin ~/.config/sway' || true
  copier "$DEPOT"/target/release/{pupitre,pupitre-fond,pupitre-askpass} essai@127.0.0.1:pupitre/bin/
  copier -r "$DEPOT/assets" essai@127.0.0.1:pupitre/
  echo "Pupitre copié dans la VM"
}

essayer() {
  mkdir -p "$VM/captures"
  copier "$DEPOT/tests/vm/essai-sway.sh" essai@127.0.0.1:essai-sway.sh
  # stdin fermé : wl-copy (lancé par l'essai) ne garde pas la connexion ouverte.
  vm 'bash ~/essai-sway.sh' </dev/null
  for c in $(vm 'ls /tmp/essai/*.png' 2>/dev/null); do copier "essai@127.0.0.1:$c" "$VM/captures/"; done
  echo "Captures : $VM/captures"
}

arreter() {
  vm 'sudo systemctl poweroff' 2>/dev/null || true
  echo "VM éteinte"
}

case "${1:-}" in
  creer|demarrer|deployer|essayer|arreter) "$1" ;;
  tout) creer; demarrer; deployer; essayer ;;
  *) sed -n 3,15p "$0"; exit 2 ;;
esac
