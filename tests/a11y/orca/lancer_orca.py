# SPDX-License-Identifier: GPL-3.0-or-later
"""Lance Orca avec la synthèse muette ; les arguments sont transmis à Orca.

Usage : python3 lancer_orca.py -u DOSSIER_PREFS --debug-file JOURNAL

Trois adaptations, sans toucher à l'installation d'Orca :
- la synthèse « pupitre_muet » est ajoutée aux synthèses connues et imposée ;
- le refus de démarrer quand un autre Orca tourne pour le même utilisateur est levé :
  ce test tourne sur son propre écran et son propre bus, il ne gêne pas l'Orca de
  l'utilisateur (et ne le remplace pas) ;
- le journal est écrit ligne par ligne (sinon il arrive par blocs, avec retard).
"""
import faulthandler
import os
import signal
import sys

# Diagnostic : « kill -USR1 <pid> » écrit la pile de tous les fils d'Orca sur sa sortie
# d'erreur (orca.out), utile si Orca se fige.
faulthandler.register(signal.SIGUSR1, all_threads=True)

ICI = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, ICI)

import pupitre_muet  # noqa: E402

sys.modules["orca.pupitre_muet"] = pupitre_muet
from orca import settings  # noqa: E402

settings.speechFactoryModules = ["pupitre_muet"]

LANCEUR = "/usr/bin/orca"
with open(LANCEUR, encoding="utf-8") as f:
    source = f.read()
# (texte d'origine, remplacement) ; chaque texte doit être présent.
RETOUCHES = [
    ("if otherOrcas():", "if False:"),
    # Journal écrit ligne par ligne : le scénario le lit pendant qu'Orca tourne.
    ("open(args.debug_file, 'w')", "open(args.debug_file, 'w', buffering=1)"),
]
for avant, apres in RETOUCHES:
    if avant not in source:
        sys.exit(f"{LANCEUR} : « {avant} » introuvable, version d'Orca non prise en charge")
    source = source.replace(avant, apres)

sys.argv = ["orca", "--speech-system", "pupitre_muet", *sys.argv[1:]]
exec(compile(source, LANCEUR, "exec"), {"__name__": "__main__", "__file__": LANCEUR})
