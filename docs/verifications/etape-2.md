# Vérifications de l'étape 2 (SSH)

Machine : Debian 13 (trixie), GTK 4.18.6, VTE 0.80, Orca 48.1, OpenSSH 10, écrans virtuels
Xvfb (X11, avec xfwm4) et Weston sans écran (Wayland). Ce document est complété en fin
d'étape (Task 12) ; il rassemble pour l'instant ce qui a été vérifié tâche par tâche.

## Tâches 0 à 10 — 2026-10-31

| Tâche | Contenu | Commits | Vérification |
|---|---|---|---|
| 0 | Comportement réel d'OpenSSH 10 et du trousseau (essais) | 2a91fd3 | essais consignés |
| 1 | Recherche et lancement de l'agent SSH (`crates/ssh`) | f220e1d, b382f10 | tests unitaires + vrai `ssh-agent` |
| 2 | Phrases de passe dans le trousseau (Secret Service), démon jetable pour les tests | 7b2d6be, c32b76d | `trousseau_reel` (démon `gnome-keyring-daemon` jetable), `trousseau_absent` |
| 3 | Reconnaissance des invites d'OpenSSH | 0600466, 094264f | 12 tests, dont entrées étranges sans panique |
| 4 | Protocole version 2, compatible version 1 | f9c705c | octets v1 figés (`encodage_v1_inchange`), fuzz 259 857 essais sans plantage |
| 5 | Agent SSH garanti dans les shells du fond | 4184f51, 623f74f, 7712b2c | tests du fond (agent relancé, repris, tué à la fin) |
| 6 | Relais des demandes de phrase de passe | f39845b | 32 tests du fond |
| 7 | `pupitre-askpass` et chargement des clés mémorisées | 358392d, 08a63c3 | tests du fond et d'`askpass` |
| 8 | Connexions mémorisées et hôtes de `~/.ssh/config` | e4b97ff | tests de `pupitre-config` et `pupitre-ssh` |
| 9 | Fenêtre de phrase de passe SSH | d9594fd, 58f93eb | `gui_ssh` sous X11 et Wayland, vrai `ssh-add` dans un onglet |
| 10 | Fenêtre « Connexions SSH » | 23028a8, 06fe748 | `gui_ssh` (16 tests) sous X11 et Wayland |

## Orca et isolement — Task 11, 2026-10-31

### Lecteur d'écran (Orca muet, entièrement automatisé)

`scripts/test-orca.sh` : 16 vérifications (les 14 de l'étape 1 et deux nouvelles), **16/16 trois
fois de suite**. Rien n'est prononcé : aucune écoute n'est demandée.

Les vérifications 15 et 16 tournent avec un trousseau jetable (`scripts/trousseau-test.sh` :
`gnome-keyring-daemon` sur le bus privé du test), une clé SSH de test chiffrée créée dans le
dossier personnel temporaire, sans `SSH_AUTH_SOCK` et avec un `TMPDIR` temporaire : le fond lance
son propre `ssh-agent`, jamais celui de l'utilisateur. Le scénario tue à la fin les `ssh-agent`
du test (reconnus à leur ligne de commande, qui contient le dossier temporaire).

Phrases d'Orca observées (journal `target/orca/orca.log`) :

| # | Manipulation | Orca dit |
|---|---|---|
| 15 | `ssh-add ~/.ssh/id_test` | « Phrase de passe SSH boîte de dialogue » puis « Phrase de passe de la clé ~/.ssh/id_test :  champ de mot de passe. » |
| 15 | Tab | « Mémoriser dans le trousseau case à cocher cochée. » |
| 15 | Échap (vraie touche, via xdotool) | fenêtre fermée ; `ssh-add` terminé : `echo fin-$((6*7))` affiche `fin-42` et l'invite est relue ; aucune seconde fenêtre |
| 16 | Ctrl+Maj+O | « Connexions SSH boîte de dialogue », « Connexions Liste à 2 éléments », « serveur-test — alice@127.0.0.1:2222. » |
| 16 | Flèche bas | « essai-un (~/.ssh/config) » |
| 16 | Tab, Tab | « Se connecter bouton. », « Ajouter… bouton. » |
| 16 | Entrée | « Nom texte. », puis « Ajouter une connexion » (voir défaut 1) |
| 16 | Tab ×4 | « Hôte texte. », « Utilisateur (facultatif) texte. », « Port (facultatif) texte. », « Clé liste déroulante Clé. » |
| 16 | Échap, Échap | « Connexions SSH boîte de dialogue » (retour à la liste), puis le terminal ; `echo ok-$((6*7))` affiche `ok-42` |

Seule vérification où une vraie touche Échap atteint le gestionnaire de la fenêtre de phrase de
passe (les tests graphiques n'émettent que le signal).

**Défauts trouvés par ces vérifications**

1. *Le titre du formulaire « Ajouter une connexion » n'était pas annoncé.* Orca ignore
   l'activation du formulaire (« Ignoring object:state-changed:active due to multiple instances
   in short time » : la liste se désactive à l'instant où le formulaire s'active) et n'a donc que
   le premier champ à dire. Correction (`crates/pupitre/src/connexions.rs`, `ouvrir_formulaire`) :
   Pupitre annonce lui-même le titre 40 ms après l'affichage. Il est dit après « Nom texte », pas
   avant (l'événement de focus est immédiat) : acceptable, vérifié par le test.
2. *Agents `ssh-agent` du test laissés en vie* : le scénario Orca tuait le veilleur de l'agent (il a
   le dossier temporaire dans son environnement) avant que celui-ci ne tue l'agent. Défaut du
   scénario, pas de Pupitre ; corrigé en tuant aussi les processus dont la ligne de commande
   contient le dossier temporaire.

### Isolement (strace)

`scripts/verifier-isolement.sh` : `pupitre-askpass` est ajouté aux programmes jugés. La session type
comprend maintenant `ssh-add ~/.ssh/id_test` (phrase tapée dans la fenêtre, case « Mémoriser »
cochée, donc mémorisée dans le trousseau jetable), `ssh-add -l`, puis `ssh -p 1 127.0.0.1`
(connexion refusée, lancée par l'utilisateur dans le shell : `ssh` n'est pas jugé). La clé est
créée avant strace : le `ssh-keygen -N` de préparation n'est pas dans la trace.

Rapport (trois exécutions identiques, code de sortie 0) :

```
Connexions réseau (hors sockets Unix) : aucune
Écritures hors des dossiers de Pupitre : aucune
Connexions IPv4/IPv6 de programmes non jugés (ssh de l'utilisateur) : 1 (ssh)
Étapes SSH exécutées (pupitre-askpass, ssh-add, ssh dans la trace ; clé chargée dans l'agent) : oui
Phrase de passe de test dans un fichier sous HOME ou XDG_RUNTIME_DIR (111 fichiers lus) : aucune
Phrase de passe de test dans les arguments d'un execve : aucune
Caches de bibliothèques système dans ~/.cache (tolérés) : fontconfig, mesa_shader_cache, mesa_shader_cache_db
```

La recherche de la phrase parcourt en binaire tous les fichiers réguliers de `HOME` et de
`XDG_RUNTIME_DIR` après la session (le fichier du trousseau jetable est ailleurs et chiffré), et
les arguments de tous les `execve` de la trace. Un échec est aussi déclaré si les étapes SSH n'ont
pas eu lieu (programme absent de la trace, clé absente de `ssh-add -l`), pour ne pas « réussir »
sans rien avoir vérifié.

**Correction de l'analyseur de trace** : `strace -f` coupe un appel en deux lignes
(« `<unfinished ...>` » puis « `<... execve resumed>` »). L'analyseur ignorait ces appels : un
`execve` de `ssh-add` ou de `pupitre-askpass` coupé laissait le processus attribué à son nom d'avant
l'exec, donc non jugé. Les lignes sont désormais recollées avant l'analyse (valable aussi pour
`connect`, `openat`, etc.).

À compléter à la Task 12 : tests complets, clippy, Wayland, fuzz, non-régression.
