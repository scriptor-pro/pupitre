# Liste de contrôle Orca — Pupitre (étapes 1 et 2)

## Vérification automatique (à faire avant chaque version)

```sh
cd ~/Documents/Projets/pupitre && scripts/test-orca.sh
```

Durée : environ 4 à 5 minutes. **Rien n'est prononcé** et rien ne s'affiche sur l'écran :
le script ouvre un écran virtuel (Xvfb), y lance un gestionnaire de fenêtres (xfwm4),
Orca avec une synthèse muette, puis Pupitre. Il tape les touches à votre place
(xdotool) et compare ce qu'Orca aurait dit (son journal) avec le résultat attendu. Votre
Orca, votre configuration et vos dossiers ne sont pas touchés (dossiers temporaires).

Résultat : une ligne par vérification, `OK` ou `ÉCHEC` ; en cas d'échec, la raison et les
dernières phrases d'Orca. Le rapport (`rapport.txt`) et le journal complet d'Orca
(`orca.log`) restent dans `target/orca/`. Code de sortie 0 si tout passe.

Refaire une seule vérification (la 1, le lancement, est toujours faite) :
`scripts/test-orca.sh --seulement 5 12`

Outils nécessaires : `orca`, `xvfb-run` (paquet xvfb), `xdotool`, `xfwm4`, et pour les
vérifications SSH `ssh-keygen` et `gnome-keyring-daemon`.

Les vérifications 15 et 16 (SSH) tournent avec un trousseau jetable (`scripts/trousseau-test.sh`,
démon `gnome-keyring-daemon` du bus privé du test), une clé SSH de test créée dans le dossier
personnel temporaire, et sans `SSH_AUTH_SOCK` : Pupitre y lance son propre `ssh-agent` dans le
dossier temporaire. Ni votre agent SSH ni votre trousseau ne sont touchés.

Fichiers : `tests/a11y/orca/scenario.py` (les vérifications), `lancer_orca.py` (lance
Orca muet), `pupitre_muet.py` (la synthèse muette).

## Ce qui est vérifié

Orca est réglé en disposition « ordinateur de bureau » (touche Orca = Inser ; lire la
ligne courante = pavé 8 ; ligne précédente = pavé 7).

| # | Manipulation | Résultat attendu |
|---|---|---|
| 1 | Lancer Pupitre. | Orca annonce « Terminal — Pupitre » puis lit l'invite du shell. |
| 2 | `printf` d'une ligne accentuée ; pavé 7 (ligne précédente). | La ligne « ligne été à ça » est lue, accents compris. |
| 3 | Ctrl+Maj+T. | Le nouvel onglet est annoncé (« Page 2 ») et son invite lue. |
| 4 | Alt+1, puis Alt+2. | Chaque fois, l'onglet choisi est annoncé. |
| 5 | Titre `printf '\e]0;essai\a'` ; Ctrl+Tab jusqu'aux onglets ; flèches gauche et droite. | « Onglets, groupement, essai, onglet 2 sur 2 » ; chaque flèche change d'onglet, le clavier reste sur les onglets et l'onglet choisi est lu. |
| 6 | Ctrl+Tab jusqu'au menu, Entrée, flèche bas, Échap ; Ctrl+Tab. | « Menu, bouton à deux états » ; les entrées sont lues avec leur raccourci ; Échap ferme ; Ctrl+Tab ramène au terminal. |
| 7 | `sleep 30` ; Ctrl+Maj+W ; Échap. | « Fermer ? boîte de dialogue. Une commande est en cours dans cet onglet. Fermer quand même ? » puis « Annuler, bouton » ; Échap ferme le dialogue sans fermer l'onglet. |
| 8 | `exit`, puis Entrée. | « Le shell s'est terminé (code 0). Appuyez sur Entrée pour en ouvrir un nouveau. » ; Entrée ouvre un shell (invite lue). |
| 9 | Écrire `historique_lignes = "x"` dans `pupitre.toml`, puis remettre la valeur. | « Configuration invalide, ligne 24 : … » ; Pupitre reste ouvert. |
| 10 | Ctrl+Maj+F, un mot présent, Entrée ; un mot absent, Entrée ; Échap ; pavé 8. | La saisie « Rechercher dans l'historique » est annoncée ; « Aucun résultat » pour le mot absent ; après Échap, l'invite est lue (clavier rendu au terminal). |
| 11 | Ctrl+plus dix fois, `tput cols` ; Ctrl+0. | La grille a nettement moins de colonnes ; Ctrl+0 rend le nombre d'origine. |
| 12 | Cloche dans l'onglet 1 pendant qu'on est sur l'onglet 2 ; Ctrl+Tab jusqu'aux onglets. | « … ; cloche dans : Terminal (onglet 1) ». |
| 13 | `cat -v`, puis F10. | F10 arrive au programme (`^[[21~`) ; le menu de Pupitre ne s'ouvre pas. |
| 14 | Orca+H (mode apprentissage), Échap ; pavé 8. | Les commandes d'Orca marchent dans Pupitre : aucune n'est captée. |
| 15 | Dans l'onglet, `ssh-add ~/.ssh/id_test` (clé chiffrée de test) ; Tab ; Échap (vraie touche). | « Phrase de passe SSH boîte de dialogue » ; « Phrase de passe de la clé ~/.ssh/id_test : champ de mot de passe » ; Tab : « Mémoriser dans le trousseau case à cocher cochée » ; Échap ferme la fenêtre, `ssh-add` se termine (la commande suivante s'exécute) et le terminal a le clavier. |
| 16 | Ctrl+Maj+O ; flèche bas ; Tab, Tab, Entrée (« Ajouter… ») ; Tab ×4 ; Échap, Échap. | « Connexions SSH boîte de dialogue », « Connexions Liste à 2 éléments », « serveur-test — alice@127.0.0.1:2222 » ; flèche bas : « essai-un (~/.ssh/config) » ; formulaire : « Nom texte », « Ajouter une connexion » ; après quatre Tab : « Clé liste déroulante Clé » ; Échap ferme le formulaire puis la liste, le terminal a le clavier. |

Couvert par d'autres tests (`scripts/test-x11.sh`) : cloche sans son et pas plus de
3 flashs par seconde, focus toujours visible, noms accessibles de tous les boutons.

## Contrôle manuel (facultatif, environ 2 minutes)

Seulement si vous voulez entendre Pupitre avec votre propre réglage d'Orca (voix,
disposition « portable » avec Verr. Maj) : lancer `./target/release/pupitre` avec Orca
actif, écouter l'annonce du lancement, taper `ls`, lire une ligne avec Orca+I, puis
Ctrl+Maj+T. Si l'un des trois diffère de la vérification automatique, le noter ici.

Notes :

-
