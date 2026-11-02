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
config-invite-inconnue = invite « { $v } » inconnue : valeurs possibles « starship » ou « shell »
config-annonce-inconnue = annonce_fin_commande « { $v } » inconnue : valeurs possibles « longues », « toutes » ou « aucune »
config-shell-nom-vide = nom de shell vide : indiquer le nom affiché dans le menu
config-shell-commande-vide = commande vide : le premier élément doit être un programme, ex. ["ssh", "serveur"]
config-shell-commande-option = commande « { $v } » refusée : le premier élément ne doit pas commencer par « - »

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

fond-usage = usage : pupitre-fond [--socket CHEMIN] [--langue CODE]
fond-socket-attend-chemin = pupitre-fond : --socket attend un chemin
fond-langue-attend-code = pupitre-fond : --langue attend un code de langue (ex. fr)
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

## Client du processus de fond (côté interface)

client-lancement = impossible de lancer pupitre-fond : { $erreur }
client-injoignable = pupitre-fond ne répond pas : { $erreur }
client-refus = pupitre-fond refuse la connexion : { $erreur }
client-io = erreur de connexion à pupitre-fond : { $erreur }
client-deconnecte = connexion à pupitre-fond fermée
client-reponse-inattendue = réponse inattendue : { $nom }

## Boutons communs

bouton-fermer = Fermer
bouton-annuler = Annuler
bouton-valider = Valider
message-fermer-aide = Fermer ce message

## Demandes SSH (fenêtre modale)

demande-titre-phrase = Phrase de passe SSH
demande-titre-hote = Serveur inconnu
demande-titre-confirmation = Confirmer l'usage de la clé
demande-titre-question = Question de SSH
demande-titre-information = Information de SSH
demande-phrase-question = Phrase de passe de la clé { $cle } :{ " " }
demande-phrase-question-incorrecte = Phrase incorrecte. Phrase de passe de la clé { $cle } :{ " " }
demande-memoriser = Mémoriser dans le trousseau
demande-trousseau-verrouille = Trousseau verrouillé : la phrase ne peut pas être mémorisée.
demande-trousseau-absent = Aucun trousseau disponible : la phrase sera demandée à chaque fois.
demande-trousseau-lent = Le trousseau ne répond pas pour l'instant : la phrase ne peut pas être mémorisée cette fois.
demande-refuser = Refuser
demande-faire-confiance = Faire confiance et se connecter
demande-autoriser = Autoriser

## Recherche dans l'historique

recherche-placeholder = Rechercher dans l'historique
recherche-expression = Expression régulière
recherche-precedent = Précédent
recherche-precedent-aide = Occurrence précédente (Entrée)
recherche-suivant = Suivant
recherche-suivant-aide = Occurrence suivante (Maj+Entrée)
recherche-fermer = Fermer la recherche
recherche-fermer-aide = Fermer la recherche (Échap)
recherche-expression-invalide = Expression invalide : { $detail }
recherche-aucun-resultat = Aucun résultat

## Surveillance des fichiers de configuration

surveillance-impossible = surveillance des fichiers impossible : { $erreur }
surveillance-dossier-impossible = surveillance de { $dossier } impossible : { $erreur }

## Fenêtre des connexions SSH

connexions-titre = Connexions SSH
connexions-liste = Connexions
connexions-vide = Aucune connexion. Ajoutez-en une, ou décrivez vos serveurs dans ~/.ssh/config.
connexions-vide-invalide = Aucune connexion utilisable : corrigez connexions.toml, ou décrivez vos serveurs dans ~/.ssh/config.
connexions-se-connecter = Se connecter
connexions-ajouter = Ajouter…
connexions-modifier = Modifier…
connexions-supprimer = Supprimer
connexions-hote-ssh-config = { $alias } (~/.ssh/config)
connexions-supprimer-titre = Supprimer ?
connexions-supprimer-question = Supprimer la connexion « { $nom } » ?
connexions-suppression-impossible = Suppression impossible : { $erreur }
connexions-champ-nom = Nom
connexions-champ-hote = Hôte
connexions-champ-utilisateur = Utilisateur (facultatif)
connexions-champ-port = Port (facultatif)
connexions-champ-cle = Clé
connexions-cle-par-defaut = Par défaut
connexions-formulaire-modifier = Modifier la connexion
connexions-formulaire-ajouter = Ajouter une connexion
connexions-enregistrer = Enregistrer
connexions-enregistrement-invalide = Enregistrement impossible : connexions.toml est invalide ({ $erreur }).
connexions-enregistrement-disparue = Enregistrement impossible : cette connexion n'existe plus.
connexions-enregistrement-impossible = Enregistrement impossible : { $erreur }

## Fenêtre principale, onglets, menu

fenetre-nom = Pupitre
fenetre-titre = { $titre } — Pupitre
onglet-titre-defaut = Terminal
onglet-infobulle-cloche = { $titre } (cloche)
onglet-nom-cloche = { $titre }, cloche
onglet-nom = Nom de l'onglet
onglet-nom-aide = Nom de l'onglet (vide : titre automatique)
onglets-nom = Onglets
onglets-position = { $nom }, onglet { $i } sur { $n }
onglets-autre-cloche = { $titre } (onglet { $i })
onglets-cloche-dans = { $texte } ; cloche dans : { $liste }

shell-termine = Le shell s'est terminé (code { $code }). Appuyez sur Entrée pour en ouvrir un nouveau.
shell-termine-signal = Le shell s'est terminé (code signal). Appuyez sur Entrée pour en ouvrir un nouveau.
shell-lancement-impossible = Le shell n'a pas pu être lancé : { $erreur }

fond-injoignable = Pupitre ne peut pas joindre le processus de fond : { $erreur }
fond-arrete = Le processus de fond s'est arrêté : les shells ouverts sont perdus.
fond-nouveau-terminal = Ouvrir un nouveau terminal
fond-relance-impossible = Impossible de relancer le processus de fond : { $erreur }
fond-ancien = Le processus de fond est d'une version plus ancienne : les fonctions SSH de Pupitre seront actives quand tous les onglets auront été fermés.

config-invalide = Configuration invalide, { $erreur }
config-rechargement-indisponible = Rechargement automatique de la configuration indisponible : { $erreur }
theme-illisible = Thème { $chemin } illisible, { $erreur }
theme-illisible-conserve = Thème { $chemin } illisible, { $erreur } : thème précédent conservé
theme-contraste-insuffisant = Thème « { $nom } » : contraste insuffisant — { $liste }
police-introuvable = Police « { $famille } » introuvable : { $secours } utilisée.

fermeture-titre = Fermer ?
fermeture-fenetre-une-commande = Une commande est en cours dans un onglet de cette fenêtre. Fermer quand même ?
fermeture-fenetre-commandes = Des commandes sont en cours dans { $n } onglets de cette fenêtre. Fermer quand même ?
fermeture-onglet-commande = Une commande est en cours dans cet onglet. Fermer quand même ?

menu-nom = Menu
menu-aide = Menu (Ctrl+Tab puis Entrée)
menu-nouvel-onglet = Nouvel onglet
menu-nouvelle-fenetre = Nouvelle fenêtre
menu-connexions = Connexions SSH…
menu-copier = Copier
menu-coller = Coller
menu-rechercher = Rechercher…
menu-agrandir = Agrandir le texte
menu-reduire = Réduire le texte
menu-taille-normale = Taille normale
menu-plein-ecran = Plein écran
menu-renommer-onglet = Renommer l'onglet
menu-detacher-onglet = Détacher dans une nouvelle fenêtre
menu-fermer-onglet = Fermer l'onglet
menu-aide-pupitre = Aide

## Aide intégrée (Ctrl+Maj+H)

aide-titre = Aide de Pupitre
aide-recherche = Rechercher dans l'aide
aide-liste = Sections de l'aide
aide-texte = Texte de la section
aide-resultats =
    { $n ->
        [one] { $n } section trouvée.
       *[other] { $n } sections trouvées.
    }
aide-aucun-resultat = Aucune section trouvée.

connexions-fichier-invalide = Le fichier connexions.toml est invalide ({ $erreur }). Il n'est pas modifié : corrigez-le pour retrouver vos connexions.

## Ligne de commande : pupitre check, aide

usage =
    Usage :
      pupitre                        ouvrir une fenêtre
      pupitre check                  vérifier la configuration et le contraste des thèmes
      pupitre check --theme CHEMIN   vérifier un fichier de thème
      pupitre shells                 lister les shells installés et leur prise en charge
      pupitre aide                   afficher l'aide complète (Ctrl+Maj+H dans la fenêtre)
      pupitre --help                 afficher ce résumé
arguments-incorrects = Arguments incorrects.
argument-inconnu = Argument inconnu « { $argument } ».
check-config-invalide = Configuration invalide ({ $chemin }), { $erreur }
check-pas-de-fichier = Pas de fichier { $chemin } : valeurs par défaut.
check-theme-ok = Thème « { $nom } » : aucun problème de contraste.
check-theme-problemes = Thème « { $nom } » : { $n } problème(s) de contraste :

## Lanceur du bureau (assets/fr.pupitre.Pupitre.desktop : un test vérifie la concordance)

bureau-nom = Pupitre
bureau-nom-generique = Terminal
bureau-commentaire = Terminal à onglets accessible
bureau-mots-cles = terminal;shell;console;ssh;commande;invite;
bureau-nouvelle-fenetre = Nouvelle fenêtre

## Protocole (trames entre l'interface et le processus de fond)

trame-io = erreur d'entrée-sortie : { $erreur }
trame-trop-grande = trame trop grande : { $n } octets (max. { $max })
trame-illisible = trame illisible : { $erreur }
trame-fermee = connexion fermée
trame-octets-en-trop = { $n } octet(s) en trop après le message

## Shells (lancement)

shell-integration-absente = L'intégration de Pupitre pour { $shell } est introuvable : le shell est lancé tel quel.
shell-starship-absent = Starship introuvable : l'invite habituelle du shell est gardée.
shell-starship-absent-installer = Starship introuvable : l'invite habituelle du shell est gardée. Pour l'installer : { $commande }
shell-configure-absent = Le shell « { $shell } » est introuvable : le shell de connexion est utilisé.
shell-configure-absent-installer = Le shell « { $shell } » est introuvable : le shell de connexion est utilisé. Pour l'installer : { $commande }
shell-starship-config-impossible = Impossible de créer l'invite { $chemin } ({ $erreur }) : l'invite habituelle du shell est gardée.

## Menu « Nouvel onglet avec » et commande pupitre shells

menu-nouvel-onglet-avec = Nouvel onglet avec
shells-installes = Shells installés :
shells-aucun = aucun shell trouvé
shells-niveau = ({ $niveau })
shells-niveau-complet = complet
shells-niveau-invite = invite gérée
shells-niveau-tel-quel = tel quel
shells-absents = Shells pris en charge, non installés :
shells-absent = { $nom } ({ $niveau })
shells-absent-installer = { $nom } ({ $niveau }) : { $commande }

## Fin des commandes et navigation de commande en commande

duree-moins-seconde = moins d'une seconde
duree-secondes = { $s } s
duree-minutes = { $m } min { $s } s
duree-minutes-rondes = { $m } min
commande-terminee = Commande terminée.
commande-terminee-duree = Commande terminée en { $duree }.
commande-echec = Commande en échec, code { $code }.
commande-echec-duree = Commande en échec, code { $code }, en { $duree }.
commande-navigation = Commande : { $texte }
commande-navigation-echec = Commande : { $texte }, en échec, code { $code }
commande-vide = Commande vide.
commande-vide-echec = Commande vide, en échec, code { $code }.
commande-aucune-avant = Aucune commande avant.
commande-aucune-apres = Aucune commande après.
commande-navigation-indisponible = Navigation indisponible pendant un programme plein écran.
commande-navigation-impossible = Navigation impossible pour l'instant.
