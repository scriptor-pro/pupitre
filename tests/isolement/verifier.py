# SPDX-License-Identifier: GPL-3.0-or-later
"""Vérifie, sous strace, qu'une session type de Pupitre :
- n'ouvre aucune connexion réseau (seulement des sockets Unix) ;
- n'écrit rien hors de ses dossiers ($XDG_CONFIG_HOME/pupitre, $XDG_STATE_HOME/pupitre,
  et son socket dans $XDG_RUNTIME_DIR/pupitre).

Seuls Pupitre et pupitre-fond sont jugés : les shells lancés dans les onglets (et leurs
commandes) écrivent ce qu'ils veulent, ce n'est pas le fait de Pupitre.

Usage (sur un écran X11, normalement via scripts/verifier-isolement.sh) :
  python3 verifier.py --pupitre CHEMIN --fond CHEMIN --sortie DOSSIER
Code 0 si tout est conforme, 1 sinon ; la trace complète reste dans DOSSIER/strace.log.
"""
import argparse
import os
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time

# Écritures attendues hors des dossiers de Pupitre : terminaux, périphériques, fichiers
# temporaires anonymes, mémoire partagée (Mesa, GTK), /proc du processus lui-même.
TOLERES = (
    re.compile(r"^/dev/(pts/\d+|ptmx|tty|null|udmabuf|dri/.*|shm/.*)$"),
    re.compile(r"^/proc/(self|\d+)/"),
    re.compile(r"^/memfd:"),
)

# Caches de bibliothèques chargées par GTK (pas par le code de Pupitre), dans
# $XDG_CACHE_HOME : signalés à part, sans faire échouer la vérification.
CACHES_BIBLIOTHEQUES = ("mesa_shader_cache", "mesa_shader_cache_db", "fontconfig")

LIGNE = re.compile(r"^(\d+)\s+(\w+)\((.*)\)\s+=\s+(-?\d+|\?)")


def touches(env, *noms):
    for n in noms:
        subprocess.run(["xdotool", "key", "--clearmodifiers", n], env=env, check=True)
        time.sleep(0.4)


def taper(env, texte):
    subprocess.run(["xdotool", "type", "--delay", "40", texte], env=env, check=True)
    touches(env, "Return")


def session_type(env, perso):
    """Onglets, commandes, recherche, zoom, configuration modifiée, fermeture."""
    taper(env, "echo bonjour; ls /")
    touches(env, "ctrl+shift+t")
    taper(env, "printf '\\e]0;essai\\a'")
    touches(env, "ctrl+shift+f")
    taper(env, "bonjour")
    touches(env, "Escape", "ctrl+plus", "ctrl+plus", "ctrl+0")
    touches(env, "ctrl+shift+n")  # seconde fenêtre
    time.sleep(1)
    chemin = os.path.join(env["XDG_CONFIG_HOME"], "pupitre", "pupitre.toml")
    if os.path.exists(chemin):
        with open(chemin, encoding="utf-8") as f:
            t = f.read()
        with open(chemin, "w", encoding="utf-8") as f:
            f.write(t.replace("historique_lignes = 10000", "historique_lignes = 12000"))
    time.sleep(2)
    for _ in range(4):  # fermer les shells : la dernière fenêtre se ferme
        taper(env, "exit")
        time.sleep(0.5)
        touches(env, "ctrl+shift+w")
        time.sleep(0.5)


def analyser(trace, autorises, cache):
    programmes = {}  # pid → programme exécuté
    premier = None
    reseau, ecritures, caches = [], [], set()
    with open(trace, encoding="utf-8", errors="replace") as f:
        lignes = f.readlines()
    for l in lignes:
        m = LIGNE.match(l)
        if not m:
            continue
        pid, appel, args, ret = m.groups()
        if appel == "execve" and ret == "0":
            prog = os.path.basename(args.split('"')[1])
            programmes[pid] = prog
            premier = premier or pid
        elif appel in ("clone", "clone3", "vfork", "fork") and ret.isdigit():
            programmes.setdefault(ret, programmes.get(pid, "?"))
    juges = {"pupitre", "pupitre-fond"}
    for l in lignes:
        m = LIGNE.match(l)
        if not m:
            continue
        pid, appel, args, ret = m.groups()
        prog = programmes.get(pid, "pupitre" if pid == premier else "?")
        if prog not in juges:
            continue
        if appel == "connect" and "sa_family=AF_UNIX" not in args:
            reseau.append(f"{prog}[{pid}] {l.strip()}")
        chemins = []
        if appel in ("openat", "open", "creat") and ret != "-1" and re.search(
                r"O_WRONLY|O_RDWR|O_CREAT|O_TRUNC", args):
            chemins = re.findall(r'"([^"]*)"', args)[:1]
        elif appel in ("rename", "renameat", "renameat2", "mkdir", "mkdirat", "unlink",
                       "unlinkat", "bind") and not ret.startswith("-"):
            chemins = re.findall(r'"([^"]*)"', args) or re.findall(r"sun_path=\"([^\"]*)", args)
        for c in chemins:
            if not c.startswith("/"):
                continue  # relatif à un dossier déjà ouvert : couvert par son ouverture
            if any(c == a or c.startswith(a + "/") for a in autorises):
                continue
            if any(t.match(c) for t in TOLERES):
                continue
            nom = next((n for n in CACHES_BIBLIOTHEQUES
                        if c == os.path.join(cache, n) or c.startswith(os.path.join(cache, n) + "/")),
                       None)
            if nom:
                caches.add(nom)
                continue
            ecritures.append(f"{prog}[{pid}] {appel} {c}")
    return reseau, sorted(set(ecritures)), sorted(caches)


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--pupitre", required=True)
    p.add_argument("--fond", required=True)
    p.add_argument("--sortie", required=True)
    a = p.parse_args()
    os.makedirs(a.sortie, exist_ok=True)
    perso = tempfile.mkdtemp(prefix="pupitre-isolement-")
    env = dict(os.environ)
    for var, sous in {"HOME": "", "XDG_CONFIG_HOME": ".config",
                      "XDG_STATE_HOME": ".local/state", "XDG_DATA_HOME": ".local/share",
                      "XDG_CACHE_HOME": ".cache", "XDG_RUNTIME_DIR": "run"}.items():
        d = os.path.join(perso, sous)
        os.makedirs(d, mode=0o700, exist_ok=True)
        env[var] = d
    env.update(SHELL="/bin/bash", GDK_BACKEND="x11", GIO_USE_VFS="local",
               GDK_DEBUG="no-portals", PUPITRE_FOND=os.path.abspath(a.fond))
    env.pop("WAYLAND_DISPLAY", None)
    trace = os.path.join(a.sortie, "strace.log")
    wm = subprocess.Popen(["xfwm4", "--compositor=off"], env=env,
                          stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(1)
    proc = subprocess.Popen(
        ["strace", "-f", "-qq", "-o", trace, "-s", "256",
         "-e", "trace=execve,clone,clone3,vfork,fork,connect,bind,openat,open,creat,"
               "rename,renameat,renameat2,mkdir,mkdirat,unlink,unlinkat",
         os.path.abspath(a.pupitre)],
        env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        time.sleep(4)
        session_type(env, perso)
        try:
            proc.wait(timeout=20)
        except subprocess.TimeoutExpired:
            proc.send_signal(signal.SIGTERM)
            proc.wait(timeout=10)
        # Le processus de fond s'arrête seul quand il n'a plus de client ni de session.
        limite = time.time() + 30
        while time.time() < limite and subprocess.run(
                ["pgrep", "-f", env["XDG_RUNTIME_DIR"]], capture_output=True).returncode == 0:
            time.sleep(0.5)
    finally:
        subprocess.run(["pkill", "-f", env["XDG_RUNTIME_DIR"]])
        wm.terminate()
    autorises = [os.path.join(env["XDG_CONFIG_HOME"], "pupitre"),
                 os.path.join(env["XDG_STATE_HOME"], "pupitre"),
                 os.path.join(env["XDG_RUNTIME_DIR"], "pupitre")]
    reseau, ecritures, caches = analyser(trace, autorises, env["XDG_CACHE_HOME"])
    lignes = ["Connexions réseau (hors sockets Unix) : " + ("aucune" if not reseau else "")]
    lignes += [f"  {r}" for r in reseau]
    lignes.append("Écritures hors des dossiers de Pupitre : " + ("aucune" if not ecritures else ""))
    lignes += [f"  {e}" for e in ecritures]
    if caches:
        lignes.append("Caches de bibliothèques système dans ~/.cache (tolérés) : "
                      + ", ".join(caches))
    rapport = "\n".join(lignes).replace(perso, "~") + "\n"
    with open(os.path.join(a.sortie, "rapport.txt"), "w", encoding="utf-8") as f:
        f.write(rapport)
    print(rapport)
    shutil.rmtree(perso, ignore_errors=True)
    return 1 if (reseau or ecritures) else 0


sys.exit(main())
