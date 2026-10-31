# ESSAI JETABLE — environnement Debian 13 (trixie) pour mesurer VTE/GTK récents.
FROM debian:trixie
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    cargo rustc pkg-config libgtk-4-dev libvte-2.91-gtk4-dev \
    at-spi2-core dbus dbus-x11 python3-gi gir1.2-atspi-2.0 \
    xclip xdotool imagemagick fontconfig fonts-noto-color-emoji fonts-noto-cjk ca-certificates \
    && rm -rf /var/lib/apt/lists/*
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends xvfb x11-utils vim-nox locales \
    && rm -rf /var/lib/apt/lists/* && useradd -u 1000 -m essai
RUN sed -i "s/# fr_FR.UTF-8/fr_FR.UTF-8/" /etc/locale.gen && locale-gen
