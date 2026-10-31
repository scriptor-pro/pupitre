# Résultats des essais jetables — VTE sur Debian 12 et Debian 13, 2026-10-25

Méthode : mini-terminal Rust (gtk4 0.10 + vte4 0.9) ; `test.sh` interroge le terminal par séquences
d'échappement, l'application note signaux et effets, `sonde_a11y.py` lit l'arbre AT-SPI.
Debian 12 : sur l'hôte. Debian 13 : conteneur Docker (`debian13/`), écran virtuel Xvfb.
Fichiers : `resultats-*-debian12.txt`, `resultats-*-debian13.txt`, `capture-*.png`.

| Fonction | Debian 12 (GTK 4.8 / VTE 0.70) | Debian 13 (GTK 4.18 / VTE 0.80) |
|---|---|---|
| **Texte lisible par lecteur d'écran** | ❌ `filler` sans texte | ✅ rôle `terminal`, texte exposé |
| **API disposition clavier GTK** | ❌ absente | ✅ `layout_names` / `active_layout_index` |
| Couleurs exactes, 256 couleurs, palette 16 | ✅ | ✅ |
| Liens OSC 8, dossier OSC 7, titre, cloche | ✅ | ✅ |
| Collage sécurisé, focus, souris SGR | ✅ | ✅ |
| Requêtes couleurs OSC 10/11/4 | ✅ | ✅ |
| Propriétés de terminal (termprops) | ❌ | ✅ (reçues : `vte.cwd`, `xterm.title`) |
| OSC 52 lecture | bloquée (souhaité) | bloquée (souhaité) |
| **OSC 52 écriture** | ❌ | ❌ |
| **Notifications OSC 9 / 777** | ❌ | ❌ |
| **Marques OSC 133** (et OSC 666 vte.shell.*) | ❌ | ❌ aucune termprop reçue |
| **Synchronisation de l'affichage (2026)** | ❌ (DECRQM 4) | ❌ (DECRQM 4) |
| **Protocole clavier kitty / modifyOtherKeys** | ❌ | ❌ |
| **Graphèmes complexes (👍🏽, 🇫🇷)** | ❌ | ❌ |
| **Ligatures** (Ioskeley, qui a `calt`) | ❌ | ❌ |
| Images sixel / kitty | ❌ | ❌ |

Rattrapables par le processus de fond (placé entre le PTY et VTE) : OSC 52 écriture, OSC 9/777, OSC 133.
Non rattrapables sans modifier VTE : synchronisation, clavier kitty, graphèmes, ligatures, images.

Polices : B612 Mono et IBM Plex Mono n'ont pas de ligatures ; Nordwand (`liga`) et Ioskeley (`calt`) en ont.
