# Vérifications de l'étape 3 (shells et invite)

Machine : Debian 13 (trixie), GTK 4.18.6, VTE 0.80, Orca 48, Starship 1.22.1, écrans virtuels
Xvfb (X11) et Weston sans écran (Wayland) ; poste du porteur : Xfce sous X11. Plan :
`docs/plans/2026-11-01-pupitre-etape-3.md`. Registre d'exécution (fait foi pour le détail des
relectures) : `.superpowers/sdd/2026-11-01-pupitre-etape-3/progress.md` ; ses identifiants de
commit sont antérieurs à la réécriture de l'historique du 2026-11-12 (publication du dépôt).

## Tâches 0 à 8

| Tâche | Contenu | Relecture |
|---|---|---|
| 0 | Essai jetable : shells, Starship et OSC 133 réels | conclusions revérifiées en Task 2 |
| 0bis | Système de traduction (français seulement) | propre |
| 1 | Crate `pupitre-shell` : détection, niveaux, installation, lancement | propre |
| 2 | Fichiers d'intégration (configuration de l'utilisateur, puis marques et dossier courant) | propre |
| 3 | Starship du système, invite par défaut | propre |
| 4 | Protocole v3 : ouverture avec variables, marques, trousseau lent | propre |
| 5 | Le fond lit les marques OSC 133 | propre après 3 tours |
| 6 | Choix du shell, menu « Nouvel onglet avec », `pupitre shells` | propre |
| 7 | Annonce de fin de commande, navigation de commande en commande | propre après 5 tours |
| 7bis | Aide dans Pupitre, hors ligne | propre |
| 7ter | Menu « Préférences » | propre |
| 8 | Orca, isolement, VM | défaut d'onglet Orca trouvé, corrigé le 2026-11-11 |

Hors plan, à la demande du porteur : copie automatique de la sélection (2026-11-11).

## Task 9 — vérification de fin d'étape, 2026-11-11 et 2026-11-12

### Steps 1 à 4 (journaux : `target/etape3/`)

| Vérification | Résultat |
|---|---|
| `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check` | 0 avertissement, formatage conforme (refait après chaque correction ci-dessous) |
| `scripts/test-x11.sh`, `scripts/test-wayland.sh` | verts ; voir « Tests instables sous charge » |
| Fuzzing `trame`, 5 min | 8 736 essais en 301 s, aucun plantage |
| Fuzzing `marques`, 5 min | 1 601 901 essais en 301 s, aucun plantage |
| `scripts/verifier-isolement.sh` | conforme : aucune connexion réseau, aucune écriture hors des dossiers de Pupitre, arrêt propre, phrase de passe nulle part (Starship dans la trace ; caches tolérés : fontconfig, mesa, starship) |
| `scripts/test-orca.sh` | 21/21 |
| VM Sway, GNOME, KDE (`scripts/vm.sh … essayer`) | 15/15 chacune |

### Step 5 — essai sur le poste du porteur

| # | Point | Résultat |
|---|---|---|
| 1 | L'invite ressemble à l'actuelle | ok |
| 2 | `sleep 6` (annonce d'Orca) | ok |
| 3 | Menu ▸ « Nouvel onglet avec » ▸ fish | **défaut**, corrigé (1), puis ok |
| 3 bis | Ctrl+Maj+T depuis un onglet fish | ouvrait bash ; changé (2), puis ok |
| 4 | `pupitre shells` | ok |
| 5 | Ctrl+Maj+H ouvre l'aide ; Échap ferme le menu | ok (Ctrl+Maj+H était pris par un raccourci Xfce du porteur, déplacé sur Super+N) |
| 6 | Préférences ▸ « Modifier la configuration… » | fenêtre de choix d'application ; changé (3) et (4), puis ok |
| — | `exit` (remarque du porteur) | ne fermait pas l'onglet ; changé (5), puis ok |

**Défauts et changements issus de l'essai**

1. *Configuration relue sans fin* (défaut). `notify` signale aussi l'ouverture et la fermeture
   sans écriture du fichier surveillé ; chaque relecture de `pupitre.toml` en provoquait une
   autre (≈ 600 ms, ≈ 22 % de processeur en continu sur le poste du porteur), dès le premier
   changement ou la première ouverture par un éditeur. Le sous-menu « Nouvel onglet avec » était
   refait à chaque fois : surbrillance revenue sur « sh », clic perdu (vidéo du porteur).
   Établi sous Xvfb (strace : 5 ouvertures en 3 s sans aucune écriture). Correction
   (`rechargement.rs`) : simples lectures ignorées ; sous-menu refait seulement s'il change
   (`fenetre.rs`). Test `une_lecture_ne_relance_pas` (7 rappels avant la correction, 1 après).
2. *Nouvel onglet* (décision du porteur) : Ctrl+Maj+T reprend le shell de l'onglet actif
   (installé ou déclaré) ; depuis une connexion SSH, une session reprise ou une nouvelle fenêtre :
   shell des réglages. Tests `nouvel_onglet_reprend_le_shell_actif` (échouait avant : `[bash]` au
   lieu de `[sh]`) et `nouvel_onglet_apres_connexion_shell_par_defaut`.
3. *Éditeur graphique* (décision du porteur, option B) : aucune application n'était associée à
   `application/toml` ; Pupitre prend alors l'éditeur de `text/plain`. Test
   `sans_appli_pour_le_type_editeur_de_texte` (échoue sans le repli).
4. *Éditeur du terminal* (demande du porteur : nano) : avec `$VISUAL`, sinon `$EDITOR`, installé,
   le fichier s'ouvre dans un nouvel onglet nommé d'après lui ; sinon l'éditeur graphique (3).
   Tests `editeur_du_terminal_visual_puis_editor`, `editeur_du_terminal_dans_un_onglet`.
5. *Fin du shell* (décision du porteur, option A) : `exit` (code 0) ferme l'onglet, le dernier
   ferme la fenêtre ; sur erreur ou signal, l'onglet reste avec « Le shell s'est terminé
   (code N). Appuyez sur Entrée… ». Test `exit_ferme_l_onglet` ; `onglet_shell_termine_affiche_message`
   et la vérification Orca 8 passent à `exit 3`.

Noté pour plus tard (conception §2) : bouton « + » de nouvel onglet dans la barre de menu.

### Après les corrections

- Suite X11 complète (`--no-fail-fast`) : 465 tests réussis. Orca : 15/21 sous forte charge, puis
  chaque vérification en échec (1, 2, 6, 7, 8, 15) réussie à la relance.
- Suite Wayland complète (`--no-fail-fast`) : 463 tests réussis, 2 échecs.
  `cle_memorisee_chargee_au_demarrage` réussit à la relance ; `cout_borne_du_redimensionnement`
  échoue encore à la relance (497 ms mesurées pour 200 ms au plus, charge moyenne 13,9) : il
  a réussi à la relance sous X11 le même jour et ne touche pas au code modifié.

### Tests instables sous charge

La machine était très chargée pendant ces essais (charge moyenne 6 à 18, Firefox). Échecs
ponctuels, tous réussis à la relance, sans lien avec les corrections :

- `gui_demarrage` : `demande_recue_deux_fois_une_seule_fenetre` et
  `message_garde_montre_dans_la_premiere_fenetre`. Le premier échoue **aussi sur l'ancien code**
  dans les mêmes conditions (vérifié en remettant les fichiers d'avant la correction (1)) : le
  trousseau répond trop lentement (« délai d'attente dépassé »).
- `gui_commandes` : `cout_borne_du_redimensionnement` (minutage).
- `pupitre-fond` : `cle_memorisee_chargee_au_demarrage` (trousseau ; le plan prévoit une relance).
- Orca : vérifications 1, 2, 6, 7, 15.

À relancer sur une machine calme avant la publication du paquet.

## Relecture finale — 2026-11-12

Relecture indépendante de toute l'étape (24e664b..74ece1d, paquet `.deb` compris) : « Approuvée
avec corrections », aucun défaut critique. Aucun panic atteignable, aucune écriture dans les
fichiers de l'utilisateur, aucun accès réseau ; lecture des marques, protocole v3, fichiers
d'intégration, annonces et changements du 2026-11-12 jugés corrects.

Passe de corrections (74ece1d..0e3acda), chacune avec un test qui échouait avant :

- I1 : une demande d'ouverture faite pendant une panne du processus de fond restait en
  attente ; après la relance, les onglets recevaient le titre ou le shell d'une demande
  périmée, et Préférences annonçait une fausse confirmation. L'attente n'est mise en file que
  si la demande part ; sinon la panne est réannoncée. File vidée à chaque connexion.
- I2 : `exit` après une commande en échec (code de cette commande) gardait l'onglet ouvert.
  Le critère proposé (« arrêté à l'invite ») ne marche pas : bash émet la marque C par `PS0`
  avant d'exécuter `exit`. Critère retenu : pour un shell, tout code ferme l'onglet une fois
  l'invite affichée (marque A, rejeu compris) ; arrêt avant la première invite ou par un
  signal : l'onglet reste. `exit 3` tapé à la main ferme donc aussi l'onglet ; la
  vérification Orca 8 passe à `kill -9 $$`.
- I3 : un onglet d'arrière-plan qui se fermait n'était pas annoncé. Annonce « Onglet « titre »
  fermé : son shell s'est terminé. ».
- M1 : fish perdait `$pipestatus` pour l'invite de l'utilisateur ; rétabli (jusqu'à 8 étages).
- M2 : Starship écrivait dans `~/.cache/starship` ; `STARSHIP_CACHE` dans
  `$XDG_STATE_HOME/pupitre/starship`, plus toléré par la vérification d'isolement.
- M3 : clic sur un shell déclaré retiré de la configuration : annoncé.
- M4 : connexion ou éditeur terminé : « Le programme s'est terminé… ».
- M6 : `$VISUAL` découpé comme le shell (guillemets).

Relecture de ces corrections : « Approuvée avec corrections ». Suites (0e3acda) :

- N1 (important) : le critère de I2 fermait aussi un onglet de connexion SSH vers un hôte
  doté de l'intégration quand la connexion tombait (ssh rend 255) : message et historique
  perdus. Le critère ne vaut plus que pour les shells ; une connexion ou un éditeur ne ferme
  son onglet que sur le code 0. Test `connexion_coupee_garde_l_onglet`.
- N2 : l'annonce de fermeture va d'abord dans la fenêtre active.

Vérifications après corrections : clippy et `cargo fmt` propres ; X11 471 tests réussis (2
échecs ponctuels réussis à la relance sur machine calme, avec le test de N1) ; Wayland 470/470 ;
Orca 21/21 ; isolement conforme ; intégration réelle des shells 26/26.

**Restes connus**

- M5 : un binaire de développement prend les fichiers d'intégration de `/usr/share/pupitre/shell`
  si le paquet est installé.
- M7 (réglé à l'étape 4, commit d7bcb94) : recherche des shells sur le fil GTK à chaque relecture de la configuration (un dossier
  du `PATH` sur un montage réseau figé gèlerait l'interface ; plausible, non confirmé).
- N3 : deux onglets de même titre fermés à moins de 5 s : une seule annonce.
- N4 (réglé à l'étape 4, commit d7bcb94 ; antérieur à l'étape 3) : deux Entrée rapides sur un onglet terminé peuvent laisser un shell
  orphelin dans le processus de fond.
- Un shell en `set -e` qui meurt sur une commande en échec après son invite ferme son onglet
  (comme `exit`).
- fish : au-delà de 8 étages de pipeline, ou après `not`, seul `$status` est rétabli.
