# Task 0 — VTE alimenté par `feed()` : résultats

Date : 2026-10-31. Environnement : conteneur Debian 13 (GTK 4.18, VTE 0.80), Xvfb, sans gestionnaire de fenêtres.
Programme : `src/main.rs` (PTY `portable-pty` géré par le programme, aucun `spawn` VTE). Script dans le PTY : `enfant.sh`. Pilote xdotool : `pilote.sh`. Sorties brutes : `sortie/`.

**Verdict : les 6 critères passent. L'architecture prévue (le processus de fond possède le PTY, l'interface utilise `feed()`) est validée.**

| Critère | Attendu | Obtenu | |
|---|---|---|---|
| Saisie clavier → `commit` | le shell reçoit la ligne tapée | `bonjour clavier` reçu, un `commit` par touche, Entrée = `\r` | ✅ |
| Réponse DA1 `ESC[c` revient par `commit` | `ESC[?…c` | `ESC[?61;1;21;22;28c` | ✅ |
| Réponse DECRQM 2004 revient par `commit` | `ESC[?2004;…$y` | `ESC[?2004;2$y` | ✅ |
| Collage sécurisé (`ESC[?2004h` + Ctrl+Maj+V) | `ESC[200~…ESC[201~` | `ESC[200~texte colle ESC[201~` | ✅ |
| Souris SGR (`?1000h ?1006h` + clic) | `ESC[<0;…M` | `ESC[<0;10;4M` puis `ESC[<0;10;4m` (relâchement) | ✅ |
| Sonde AT-SPI | rôle `terminal`, texte lisible | rôle `terminal`, texte complet avec accents (`sortie/sonde.txt`) | ✅ |
| vim plein écran redessiné après redimensionnement | `&columns`×`&lines` = grille VTE | 95×31 → 63×20 → 109×36 ; vim voit 63×20 puis 109×36 ; captures nettes | ✅ |

## Deux pièges découverts (à respecter dans Pupitre)

1. **VTE n'a pas de propriété `column-count`/`row-count`** : `connect_notify_local("column-count")` ne se déclenche jamais (premier essai : vim est resté en 80×24). Il faut être prévenu par `size_allocate`.
2. **Ne pas dériver `vte4::Terminal` en Rust** : avec `vte4` 0.9.0 (et 0.10.0, même code), toute sous-classe fait planter le programme au premier `commit` (`free(): double free detected`). Le trampoline `terminal_commit` enveloppe le texte de VTE dans `GString::from_ptr_and_len_unchecked`, qui en prend possession et le libère alors qu'il appartient à VTE. Bogue à signaler en amont (gtk-rs/vte4-rs).

**Solution validée** : un petit conteneur `gtk::Widget` (ici `CadreTerminal`) qui contient le `vte4::Terminal` comme enfant, relaie `measure` et, dans `size_allocate`, alloue l'enfant puis compare `column_count()`/`row_count()` à la grille précédente et envoie la nouvelle taille au PTY. Lecteur d'écran non affecté (la sonde trouve toujours le rôle `terminal`).

## Remarques

- Ctrl+Maj+V doit être fourni par l'application (`paste_clipboard()`), VTE n'a pas de raccourci propre.
- `xdotool windowfocus` renvoie `BadMatch` sans gestionnaire de fenêtres, sans conséquence : le focus clavier est correct.
- Image Docker reconstruite (`debian13/Dockerfile` + vim, locales) ; la ligne `credsStore: desktop` de `~/.docker/config.json` a été retirée le 2026-10-31 (sauvegarde `config.json.bak`).
