# SPDX-License-Identifier: GPL-3.0-or-later
"""Pilotage de KWin (Plasma 6, Wayland) pour les essais en VM.

  sudo kde.py taper TEXTE          tape le texte puis Entrée
  sudo kde.py ecrire TEXTE         tape le texte, sans Entrée
  sudo kde.py touches ctrl+shift+t ...  combinaisons (ctrl, shift, alt, return, escape,
                                   prior, next, tab, ou un caractère)
  kde.py fenetres JOURNAL          une ligne par fenêtre : id — titre — focus=… (JOURNAL :
                                   fichier où KWin écrit sa sortie)

Le clavier est un vrai périphérique du noyau (uinput, d'où sudo) : KWin le lit par
libinput comme un clavier branché. Les caractères sont traduits en touches du plan
QWERTY américain, celui que KWin utilise dans la VM. L'état des fenêtres vient d'un petit
script KWin (D-Bus org.kde.KWin /Scripting) qui écrit dans la sortie de KWin.
"""
import os
import subprocess
import sys
import tempfile
import time

TOUCHES_NOMMEES = {
    "ctrl": "KEY_LEFTCTRL", "shift": "KEY_LEFTSHIFT", "alt": "KEY_LEFTALT",
    "return": "KEY_ENTER", "escape": "KEY_ESC", "prior": "KEY_PAGEUP", "next": "KEY_PAGEDOWN",
    "tab": "KEY_TAB", "space": "KEY_SPACE",
}
SANS_MAJ = {" ": "SPACE", "-": "MINUS", "=": "EQUAL", "[": "LEFTBRACE", "]": "RIGHTBRACE",
            ";": "SEMICOLON", "'": "APOSTROPHE", "`": "GRAVE", "\\": "BACKSLASH", ",": "COMMA",
            ".": "DOT", "/": "SLASH"}
AVEC_MAJ = {"_": "MINUS", "+": "EQUAL", "{": "LEFTBRACE", "}": "RIGHTBRACE", ":": "SEMICOLON",
            '"': "APOSTROPHE", "~": "GRAVE", "|": "BACKSLASH", "<": "COMMA", ">": "DOT",
            "?": "SLASH", "!": "1", "@": "2", "#": "3", "$": "4", "%": "5", "^": "6",
            "&": "7", "*": "8", "(": "9", ")": "0"}


def touche_de(c):
    """(nom evdev, avec Maj) pour un caractère du plan QWERTY américain."""
    if c.isascii() and c.isalpha():
        return f"KEY_{c.upper()}", c.isupper()
    if c.isdigit():
        return f"KEY_{c}", False
    if c in SANS_MAJ:
        return f"KEY_{SANS_MAJ[c]}", False
    if c in AVEC_MAJ:
        return f"KEY_{AVEC_MAJ[c]}", True
    raise SystemExit(f"caractère non pris en charge : {c!r}")


class Clavier:
    def __enter__(self):
        from evdev import UInput, ecodes
        self.e = ecodes
        self.ui = UInput({ecodes.EV_KEY: list(ecodes.keys.keys())}, name="clavier-essai-pupitre")
        time.sleep(1.0)  # le temps que KWin découvre le périphérique
        self.touche("KEY_LEFTSHIFT")  # touche neutre : la première peut être perdue
        return self

    def _ev(self, nom, valeur):
        self.ui.write(self.e.EV_KEY, getattr(self.e, nom), valeur)
        self.ui.syn()
        time.sleep(0.015)

    def touche(self, nom, maj=False):
        if maj:
            self._ev("KEY_LEFTSHIFT", 1)
        self._ev(nom, 1)
        self._ev(nom, 0)
        if maj:
            self._ev("KEY_LEFTSHIFT", 0)

    def texte(self, t):
        for c in t:
            self.touche(*touche_de(c))

    def combinaison(self, combo):
        noms = combo.lower().split("+") if len(combo) > 1 else [combo]
        codes = [TOUCHES_NOMMEES.get(n) or touche_de(n)[0] for n in noms]
        for n in codes:
            self._ev(n, 1)
        for n in reversed(codes):
            self._ev(n, 0)
        time.sleep(0.05)

    def __exit__(self, *_):
        time.sleep(0.3)
        self.ui.close()


SCRIPT_FENETRES = """
const actif = workspace.activeWindow;
for (const w of workspace.windowList()) {
    if (!w.normalWindow && !w.dialog) continue;
    console.info("FENETRE|%s|" + (w.desktopFileName || w.resourceClass) + "|" + w.caption
                 + "|" + (w === actif));
}
"""


def gdbus(*args):
    return subprocess.run(["gdbus", "call", "--session", "--dest", "org.kde.KWin", *args],
                          capture_output=True, text=True, check=True).stdout


def fenetres(journal):
    marque = f"m{time.time_ns()}"
    with tempfile.NamedTemporaryFile("w", suffix=".js", delete=False) as f:
        f.write(SCRIPT_FENETRES % marque)
    try:
        num = gdbus("--object-path", "/Scripting", "--method", "org.kde.kwin.Scripting.loadScript",
                    f.name, marque).strip("()\n, ")
        gdbus("--object-path", f"/Scripting/Script{num}", "--method", "org.kde.kwin.Script.run")
        # La sortie du script arrive dans le journal de KWin avec un peu de retard (le
        # premier script est plus lent) : on relit jusqu'à 5 s.
        lignes = []
        limite = time.monotonic() + 5
        while not lignes and time.monotonic() < limite:
            time.sleep(0.3)
            with open(journal, encoding="utf-8", errors="replace") as j:
                lignes = [l for l in j if f"FENETRE|{marque}|" in l]
        gdbus("--object-path", "/Scripting", "--method", "org.kde.kwin.Scripting.unloadScript", marque)
    finally:
        os.unlink(f.name)
    for ligne in lignes:
        reste = ligne.rstrip("\n").split(f"FENETRE|{marque}|", 1)[1]
        app, reste = reste.split("|", 1)
        titre, focus = reste.rsplit("|", 1)
        print(f"{app} — {titre} — focus={focus}")


def main():
    cmd, args = sys.argv[1], sys.argv[2:]
    if cmd in ("taper", "ecrire"):
        with Clavier() as c:
            c.texte(args[0])
            if cmd == "taper":
                c.touche("KEY_ENTER")
    elif cmd == "touches":
        with Clavier() as c:
            for combo in args:
                c.combinaison(combo)
    elif cmd == "fenetres":
        fenetres(args[0])
    else:
        raise SystemExit(__doc__)


main()
