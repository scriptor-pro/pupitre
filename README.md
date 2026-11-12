# Pupitre

Émulateur de terminal à onglets pour Linux (X11 et Wayland), écrit en Rust avec GTK 4 et VTE,
pensé d'abord pour l'accessibilité :

- tout se fait au clavier ;
- le lecteur d'écran Orca lit la sortie, annonce la fin des commandes longues et permet d'aller
  d'une commande à l'autre (Ctrl+Maj+↑ et Ctrl+Maj+↓) ;
- chaque thème de couleurs respecte le contraste WCAG AA ;
- les shells tournent dans un processus de fond séparé : ils survivent à un plantage de la
  fenêtre ;
- SSH : un agent garanti pour les shells de Pupitre, phrases de passe mémorisées dans le
  trousseau du système, connexions mémorisées ;
- invite Starship prête à l'emploi pour bash, zsh, fish, nushell et elvish, sans toucher à vos
  propres fichiers de configuration.

L'interface est en français ; d'autres langues sont prévues. L'aide intégrée s'ouvre avec
Ctrl+Maj+H.

## Installer le paquet Debian

Debian 13 (trixie) ou plus récent, ou une distribution qui en dérive (GTK 4.18 et VTE 0.80 au
minimum) :

```
sudo apt install ./pupitre_0.1.0-1_amd64.deb
```

`apt` installe aussi les dépendances. Starship et un trousseau (GNOME Keyring, KWallet ou
KeePassXC) sont recommandés : sans Starship, chaque shell garde son invite habituelle ; sans
trousseau, les phrases de passe SSH ne sont pas mémorisées.

Désinstaller : `sudo apt remove pupitre`. Vos réglages, dans `~/.config/pupitre/`, restent en
place.

## Compiler depuis les sources

Outils et bibliothèques (Debian 13) :

```
sudo apt install build-essential pkg-config libgtk-4-dev libvte-2.91-gtk4-dev
```

Rust stable, installé avec [rustup](https://rustup.rs) (la version de Rust de Debian 13 convient
aussi : 1.85 au minimum).

Compiler :

```
cargo build --release --locked
```

Les trois exécutables (`pupitre`, `pupitre-fond`, `pupitre-askpass`) sont dans `target/release/`
et doivent rester dans le même dossier. Lancer : `target/release/pupitre`. Pour ajouter Pupitre
au menu des applications sans paquet : `scripts/installer-bureau.sh`.

## Construire le paquet Debian

```
sudo apt install dpkg-dev lintian
scripts/construire-deb.sh
```

Le paquet est écrit dans `target/deb/`. Le script compile d'abord ; `--sans-compiler` réutilise
`target/release`.

## Tests

```
cargo test --workspace
```

Les tests graphiques tournent sous un écran virtuel (Xvfb) ; d'autres essais existent pour
Wayland, Orca et des machines virtuelles (`scripts/`). La conception et les plans sont dans
`docs/`.

## Licence

GPL-3.0 ou ultérieure (voir `LICENSE`). Les polices embarquées, B612 Mono et Atkinson Hyperlegible
Mono, sont sous licence SIL Open Font License 1.1 (`assets/fonts/OFL-*.txt`).
