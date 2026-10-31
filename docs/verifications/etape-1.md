# Vérifications de l'étape 1

## Texte atténué (SGR 2) — Task 8, 2026-10-31

Mesure : `crates/pupitre/tests/gui_fenetre.rs`, test `texte_attenue_lisible` (pavés pleins U+2588
rendus par VTE 0.80 / GTK 4.18, capture de la texture, X11 et Wayland).

VTE affiche le texte atténué en **multipliant la couleur par 2/3** (vers le noir).

| Thème | Texte atténué | Couleurs de palette sous 4,5:1 une fois atténuées |
|---|---|---|
| Clair | #4c4f69 → #333546 (10,68:1) | aucune (toutes ≈ 9:1 : l'atténuation assombrit, donc renforce) |
| Sombre | #cdd6f4 → #898fa3 (5,09:1) | 1 et 9 (3,39:1), 4 et 12 (3,66:1), 7 (4,24:1), 8 (2,59:1), 15 (3,89:1) |

**Décision** (conception, section 7 : « le remplacer par une couleur réelle conforme ou le
désactiver ») : l'atténuation est **désactivée pour tout thème où une couleur de texte atténuée
passe sous 4,5:1** — aujourd'hui le thème sombre. Le paramètre `2` est retiré des séquences SGR
avant `feed()` (`crates/pupitre/src/attenuation.rs`) ; le texte s'affiche dans sa couleur normale.
Le thème clair garde l'atténuation. Les thèmes de l'utilisateur sont jugés par le même calcul.
