# Étape 2, Task 0 : comportement réel d'OpenSSH 10 et du trousseau

Verdict : **les trois critères de la Task 0 sont satisfaits** (voir la fin). Aucun point de décision bloquant.
Deux invites n'ont pas pu être observées (pas de sshd installé), voir « Non observé ».
Deux surprises à intégrer au plan : (1) en mode `confirm`, l'askpass ne doit **rien écrire sur stdout** ; (2) `SSH_ASKPASS_PROMPT` est **absent** pour les invites de phrase (il ne vaut `confirm` que pour la confirmation d'usage).

Environnement : OpenSSH 10.0p2 (Debian 13), git 2.47.3, secret-service 4 (`blocking`, `rt-async-io-crypto-rust`), gnome-keyring. HOME, bus D-Bus, clé et agent jetables. Sorties brutes : `essai.out`, `git.out`, `trousseau.out`. Scripts : `essai.sh`, `essai-git.sh`, `essai-trousseau.sh`, `espion.sh`, `serveur401.py`, `trousseau/`.

Conditions : `SSH_ASKPASS=espion.sh`, `SSH_ASKPASS_REQUIRE=force`, ni `DISPLAY` ni `WAYLAND_DISPLAY`, `setsid -w … </dev/null` (pas de terminal). Dans tous les cas l'espion a été appelé : `force` utilise l'askpass sans écran.
Convention : le chemin de la clé est remplacé par `$T/cle` dans ce document. Les chaînes reçues par l'askpass (`argv[1]`) se terminent par `: ` (deux-points et espace final), sans saut de ligne ; dans les sorties brutes, `$` après `cat -A` est le saut de ligne ajouté par l'espion.

## 1. `ssh-add $T/cle` (clé ed25519, phrase `secret1`)

1re invite (`argv[1]`), `SSH_ASKPASS_PROMPT` absent :

```
Enter passphrase for $T/cle: 
```
(brut : `Enter passphrase for /tmp/tmp.XXXX/cle: $` avec `cat -A`, `$` = fin de ligne ajoutée par l'espion)

Mauvaise réponse puis bonne : 2e invite, `SSH_ASKPASS_PROMPT` absent, code final 0 :

```
Bad passphrase, try again for $T/cle: 
```

Réponse vide : une seule invite, `ssh-add` s'arrête, **code 1**, rien n'est ajouté (pas de nouvelle invite).

Succès : code 0, sortie `Identity added: $T/cle (commentaire)`.

## 2. `ssh-add -c $T/cle` puis usage

Invite de phrase (ajoute un suffixe), `SSH_ASKPASS_PROMPT` absent :

```
Enter passphrase for $T/cle (will confirm each use): 
```

Lors de l'usage de la clé (`ssh-keygen -Y sign -U` via l'agent), c'est **ssh-agent** qui lance l'askpass, avec `SSH_ASKPASS_PROMPT=confirm`. `argv[1]` est sur **deux lignes** :

```
Allow use of key Baudouin@B612?\n
Key fingerprint SHA256:+puNOuRbTCiaMni0vafFvVMW8QJTOq9z3zcqWNA9dH8.
```
(`<commentaire de la clé>` à la place de `Baudouin@B612` ; la 2e ligne finit par un point ; le descripteur 3 de l'askpass est une socket, l'agent est le parent.)

Points importants pour `pupitre-askpass` :
- l'agent ne lit l'environnement (`SSH_ASKPASS`, `SSH_ASKPASS_REQUIRE`) qu'**à son démarrage** : un agent lancé sans `SSH_ASKPASS` refuse l'usage de la clé (`agent refused operation`) ;
- **code 0 = autoriser, code ≠ 0 = refuser** (vérifié : code 1 donne `agent refused operation`) ;
- **toute sortie sur stdout fait refuser** (vérifié : un askpass qui affiche `reponse` puis sort 0 est refusé). En mode `confirm` il ne faut donc rien écrire.

## 3. `ssh -i $T/cle …`

`ssh -i $T/cle -o IdentitiesOnly=yes -o UserKnownHostsFile=$T/kh nimporte@127.0.0.1 -p 1` : `ssh: connect to host 127.0.0.1 port 1: Connection refused`, code 255, askpass **non appelé**. Idem `-p 22 localhost` : aucun sshd ne répond (non installé).

### Non observé (d'après les sources OpenSSH, à confirmer sur un vrai serveur)
Il n'y a ni sshd, ni dropbear, ni bibliothèque serveur SSH disponibles ; je n'ai pas installé de paquet.
- Phrase de clé pour `ssh` : `Enter passphrase for key '<chemin>': ` (le chemin est entre apostrophes simples, différent de `ssh-add` qui n'a pas le mot `key` ni d'apostrophes). `SSH_ASKPASS_PROMPT` : absent (d'après les sources).
- Hôte inconnu : `The authenticity of host '<hôte> (<ip>)' can't be established.\nED25519 key fingerprint is SHA256:….\nThis key is not known by any other names.\nAre you sure you want to continue connecting (yes/no/[fingerprint])? ` avec `SSH_ASKPASS_PROMPT=confirm` (d'après les sources), réponse attendue `yes`/`no`/empreinte. À vérifier contre un vrai serveur avant de figer les tests de la Task 3.
- Mot de passe : `<user>@<hôte>'s password: `.

## 4. git et `SSH_ASKPASS`

Faux serveur HTTP local répondant 401 (`serveur401.py`), sans `GIT_ASKPASS`, `core.askPass` ni `GIT_TERMINAL_PROMPT`, sans terminal. **git appelle bien `SSH_ASKPASS`**, avec ou sans `SSH_ASKPASS_REQUIRE=force`, `SSH_ASKPASS_PROMPT` absent :

```
Username for 'http://127.0.0.1:18401': 
Password for 'http://x@127.0.0.1:18401': 
```
(deuxième invite après réponse `x` à la première ; en HTTPS : `Username for 'https://hôte': `). Échec final : code 128. Les messages de git sont localisés (français ici) mais pas les invites askpass.

## 5. Descripteur non `CLOEXEC`

Un fd 7 ouvert sans `CLOEXEC` dans l'appelant **arrive jusqu'à l'askpass** lancé par `ssh-add` (`/proc/$$/fd/7` pointe vers le fichier). Information seulement : l'appelant doit fermer ses descripteurs hérités ; ne pas compter sur la fermeture par ssh-add. (Dans l'askpass, le fd 3 était un tube vers ssh-add, ou une socket vers ssh-agent.)

## 6. Trousseau (secret-service 4, `blocking`, `EncryptionType::Dh`)

Sous `dbus-run-session`, HOME et XDG_* jetables, `gnome-keyring-daemon --unlock --components=secrets` (mot de passe `motdepasse` sur stdin) :
- `connect` : ~0,3 à 0,5 s (premier démarrage du daemon) ;
- `get_default_collection()` OK ; `is_locked()` = `Ok(false)`, en ~6 ms ;
- écriture (`create_item`, remplacement activé) OK ; `search_items` par attributs `{application, cle}` : 1 résultat, secret relu `secret1` ; recherche avec une autre valeur : 0 ; `delete()` OK, 0 restant.

Collection verrouillée (même HOME, nouveau bus, `gnome-keyring-daemon --start` sans déverrouiller) : `is_locked()` = `Ok(true)` en ~3 ms, **sans aucune invite** (aucun prompt, le programme se termine seul). Seules les opérations qui demandent le déverrouillage (écriture, lecture de secret) peuvent déclencher une invite ; elles n'ont pas été lancées sur la collection verrouillée.

Cas d'erreur (tous rendus rapidement, aucune attente infinie) :
- bus de session sans aucun service activable : `connect` renvoie en **~150 ms** `zbus error: org.freedesktop.DBus.Error.ServiceUnknown: The name org.freedesktop.secrets was not provided by any .service files` ;
- pas de bus de session (`DBUS_SESSION_BUS_ADDRESS` absent) : **~5 ms**, `no secret service provider or dbus session found` (`Error::Unavailable`) ;
- bus normal mais HOME neuf sans trousseau : le daemon est activé par D-Bus ; `connect` OK mais `get_default_collection()` renvoie `Err(NoResult)` (aucune collection par défaut) : à traiter comme « trousseau indisponible », sans invite.

Attention : sur un bus normal, `connect` **active automatiquement** `gnome-keyring` (service D-Bus) ; `connect` n'est donc pas « sans effet de bord ».

## Verdict des critères (Step 4)

1. `force` utilise l'askpass sans écran : **OK** (ssh-add, ssh-agent, git).
2. Les invites de phrase contiennent le chemin de la clé : **OK** pour `ssh-add` (`Enter passphrase for <chemin>: `, `Bad passphrase, try again for <chemin>: `, `… (will confirm each use): `). Pour `ssh` : `Enter passphrase for key '<chemin>': ` **non observé**, d'après les sources.
3. `is_locked()` ne déclenche aucune invite : **OK** (vérifié collection déverrouillée et verrouillée).
