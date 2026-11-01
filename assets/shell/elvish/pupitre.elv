# SPDX-License-Identifier: GPL-3.0-or-later
# Intégration de Pupitre pour elvish (lancé par `elvish -rc <ce fichier>`, ce qui remplace le
# rc.elv de l'utilisateur : on l'évalue nous-mêmes en premier).
# Niveau partiel : marques A, C, D et OSC 7 seulement. Pas de marque B : elvish affiche en clair
# les caractères de contrôle placés dans l'invite. Pas de code de sortie dans D (elvish ne le
# fournit pas aux crochets). Rien n'est écrit dans HOME.
#
# Limite connue : le rc de l'utilisateur est évalué dans son propre espace de noms (`eval`) ;
# ses réglages d'éditeur, variables d'environnement et invites s'appliquent, mais les fonctions
# et variables qu'il définit ne sont pas visibles depuis la ligne de commande.
use path
use platform

# Une erreur dans le rc de l'utilisateur ou dans Starship ne doit jamais empêcher les marques :
# chaque étape est protégée, l'erreur est affichée sur la sortie d'erreur (message d'elvish).
fn pupitre-signaler {|e|
  echo (to-string $e) >&2
}

# Vrai si le fichier est exécutable (elvish n'a pas de test intégré : on passe par `test -x`).
fn pupitre-executable {|chemin|
  try {
    e:test -x $chemin
    put $true
  } catch _ {
    put $false
  }
}

# 1. Configuration habituelle de l'utilisateur (même recherche qu'elvish : XDG puis ~/.config,
#    puis l'ancien ~/.elvish/rc.elv).
var dossier-config = ~/.config
if (has-env XDG_CONFIG_HOME) {
  set dossier-config = $E:XDG_CONFIG_HOME
}
var candidats = [(path:join $dossier-config elvish rc.elv) ~/.elvish/rc.elv]
# Pour savoir si le rc de l'utilisateur initialise lui-même Starship (variables héritées oubliées).
unset-env STARSHIP_SESSION_KEY
for rc $candidats {
  if (path:is-regular $rc) {
    try {
      eval (slurp < $rc)
    } catch e {
      pupitre-signaler $e
    }
    break
  }
}

# 2. Starship, seulement si demandé, si le binaire est exécutable et s'il n'est pas déjà
#    initialisé par le rc de l'utilisateur.
try {
  if (and (has-env PUPITRE_INVITE) (has-env PUPITRE_STARSHIP) (==s $E:PUPITRE_INVITE starship) (not (has-env STARSHIP_SESSION_KEY)) (path:is-regular $E:PUPITRE_STARSHIP) (pupitre-executable $E:PUPITRE_STARSHIP)) {
    eval ((external $E:PUPITRE_STARSHIP) init elvish --print-full-init | slurp)
  }
} catch e {
  pupitre-signaler $e
}

# 3. Marques, posées après tout le reste.
var deja = $false
set edit:before-readline = [ $@edit:before-readline {
  if $deja {
    print "\e]133;D\a"
  }
  set deja = $true
  print "\e]7;file://"(platform:hostname)$pwd"\a"
  print "\e]133;A\a"
} ]
set edit:after-readline = [ $@edit:after-readline {|ligne|
  print "\e]133;C\a"
} ]
