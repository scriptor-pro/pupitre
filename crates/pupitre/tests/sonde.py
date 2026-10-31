# SPDX-License-Identifier: GPL-3.0-or-later
"""Sonde AT-SPI : affiche l'arbre d'accessibilité d'une application, une ligne par élément :
« rôle<TAB>nom<TAB>texte » (texte sur une ligne, \\n échappés).

Usage : python3 sonde.py <nom-application>   (nom exact, casse ignorée)
Code de sortie 0 si l'application a été trouvée, 1 sinon.
"""
import sys

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi  # noqa: E402

PROFONDEUR_MAX = 25


def parcourir(noeud, prof=0):
    try:
        role = noeud.get_role_name()
        nom = noeud.get_name() or ""
    except Exception:
        return
    texte = ""
    try:
        if noeud.get_text_iface() is not None:
            texte = Atspi.Text.get_text(noeud, 0, -1) or ""
    except Exception:
        pass
    texte = texte.replace("\\", "\\\\").replace("\n", "\\n")
    print(f"{role}\t{nom}\t{texte}")
    if prof >= PROFONDEUR_MAX:
        return
    for i in range(noeud.get_child_count()):
        enfant = noeud.get_child_at_index(i)
        if enfant is not None:
            parcourir(enfant, prof + 1)


def main():
    nom = sys.argv[1].lower() if len(sys.argv) > 1 else "pupitre"
    bureau = Atspi.get_desktop(0)
    trouve = False
    for i in range(bureau.get_child_count()):
        appli = bureau.get_child_at_index(i)
        if appli is None or (appli.get_name() or "").lower() != nom:
            continue
        trouve = True
        parcourir(appli)
    sys.exit(0 if trouve else 1)


main()
