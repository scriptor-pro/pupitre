# SPDX-License-Identifier: GPL-3.0-or-later
"""Écoute les annonces faites au lecteur d'écran (événement AT-SPI « object:announcement »,
émis par gtk_accessible_announce) par une application, pendant une durée donnée.

Usage : python3 ecoute.py --app NOM --duree SECONDES
Écrit « pret » dès que l'écoute est active, puis une ligne « annonce<TAB>texte » par
annonce reçue. Code 0 dans tous les cas.
"""
import argparse
import sys

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi, GLib  # noqa: E402


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--app", required=True)
    p.add_argument("--duree", type=float, default=5.0)
    a = p.parse_args()
    nom = a.app.lower()

    def recu(e):
        try:
            appli = e.source.get_application()
            if appli is None or (appli.get_name() or "").lower() != nom:
                return
        except Exception:
            return
        texte = e.any_data if isinstance(e.any_data, str) else str(e.any_data)
        print(f"annonce\t{texte}", flush=True)

    ecouteur = Atspi.EventListener.new(recu)
    ecouteur.register("object:announcement")
    print("pret", flush=True)
    GLib.timeout_add(int(a.duree * 1000), lambda: (Atspi.event_quit(), False)[1])
    Atspi.event_main()
    return 0


sys.exit(main())
