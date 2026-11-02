# SPDX-License-Identifier: GPL-3.0-or-later
"""Vérifie, sous strace, qu'une session type de Pupitre :
- n'ouvre aucune connexion réseau (seulement des sockets Unix) ;
- n'écrit rien hors de ses dossiers ($XDG_CONFIG_HOME/pupitre, $XDG_STATE_HOME/pupitre,
  et son socket dans $XDG_RUNTIME_DIR/pupitre) ;
- ne laisse la phrase de passe d'une clé SSH de test dans aucun fichier sous $HOME ni
  $XDG_RUNTIME_DIR (recherche de la chaîne après la session), ni dans les arguments d'un
  `execve` de la trace.

Elle se déroule avec l'invite Starship par défaut, dans un dépôt git : Starship (lancé par
les shells de Pupitre) est jugé comme Pupitre, et le `~/.config/starship.toml` du HOME de test
ne doit jamais être ouvert (Pupitre impose son propre fichier avec STARSHIP_CONFIG).

Seuls Pupitre, pupitre-fond, pupitre-askpass et starship sont jugés : les shells lancés dans les onglets (et leurs
commandes, dont `ssh`) écrivent ce qu'ils veulent, ce n'est pas le fait de Pupitre.

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

# Caches de bibliothèques chargées par GTK (pas par le code de Pupitre) et journal de Starship, dans
# $XDG_CACHE_HOME : signalés à part, sans faire échouer la vérification.
CACHES_BIBLIOTHEQUES = ("mesa_shader_cache", "mesa_shader_cache_db", "fontconfig",
                        # Journal de session de Starship (~/.cache/starship/session_*.log) : écrit
                        # par Starship lui-même, dans le seul dossier de cache de l'utilisateur.
                        "starship")

# Phrase de passe de la clé de test (jamais utilisée ailleurs). La clé est créée AVANT le
# démarrage de strace : `ssh-keygen -N` ne figure donc pas dans la trace.
PHRASE_CLE = "phrase-isolement-7316"

# Appel coupé par strace -f : « <unfinished ...> », puis « <... nom resumed> » plus loin.
COUPE = re.compile(r"^(\d+)\s+(.*?)\s*<unfinished \.\.\.>\s*$")
REPRIS = re.compile(r"^(\d+)\s+<\.\.\. \w+ resumed>(.*)$")
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
    # Dans un dépôt git : l'invite Starship lit la branche et l'état à chaque affichage.
    taper(env, "cd ~/depot && echo modif > nouveau && cd ~")
    time.sleep(1)
    taper(env, "cd ~/depot")
    time.sleep(2)
    taper(env, "echo bonjour; ls /")
    # Clé SSH : ssh-add (la fenêtre de phrase de passe est remplie au clavier, la phrase
    # est mémorisée dans le trousseau jetable), puis une connexion refusée par l'utilisateur.
    taper(env, "ssh-add ~/.ssh/id_test")
    time.sleep(4)
    subprocess.run(["xdotool", "type", "--delay", "40", PHRASE_CLE], env=env, check=True)
    touches(env, "Return")
    time.sleep(4)
    taper(env, "ssh-add -l > ~/liste-cles")
    taper(env, "ssh -p 1 127.0.0.1")
    time.sleep(2)
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


def phrase_dans_fichiers(racines, phrase):
    """Fichiers réguliers (liens non suivis) contenant `phrase`, lus en binaire."""
    cible = phrase.encode()
    trouves, lus = [], 0
    for racine in racines:
        for dossier, _, noms in os.walk(racine):
            for nom in noms:
                chemin = os.path.join(dossier, nom)
                try:
                    if os.path.islink(chemin) or not os.path.isfile(chemin):
                        continue
                    with open(chemin, "rb") as f:
                        lus += 1
                        if cible in f.read():
                            trouves.append(chemin)
                except OSError:
                    pass
    return trouves, lus


def recoller(lignes):
    """Reforme les appels que strace -f a coupés en deux (sinon ils échappent à l'analyse)."""
    en_cours, sortie = {}, []
    for l in lignes:
        c = COUPE.match(l)
        if c:
            en_cours[c.group(1)] = c.group(2)
            continue
        r = REPRIS.match(l)
        if r:
            debut = en_cours.pop(r.group(1), None)
            if debut is not None:
                sortie.append(f"{r.group(1)} {debut}{r.group(2)}\n")
            continue
        sortie.append(l)
    return sortie


def starship_perso_ouvert(trace, chemin):
    """Ouvertures (en lecture ou écriture, par n'importe quel programme) du fichier `chemin`."""
    with open(trace, encoding="utf-8", errors="replace") as f:
        return [l.strip()[:160] for l in recoller(f.readlines())
                if re.match(r"^\d+\s+(openat|open|creat|stat|newfstatat|statx|access|faccessat2?)\(", l)
                and f'"{chemin}"' in l]


def analyser(trace, autorises, cache):
    programmes = {}  # pid → programme exécuté
    premier = None
    reseau, ecritures, caches = [], [], set()
    ignores = []  # connexions réseau de programmes non jugés (ssh de l'utilisateur)
    phrase_argv = []
    with open(trace, encoding="utf-8", errors="replace") as f:
        lignes = f.readlines()
    lignes = recoller(lignes)
    for l in lignes:
        m = LIGNE.match(l)
        if not m:
            continue
        pid, appel, args, ret = m.groups()
        if appel == "execve" and PHRASE_CLE in l:
            phrase_argv.append(f"{os.path.basename(args.split(chr(34))[1])}[{pid}] {l.strip()[:120]}")
        if appel == "execve" and ret == "0":
            prog = os.path.basename(args.split('"')[1])
            programmes[pid] = prog
            premier = premier or pid
        elif appel in ("clone", "clone3", "vfork", "fork") and ret.isdigit():
            programmes.setdefault(ret, programmes.get(pid, "?"))
    juges = {"pupitre", "pupitre-fond", "pupitre-askpass", "starship"}
    for l in lignes:
        m = LIGNE.match(l)
        if not m:
            continue
        pid, appel, args, ret = m.groups()
        prog = programmes.get(pid, "pupitre" if pid == premier else "?")
        if prog not in juges:
            if appel == "connect" and "sa_family=AF_INET" in args:
                ignores.append(prog)
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
    return (reseau, sorted(set(ecritures)), sorted(caches), phrase_argv,
            sorted(set(programmes.values())), ignores)


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
    os.makedirs(os.path.join(perso, "tmp"), mode=0o700, exist_ok=True)
    env["TMPDIR"] = os.path.join(perso, "tmp")
    # Jamais l'agent SSH ni le trousseau de l'utilisateur : le fond lance son propre
    # ssh-agent ; le trousseau est celui, jetable, de scripts/trousseau-test.sh.
    for var in ("SSH_AUTH_SOCK", "SSH_AGENT_PID", "SSH_ASKPASS", "SSH_ASKPASS_REQUIRE"):
        env.pop(var, None)
    ssh = os.path.join(perso, ".ssh")
    os.makedirs(ssh, mode=0o700, exist_ok=True)
    subprocess.run(["ssh-keygen", "-t", "ed25519", "-q", "-N", PHRASE_CLE, "-C", "cle-de-test",
                    "-f", os.path.join(ssh, "id_test")], check=True)
    # Dépôt git de test (créé avant strace) : l'invite Starship affiche branche et état.
    depot = os.path.join(perso, "depot")
    os.makedirs(depot)
    git_env = dict(env, GIT_CONFIG_GLOBAL="/dev/null", GIT_CONFIG_SYSTEM="/dev/null",
                   GIT_AUTHOR_NAME="t", GIT_AUTHOR_EMAIL="t@example.invalid",
                   GIT_COMMITTER_NAME="t", GIT_COMMITTER_EMAIL="t@example.invalid")
    for cmd in (["init", "-q", "-b", "main"], ["commit", "-q", "--allow-empty", "-m", "initial"]):
        subprocess.run(["git", "-C", depot, *cmd], env=git_env, check=True,
                       stdout=subprocess.DEVNULL)
    # Un starship.toml personnel, qui ne doit jamais être ouvert (HOME de test, pas le vrai).
    perso_toml = os.path.join(env["XDG_CONFIG_HOME"], "starship.toml")
    with open(perso_toml, "w", encoding="utf-8") as f:
        f.write('format = "PERSONNEL $character"\n')
    env.update(SHELL="/bin/bash", GDK_BACKEND="x11", GIO_USE_VFS="local",
               GDK_DEBUG="no-portals", PUPITRE_FOND=os.path.abspath(a.fond))
    env.pop("WAYLAND_DISPLAY", None)
    trace = os.path.join(a.sortie, "strace.log")
    arret = "propre"  # Pupitre se ferme seul après la session ; sinon c'est un échec
    wm = subprocess.Popen(["xfwm4", "--compositor=off"], env=env,
                          stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(1)
    proc = subprocess.Popen(
        ["strace", "-f", "-qq", "-o", trace, "-s", "256",
         "-e", "trace=execve,clone,clone3,vfork,fork,connect,bind,openat,open,creat,"
               "rename,renameat,renameat2,mkdir,mkdirat,unlink,unlinkat,stat,newfstatat,statx,access,faccessat,faccessat2",
         os.path.abspath(a.pupitre)],
        env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        time.sleep(4)
        session_type(env, perso)
        try:
            proc.wait(timeout=20)
        except subprocess.TimeoutExpired:
            arret = "forcé (SIGTERM)"
            proc.send_signal(signal.SIGTERM)
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                arret = "forcé (SIGKILL)"
                proc.kill()
                proc.wait()
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
    (reseau, ecritures, caches, phrase_argv, programmes,
     ignores) = analyser(trace, autorises, env["XDG_CACHE_HOME"])
    # Les étapes SSH ont-elles eu lieu ? (sinon la vérification ne prouverait rien)
    manquants = [p for p in ("pupitre-askpass", "ssh-add", "ssh", "starship") if p not in programmes]
    perso_ouvert = starship_perso_ouvert(trace, perso_toml)
    config_pupitre = os.path.exists(os.path.join(env["XDG_CONFIG_HOME"], "pupitre", "starship.toml"))
    try:
        with open(os.path.join(perso, "liste-cles"), encoding="utf-8") as f:
            cle_chargee = "cle-de-test" in f.read()
    except OSError:
        cle_chargee = False
    fichiers, lus = phrase_dans_fichiers([env["HOME"], env["XDG_RUNTIME_DIR"]], PHRASE_CLE)
    lignes = ["Connexions réseau (hors sockets Unix) : " + ("aucune" if not reseau else "")]
    lignes += [f"  {r}" for r in reseau]
    lignes.append("Écritures hors des dossiers de Pupitre : " + ("aucune" if not ecritures else ""))
    lignes += [f"  {e}" for e in ecritures]
    lignes.append("Connexions IPv4/IPv6 de programmes non jugés (ssh de l'utilisateur) : "
                  f"{len(ignores)} ({', '.join(sorted(set(ignores))) or 'aucune'})")
    lignes.append("Étapes exécutées (pupitre-askpass, ssh-add, ssh, starship dans la trace ; clé "
                  "chargée dans l'agent) : "
                  + ("oui" if not manquants and cle_chargee else
                     f"NON (manquants : {manquants}, clé chargée : {cle_chargee})"))
    lignes.append("~/.config/starship.toml personnel ouvert ou consulté : "
                  + ("jamais" if not perso_ouvert else ""))
    lignes += [f"  {o}" for o in perso_ouvert]
    lignes.append("Invite Starship de Pupitre (pupitre/starship.toml créé) : "
                  + ("oui" if config_pupitre else "NON"))
    lignes.append("Phrase de passe de test dans un fichier sous HOME ou XDG_RUNTIME_DIR "
                  f"({lus} fichiers lus) : " + ("aucune" if not fichiers else ""))
    lignes += [f"  {f}" for f in fichiers]
    lignes.append(f"Arrêt de Pupitre : {arret}")
    lignes.append("Phrase de passe de test dans les arguments d'un execve : "
                  + ("aucune" if not phrase_argv else ""))
    lignes += [f"  {f}" for f in phrase_argv]
    if caches:
        lignes.append("Caches de bibliothèques système dans ~/.cache (tolérés) : "
                      + ", ".join(caches))
    rapport = "\n".join(lignes).replace(perso, "~") + "\n"
    with open(os.path.join(a.sortie, "rapport.txt"), "w", encoding="utf-8") as f:
        f.write(rapport)
    print(rapport)
    shutil.rmtree(perso, ignore_errors=True)
    return 1 if (reseau or ecritures or fichiers or phrase_argv or manquants
                 or not cle_chargee or perso_ouvert or not config_pupitre or arret != "propre") else 0


sys.exit(main())
