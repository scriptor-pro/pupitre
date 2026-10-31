# SPDX-License-Identifier: GPL-3.0-or-later
"""Sonde AT-SPI : affiche le texte des widgets de rôle « terminal » d'une application.

Usage : python3 sonde_terminal.py <nom-application>
Code de sortie 0 si au moins un terminal a été trouvé, 1 sinon.
"""
import sys

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi  # noqa: E402

PROFONDEUR_MAX = 15


def terminaux(noeud, prof=0):
    try:
        role = noeud.get_role_name()
    except Exception:
        return
    if role == "terminal":
        yield noeud
    if prof >= PROFONDEUR_MAX:
        return
    for i in range(noeud.get_child_count()):
        enfant = noeud.get_child_at_index(i)
        if enfant is not None:
            yield from terminaux(enfant, prof + 1)


def main():
    nom = sys.argv[1].lower() if len(sys.argv) > 1 else "pupitre"
    bureau = Atspi.get_desktop(0)
    trouve = False
    for i in range(bureau.get_child_count()):
        appli = bureau.get_child_at_index(i)
        if appli is None or nom not in (appli.get_name() or "").lower():
            continue
        for t in terminaux(appli):
            trouve = True
            try:
                texte = Atspi.Text.get_text(t, 0, -1)
            except Exception as e:
                texte = f"<texte illisible : {e}>"
            print(f"terminal : {texte}")
    sys.exit(0 if trouve else 1)


main()
