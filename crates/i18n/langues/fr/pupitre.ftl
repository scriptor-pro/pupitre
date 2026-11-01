# SPDX-License-Identifier: GPL-3.0-or-later
# Pupitre : textes français (langue source, toujours complète).

## Configuration : erreurs du fichier pupitre.toml, des thèmes et des connexions

config-ligne = ligne { $ligne } : { $message }
config-pas-ordinaire = ce n'est pas un fichier ordinaire
config-trop-gros = fichier trop gros (plus de 1 Mio)
config-pas-utf8 = le fichier n'est pas en UTF-8
config-lecture-impossible = impossible de lire { $chemin } : { $erreur }
config-cle-inconnue = clé inconnue « { $cle } » ; clés possibles : { $cles }
config-type-incorrect = type incorrect : { $recu } au lieu de { $attendu }
config-valeur-incorrecte = valeur incorrecte : { $recu } au lieu de { $attendu }
config-cle-double = clé en double (déjà définie plus haut)
config-syntaxe-toml = syntaxe TOML incorrecte ({ $detail })
config-recu-entier = le nombre { $v }
config-recu-decimal = le nombre décimal { $v }
config-recu-texte = le texte { $v }
config-recu-booleen = la valeur { $v }
config-attendu-booleen = true ou false
config-attendu-texte = un texte entre guillemets
config-attendu-entier = un nombre entier
config-theme-inconnu = thème « { $v } » inconnu : valeurs possibles « sombre », « clair », « systeme », ou le chemin d'un fichier de thème commençant par / ou ~/
config-police-vide = police vide : indiquer une famille et une taille, ex. « B612 Mono 11 »
config-historique-limites = historique de { $v } lignes hors limites (de 0 à { $max })
config-zoom-limites = zoom maximum { $v } % hors limites (de { $min } à { $max })
config-langue-inconnue = langue « { $v } » inconnue : valeurs possibles « » (langue du système), « fr », « en », « es », « it », « de », « nl »

couleur-invalide = couleur invalide { $texte } : format attendu #rrggbb (ex. #1e1e2e)
fichier-sans-nom = chemin sans nom de fichier
fichier-pas-ordinaire = { $chemin } n'est pas un fichier ordinaire

theme-palette-16 = la palette doit avoir exactement 16 couleurs (0 à 15), elle en a { $n }
theme-numero-couleur = numéro de couleur « { $v } » invalide : de 0 à 15
theme-role-inconnu = rôle « { $v } » inconnu : « texte », « fond » ou « texte et fond »
theme-base-inconnue = base « { $v } » inconnue : « sombre » ou « clair »

contraste-ligne = { $description } : { $ratio }:1, minimum { $minimum }:1
contraste-description = { $avant } ({ $couleur_avant }) sur { $arriere } ({ $couleur_arriere })
contraste-le-fond = le fond
contraste-couleur = couleur { $n }
contraste-sur-couleur = la couleur { $n }
contraste-texte = texte
contraste-texte-selectionne = texte sélectionné
contraste-la-selection = la sélection
contraste-curseur = curseur

connexion-nom-vide = nom vide
connexion-nom-controle = le nom ne doit pas contenir de caractère de contrôle
connexion-nom-double = nom « { $nom } » en double
connexion-nom-double-plus-haut = nom « { $nom } » en double (déjà défini plus haut)
connexion-hote-vide = hôte vide
connexion-hote-tiret = hôte « { $v } » refusé : ne doit pas commencer par « - »
connexion-hote-blancs = hôte refusé : ni blanc ni caractère de contrôle autorisés
connexion-utilisateur-vide = utilisateur vide
connexion-utilisateur-tiret = utilisateur « { $v } » refusé : ne doit pas commencer par « - »
connexion-utilisateur-blancs = utilisateur refusé : ni blanc ni caractère de contrôle autorisés
connexion-port-limites = port { $v } hors limites (de 1 à 65535)
connexion-port-invalide = port « { $v } » invalide : un nombre de 1 à 65535 est attendu
connexion-cle-vide = chemin de clé vide
connexion-cle-tiret = le chemin de la clé ne doit pas commencer par « - »
connexion-cle-controle = le chemin de la clé ne doit pas contenir de caractère de contrôle

## SSH : agent, trousseau, askpass

agent-deja-present = un agent répond déjà sur ce socket
agent-injoignable = ssh-agent n'est pas devenu joignable

trousseau-verrouille = le trousseau est verrouillé
trousseau-absent = aucun trousseau disponible
trousseau-delai = le trousseau ne répond pas
trousseau-erreur = erreur du trousseau : { $detail }
trousseau-fil-interrompu = fil du trousseau interrompu
trousseau-chemin-non-utf8 = chemin non UTF-8
trousseau-phrase-illisible = phrase illisible (UTF-8 invalide)
trousseau-libelle = Pupitre — phrase de passe de { $chemin }

askpass-ligne = pupitre-askpass : { $texte }
askpass-memorisee-non-lue = phrase mémorisée non lue : { $erreur }
askpass-memorisee-non-oubliee = phrase mémorisée non oubliée : { $erreur }
askpass-socket-absent = PUPITRE_SOCKET absent : pas de processus de fond à interroger
askpass-fond-injoignable = processus de fond injoignable : { $erreur }
askpass-envoi-echec = échec de l'envoi de la demande
askpass-connexion-fermee = connexion fermée avant la réponse
askpass-non-memorisee = phrase non mémorisée : { $erreur }
askpass-sortie-inutilisable = sortie standard inutilisable : { $erreur }

## Processus de fond (pupitre-fond)

fond-usage = usage : pupitre-fond [--socket CHEMIN]
fond-socket-attend-chemin = pupitre-fond : --socket attend un chemin
fond-option-inconnue = pupitre-fond : option inconnue { $option }
fond-erreur = pupitre-fond : { $socket } : { $erreur }

chargement-trousseau-verrouille = Trousseau verrouillé : les clés mémorisées n'ont pas été chargées.
chargement-memorisee-oubliee = La phrase de passe mémorisée pour { $cle } ne fonctionne plus ; elle a été oubliée. Elle sera redemandée.
chargement-non-memorisee = La phrase de passe saisie pour { $cle } n'a pas été mémorisée : elle n'ouvre pas la clé.
chargement-ssh-add-lancement = ssh-add impossible à lancer : { $erreur }
chargement-ssh-add-echec = ssh-add a échoué ({ $etat })
chargement-ssh-add-delai = ssh-add : délai dépassé

ssh-sans-agent = Agent SSH indisponible : les phrases de passe seront demandées à chaque connexion.

serveur-socket-sans-dossier = chemin de socket sans dossier
serveur-dossier-refuse = { $dossier } : { $raison }
serveur-pas-un-dossier = ce n'est pas un dossier
serveur-autre-utilisateur = appartient à un autre utilisateur
serveur-droits-larges = droits trop larges (0700 attendu)
serveur-arret = serveur en cours d'arrêt
serveur-version-incompatible = version du protocole incompatible : interface { $interface }, processus de fond { $fond }
serveur-hello-attendu = protocole : Hello attendu en premier
serveur-hello-deja = protocole : Hello déjà reçu
serveur-ressources = ressources insuffisantes : { $erreur }
serveur-session-inconnue = session { $id } inconnue

session-terminal-impossible = impossible de créer le terminal : { $erreur }
session-shell-impossible = impossible de lancer le shell : { $erreur }
