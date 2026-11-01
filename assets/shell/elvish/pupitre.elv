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

# 1. Configuration habituelle de l'utilisateur (chemins de rc.elv comme elvish les cherche).
var dossier-config = ~/.config
if (has-env XDG_CONFIG_HOME) {
  set dossier-config = $E:XDG_CONFIG_HOME
}
var rc-utilisateur = (path:join $dossier-config elvish rc.elv)
if (path:is-regular $rc-utilisateur) {
  eval (slurp < $rc-utilisateur)
}

# 2. Starship, seulement si demandé et si le binaire est toujours là.
if (and (has-env PUPITRE_INVITE) (has-env PUPITRE_STARSHIP) (==s $E:PUPITRE_INVITE starship) (path:is-regular $E:PUPITRE_STARSHIP)) {
  eval ((external $E:PUPITRE_STARSHIP) init elvish --print-full-init | slurp)
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
