#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
# Essai jetable (Task 0, étape 4) : une gio::Notification d'id fr.pupitre.Pupitre
# s'affiche-t-elle, et le clic active-t-il app.montrer-onglet(42) ?
# Usage : bulle.py [secondes_d_attente]   (envoie UNE bulle, attend, journalise)
import sys, gi
gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib

attente = int(sys.argv[1]) if len(sys.argv) > 1 else 8
app = Gio.Application(application_id="fr.pupitre.Pupitre", flags=Gio.ApplicationFlags.NON_UNIQUE)

def montrer(_a, param):
    print("ACTION app.montrer-onglet reçue, paramètre =", param.print_(False) if param else None, flush=True)

act = Gio.SimpleAction.new("montrer-onglet", GLib.VariantType("u"))
act.connect("activate", montrer)
app.add_action(act)

def activate(a):
    a.hold()
    n = Gio.Notification.new("Essai Pupitre")
    n.set_body("Essai Pupitre : notification")
    n.set_priority(Gio.NotificationPriority.NORMAL)
    n.set_default_action_and_target("app.montrer-onglet", GLib.Variant("u", 42))
    n.add_button_with_target("Montrer", "app.montrer-onglet", GLib.Variant("u", 42))
    a.send_notification("essai-1", n)
    print("bulle envoyée", flush=True)
    def fin():
        a.withdraw_notification("essai-1")
        a.release()
        return False
    GLib.timeout_add_seconds(attente, fin)

app.connect("activate", activate)
app.run([])
