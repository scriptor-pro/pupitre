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
