# SPDX-License-Identifier: GPL-3.0-or-later
"""Pilotage de GNOME Shell (Wayland, sans écran) pour les essais en VM.

  gnome.py taper TEXTE          tape le texte puis Entrée
  gnome.py ecrire TEXTE         tape le texte, sans Entrée
  gnome.py touches ctrl+shift+t ...  combinaisons (noms : ctrl, shift, alt, return,
                                escape, prior, next, tab, ou un caractère)
  gnome.py fenetres             une ligne par fenêtre : id d'application — titre — focus=…
  gnome.py capture FICHIER      capture d'écran PNG
  gnome.py eval CODE            évalue du JavaScript dans GNOME Shell

Les touches passent par l'interface de bureau à distance de Mutter
(org.gnome.Mutter.RemoteDesktop) : ce sont des keysyms, traduits par GNOME avec son
propre plan de clavier. L'état des fenêtres et la capture passent par GNOME Shell, mis en
mode « unsafe » par une extension (réservé à la VM d'essai).
"""
import json
import sys
import time

from gi.repository import Gio, GLib

BUS = Gio.bus_get_sync(Gio.BusType.SESSION, None)
SPECIALES = {
    "ctrl": 0xFFE3, "shift": 0xFFE1, "alt": 0xFFE9, "return": 0xFF0D, "escape": 0xFF1B,
    "prior": 0xFF55, "next": 0xFF56, "tab": 0xFF09, "space": 0x20,
}


def appel(nom, chemin, interface, methode, params=None, type_retour=None):
    return BUS.call_sync(nom, chemin, interface, methode, params,
                         GLib.VariantType(type_retour) if type_retour else None,
                         Gio.DBusCallFlags.NONE, 10000, None)


class Clavier:
    def __enter__(self):
        (self.session,) = appel("org.gnome.Mutter.RemoteDesktop", "/org/gnome/Mutter/RemoteDesktop",
                                "org.gnome.Mutter.RemoteDesktop", "CreateSession", None, "(o)").unpack()
        self._session("Start")
        time.sleep(0.3)
        # Le clavier virtuel de la session est neuf : sa première touche est perdue (comme
        # avec wtype). Une touche neutre la remplace.
        self.touche(SPECIALES["shift"])
        time.sleep(0.2)
        return self

    def _session(self, methode, params=None):
        appel("org.gnome.Mutter.RemoteDesktop", self.session,
              "org.gnome.Mutter.RemoteDesktop.Session", methode, params)

    def keysym(self, k, appui):
        self._session("NotifyKeyboardKeysym", GLib.Variant("(ub)", (k, appui)))

    def touche(self, k):
        self.keysym(k, True)
        self.keysym(k, False)
        time.sleep(0.02)

    def texte(self, t):
        for c in t:
            self.touche(SPECIALES["space"] if c == " " else ord(c))

    def combinaison(self, combo):
        noms = combo.lower().split("+") if len(combo) > 1 else [combo]
        ks = [SPECIALES.get(n, ord(n[0]) if len(n) == 1 else None) for n in noms]
        if None in ks:
            raise SystemExit(f"touche inconnue : {combo}")
        for k in ks:
            self.keysym(k, True)
        for k in reversed(ks):
            self.keysym(k, False)
        time.sleep(0.05)

    def __exit__(self, *_):
        time.sleep(0.2)
        self._session("Stop")


def evaluer(js):
    ok, res = appel("org.gnome.Shell", "/org/gnome/Shell", "org.gnome.Shell", "Eval",
                    GLib.Variant("(s)", (js,)), "(bs)").unpack()
    if not ok:
        raise SystemExit(f"Eval refusé ou en erreur : {res}")
    return res


def fenetres():
    js = """JSON.stringify(global.get_window_actors().map(a => a.meta_window).map(w => [
        w.get_gtk_application_id() || w.get_wm_class() || '?', w.get_title() || '',
        w === global.display.focus_window]))"""
    for app, titre, focus in json.loads(json.loads(evaluer(js))):
        print(f"{app} — {titre} — focus={'true' if focus else 'false'}")


def capture(fichier):
    appel("org.gnome.Shell.Screenshot", "/org/gnome/Shell/Screenshot", "org.gnome.Shell.Screenshot",
          "Screenshot", GLib.Variant("(bbs)", (False, False, fichier)), "(bs)")


def main():
    cmd, args = sys.argv[1], sys.argv[2:]
    if cmd in ("taper", "ecrire"):
        with Clavier() as c:
            c.texte(args[0])
            if cmd == "taper":
                c.touche(SPECIALES["return"])
    elif cmd == "touches":
        with Clavier() as c:
            for combo in args:
                c.combinaison(combo)
    elif cmd == "fenetres":
        fenetres()
    elif cmd == "capture":
        capture(args[0])
    elif cmd == "eval":
        print(evaluer(args[0]))
    else:
        raise SystemExit(__doc__)


main()
