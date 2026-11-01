# Liste de contrôle Orca — Pupitre, étape 1

Durée : environ 15 minutes. À faire avant chaque version, avec Orca et la disposition de
clavier habituelle (fr dvorak).

Pour chaque ligne : faire la manipulation, comparer avec le résultat attendu, cocher
`[x]` si c'est conforme, sinon écrire ce qui s'est passé dans « Notes ». Une seule
différence suffit à signaler un défaut.

## Préparation (2 min)

1. Construire Pupitre depuis les sources :
   `cd ~/Documents/Projets/pupitre && cargo build --release -p pupitre -p pupitre-fond`
2. Lancer Orca s'il ne tourne pas : `orca &` (ou Super+Alt+S sous GNOME).
3. Lancer Pupitre : `./target/release/pupitre`

Rappels Orca (disposition « portable », touche Orca = Verr. Maj) :
lire la ligne courante = Orca+I ; ligne précédente / suivante en revue = Orca+U / Orca+O ;
mode apprentissage = Orca+H (Échap pour sortir).
En disposition « ordinateur de bureau » : pavé numérique 7, 8, 9.

## Vérifications

| # | Manipulation | Résultat attendu | OK | Notes |
|---|---|---|---|---|
| 1 | Lancer Pupitre. | Orca annonce la fenêtre « Terminal — Pupitre » puis lit l'invite du shell (ex. « baudouin@machine:~$ »). | [ ok] | |
| 2 | Taper `ls` puis Entrée. Lire les lignes avec Orca+U / Orca+O. | La sortie de `ls` est lue ligne par ligne, accents compris. | [ ] | |
| 3 | Ctrl+Maj+T. | Un second onglet s'ouvre ; Orca annonce le nouvel onglet et lit son invite. | [non ] | |
| 4 | Alt+1, puis Alt+2 (touches du haut, sans Maj, en fr dvorak). | À chaque fois, Orca annonce l'onglet choisi et son titre. | [ Orca lit la première fois qu'il rencontre un nouveau nom sur un onglet] | |
| 5 | Dans un onglet, taper `printf '\e]0;essai\a'` puis Entrée ; puis Ctrl+Tab jusqu'aux onglets. | L'onglet s'appelle « essai » ; Orca le lit en arrivant sur les onglets. Les flèches gauche/droite changent d'onglet et chaque titre est lu. | [ ] | |
| 6 | Ctrl+Tab jusqu'au bouton du menu, puis Entrée ; parcourir avec les flèches ; Échap. | Orca dit « Menu, bouton bascule » ; dans le menu, chaque entrée est lue avec son raccourci ; Échap ferme le menu. Un nouveau Ctrl+Tab ramène au terminal. | [ ] | |
| 7 | Taper `sleep 30` puis Entrée ; Ctrl+Maj+W. | Un dialogue s'ouvre ; Orca lit la question « Une commande est en cours dans cet onglet. Fermer quand même ? » puis « Annuler, bouton ». Échap ferme le dialogue sans fermer l'onglet. Ctrl+C arrête `sleep`. | [ ] | |
| 8 | Dans un onglet, taper `exit` puis Entrée. Puis Entrée. | Orca lit « Le shell s'est terminé (code 0). Appuyez sur Entrée pour en ouvrir un nouveau. » ; Entrée ouvre un nouveau shell dans le même onglet (invite lue). | [ ] | |
| 9 | Ouvrir `~/.config/pupitre/pupitre.toml`, écrire `historique_lignes = "x"`, enregistrer. Puis remettre `historique_lignes = 10000` et enregistrer. | Orca annonce « Configuration invalide, ligne … » sans que Pupitre ne se ferme ; après correction le message disparaît. | [ ] | |
| 10 | Ctrl+Maj+F, taper un mot présent à l'écran, Entrée ; puis un mot absent, Entrée ; puis Échap. | La saisie « Rechercher dans l'historique » est annoncée ; le mot trouvé est sélectionné ; pour le mot absent, Orca annonce « Aucun résultat » ; Échap rend le clavier au terminal (l'invite est lue avec Orca+I). | [ ] | |
| 11 | Ctrl+plus dix fois (zoom 200 %), puis taper `tput cols; tput lines`. Puis Ctrl+0. | Le texte est deux fois plus grand ; rien n'est coupé à droite ni en bas ; `tput` donne une grille plus petite. Ctrl+0 revient à la taille normale. | [ ] | |
| 12 | Dans l'onglet 1 : `sleep 3; printf '\a'` puis Entrée, et passer tout de suite à l'onglet 2 (Alt+2). Attendre 3 s, puis Ctrl+Tab jusqu'aux onglets. | L'onglet 1 affiche « ● » ; Orca le lit « …, cloche ». Revenir sur l'onglet 1 efface le ●. Aucun son, et pas plus de 3 flashs par seconde. | [ ] | |
| 13 | Dans un onglet, lancer `mc` (ou `htop`), puis F10. | F10 va au programme (il se ferme) ; le menu de Pupitre ne s'ouvre pas. | [ ] | |
| 14 | Orca+H (mode apprentissage), quelques touches, Échap ; puis Orca+I. | Les commandes d'Orca fonctionnent normalement dans Pupitre : aucune n'est captée par Pupitre. | [ ] | |

## Résultat

Date : ……………… Version (commit) : ……………… Distribution / bureau : ………………

Défauts trouvés (numéro de la ligne et ce qui s'est passé) :

-
