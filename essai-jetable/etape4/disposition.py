#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
# Essai jetable (Task 0, étape 4) : disposition du clavier lue par GDK 4.
# Lecture seule : ne change jamais la disposition. Usage : disposition.py [secondes_d_ecoute]
# Pendant l'écoute, le porteur peut basculer la disposition (ici : Ctrl droit + Maj droite)
# pour voir arriver les signaux notify::active-layout-index.
import sys, gi
gi.require_version("Gdk", "4.0")
gi.require_version("Gtk", "4.0")
from gi.repository import Gdk, Gtk, GLib

ecoute = int(sys.argv[1]) if len(sys.argv) > 1 else 0
Gtk.init()
disp = Gdk.Display.get_default()
print("affichage :", type(disp).__name__, disp.get_name())
seat = disp.get_default_seat()
kb = seat.get_keyboard()
print("clavier :", kb.get_name() if kb else None)
if kb is None:
    sys.exit(0)

def etat(motif):
    print(f"[{motif}] layout_names={list(kb.get_layout_names())!r} "
          f"active_layout_index={kb.get_active_layout_index()} "
          f"has_bidi_layouts={kb.props.has_bidi_layouts} "
          f"caps_lock={kb.props.caps_lock_state}", flush=True)

etat("initial")
for nom in ("layout-names", "active-layout-index"):
    kb.connect("notify::" + nom, lambda *_a, n=nom: etat("signal notify::" + n))
if ecoute:
    boucle = GLib.MainLoop()
    GLib.timeout_add_seconds(ecoute, boucle.quit)
    boucle.run()
    etat("final")
