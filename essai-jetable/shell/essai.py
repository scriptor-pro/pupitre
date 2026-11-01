#!/usr/bin/env python3
"""Mesures de l'essai jetable (étape 3, Task 0) : lance chaque shell dans un PTY,
avec un HOME temporaire, et affiche les octets émis (ESC = ^[, BEL = ^G)."""
import os, pty, re, select, shutil, subprocess, sys, tempfile, time

def run(argv, env, inputs, wait=1.5, cwd="/tmp"):
    pid, fd = pty.fork()
    if pid == 0:
        os.chdir(cwd)
        os.execvpe(argv[0], argv, env)
    out = b""
    def drain(t):
        nonlocal out
        end = time.time() + t
        while time.time() < end:
            r, _, _ = select.select([fd], [], [], 0.1)
            if r:
                try:
                    d = os.read(fd, 65536)
                except OSError:
                    return
                if not d:
                    return
                out += d
    drain(wait)
    for i in inputs:
        os.write(fd, i.encode())
        drain(0.8)
    drain(0.5)
    try: os.kill(pid, 9)
    except OSError: pass
    try: os.waitpid(pid, 0)
    except OSError: pass
    return out

def show(b):
    s = b.decode("utf-8", "replace").replace("\x1b", "^[").replace("\x07", "^G").replace("\r", "")
    return s

def marks(b):
    """Ne garde que les marques OSC 133 / OSC 7 (extrait lisible)."""
    s = show(b)
    return " ".join(re.findall(r"\^\[\](?:133|7);[^\^]*(?:\^G|\^\[\\)", s)).replace("\n", " ")

def sans_csi(s):
    return re.sub(r"\^\[\[[0-9;?<=>]*[a-zA-Z]|\^\[\([A-Z]|\^\[[=>]", "", s)

def env_tmp():
    h = tempfile.mkdtemp(prefix="essai-shell-")
    e = {"PATH": os.environ["PATH"], "HOME": h, "TERM": "xterm-256color", "USER": "essai",
         "LANG": "C.UTF-8", "XDG_CONFIG_HOME": h + "/.config", "XDG_DATA_HOME": h + "/.local/share",
         "XDG_STATE_HOME": h + "/.local/state", "XDG_CACHE_HOME": h + "/.cache"}
    for d in (".config", ".local/share", ".local/state", ".cache"):
        os.makedirs(h + "/" + d, exist_ok=True)
    return h, e

def titre(t): print("\n" + "=" * 70 + "\n" + t + "\n" + "=" * 70)
def ecrit(p, s):
    os.makedirs(os.path.dirname(p), exist_ok=True)
    open(p, "w").write(s)
def liste_home(h):
    """Fichiers présents dans HOME hors ce que le test a lui-même posé."""
    return sorted(os.path.relpath(os.path.join(r, f), h) for r, _, fs in os.walk(h) for f in fs)

tmps = []
def nouveau():
    h, e = env_tmp(); tmps.append(h); return h, e

STARSHIP = shutil.which("starship")

# ---------------------------------------------------------------- bash
def essai_bash(avec_starship=False):
    h, e = nouveau()
    ecrit(h + "/.bashrc", 'export VU_BASHRC=1\nPROMPT_COMMAND="VU_PC=1"\nPS1="USERPS1> "\n')
    star = 'eval -- "$(starship init bash --print-full-init)"\n' if avec_starship else ""
    ecrit(h + "/pupitre.bash", r'''[ -r "$HOME/.bashrc" ] && . "$HOME/.bashrc"
''' + star + r'''__p_avant(){ local c=$?; if [ -n "${__p_dej:-}" ]; then printf '\e]133;D;%s\a' "$c"; fi; __p_dej=1
  printf '\e]7;file://%s%s\a' "$HOSTNAME" "$PWD"; return $c; }
__p_B='\[\e]133;B\a\]'
__p_apres(){ local c=$?; PS1="${PS1//"$__p_B"/}$__p_B"; printf '\e]133;A\a'; return $c; }
PROMPT_COMMAND=(__p_avant "${PROMPT_COMMAND[@]}" __p_apres)
PS0="${PS0-}"$'\e]133;C\a'
''')
    cmd = ["bash", "--rcfile", h + "/pupitre.bash", "-i"]
    if avec_starship:
        e["STARSHIP_CONFIG"] = h + "/st.toml"
        ecrit(h + "/st.toml", 'format = "STAR> "\nadd_newline = false\n')
    o = run(cmd, e, ["echo VBR=$VU_BASHRC PC=$VU_PC PS0=[${PS0:+set}]\n", "false\n", "exit\n"])
    return h, cmd, o

def bash_sysrc():
    """/etc/bash.bashrc est-il lu avec --rcfile ? (strace)"""
    h, e = nouveau()
    ecrit(h + "/r", "true\n")
    t = h + "/st.txt"
    subprocess.run(["strace", "-f", "-e", "trace=openat", "-o", t, "bash", "--rcfile", h + "/r", "-i", "-c", "true"],
                   env=e, stdin=subprocess.DEVNULL, capture_output=True)
    s = open(t).read() if os.path.exists(t) else ""
    return [l.split('"')[1] for l in s.splitlines() if ("bash.bashrc" in l or '/r"' in l) and "ENOENT" not in l]

# ---------------------------------------------------------------- zsh
def essai_zsh(avec_starship=False, login=False, zd_utilisateur=False):
    h, e = nouveau()
    ud = h + "/zd" if zd_utilisateur else h
    ecrit(ud + "/.zshenv", "export VU_ZSHENV=1\n")
    ecrit(ud + "/.zshrc", 'export VU_ZSHRC=1\nPROMPT="USERPS> "\nprecmd(){ VU_PRECMD=1 }\n')
    z = h + "/pup"
    pre = '''__pu=$ZDOTDIR
if [ -n "${PUPITRE_ZDOTDIR_ORIG+x}" ]; then ZDOTDIR=$PUPITRE_ZDOTDIR_ORIG; else unset ZDOTDIR; fi
[ -r "${ZDOTDIR:-$HOME}/%s" ] && . "${ZDOTDIR:-$HOME}/%s"
ZDOTDIR=$__pu
'''
    for n in (".zshenv", ".zprofile", ".zshrc", ".zlogin"):
        ecrit(z + "/" + n, pre % (n, n))
    star = 'eval "$(starship init zsh --print-full-init)"\n' if avec_starship else ""
    open(z + "/.zshrc", "a").write(star + r'''
__p_precmd(){ local c=$?; (( ${+__p_dej} )) && print -n "\e]133;D;$c\a"; __p_dej=1
  print -n "\e]7;file://$HOST$PWD\a\e]133;A\a"; }
__p_preexec(){ print -n "\e]133;C\a"; }
precmd_functions+=(__p_precmd); preexec_functions+=(__p_preexec)
PROMPT="${PROMPT}%{"$'\e]133;B\a'"%}"
if [ -n "${PUPITRE_ZDOTDIR_ORIG+x}" ]; then ZDOTDIR=$PUPITRE_ZDOTDIR_ORIG; else unset ZDOTDIR; fi
''')
    if zd_utilisateur:
        e["PUPITRE_ZDOTDIR_ORIG"] = ud
    e["ZDOTDIR"] = z   # (cas réel : PUPITRE_ZDOTDIR_ORIG = ZDOTDIR de l'utilisateur s'il existait)
    if avec_starship:
        e["STARSHIP_CONFIG"] = h + "/st.toml"
        ecrit(h + "/st.toml", 'format = "STAR> "\nadd_newline = false\n')
    cmd = ["zsh", "-i"] + (["-l"] if login else [])
    o = run(cmd, e, ["echo ZE=$VU_ZSHENV ZR=$VU_ZSHRC PC=$VU_PRECMD ZD=${ZDOTDIR:-unset} npre=${#precmd_functions} nexec=${#preexec_functions}\n", "false\n", "exit\n"])
    return h, cmd, o

# ---------------------------------------------------------------- fish
def essai_fish(avec_starship=False, natif=False):
    h, e = nouveau()
    ecrit(h + "/.config/fish/config.fish", "set -gx VU_FISH 1\nfunction fish_prompt; echo -n 'USERFISH> '; end\n")
    extra = "set -gx VU_AFTER $VU_FISH"
    if avec_starship:
        e["STARSHIP_CONFIG"] = h + "/st.toml"
        ecrit(h + "/st.toml", 'format = "STAR> "\nadd_newline = false\n')
        extra += "; starship init fish | source"
    cmd = ["fish", "-i"] + ([] if natif else ["-C", extra])
    o = run(cmd, e, ["echo VF=$VU_FISH AFTER=$VU_AFTER\n", "false\n", "exit\n"])
    return h, cmd, o

# ---------------------------------------------------------------- nushell
def essai_nu(mode):
    h, e = nouveau()
    cfg = "$env.VU_NU = '1'\n$env.config.show_banner = false\n"
    if mode in ("osc133-coupe", "osc133-coupe-seul"):
        cfg += "$env.config.shell_integration.osc133 = false\n"
    ecrit(h + "/.config/nushell/config.nu", cfg)
    ecrit(h + "/.config/nushell/env.nu", "$env.VU_NUENV = '1'\n")
    extra = "$env.VU_AFTER = $env.VU_NU\n$env.config.shell_integration.osc133 = true\n$env.config.shell_integration.osc7 = true\n"
    if mode == "starship":
        e["STARSHIP_CONFIG"] = h + "/st.toml"
        ecrit(h + "/st.toml", 'format = "STAR> "\nadd_newline = false\n')
        sh = subprocess.run(["starship", "init", "nu"], capture_output=True, text=True, env=e).stdout
        ecrit(h + "/starship.nu", sh)
        extra += "source " + h + "/starship.nu\n"
    ecrit(h + "/pupitre.nu", extra)
    cmd = ["nu"] + ([] if mode in ("natif", "osc133-coupe-seul") else ["-e", "source " + h + "/pupitre.nu"])
    o = run(cmd, e, ['print $"NU=($env.VU_NU?) ENV=($env.VU_NUENV?) AFTER=($env.VU_AFTER?) osc133=($env.config.shell_integration.osc133)"\n', "false\n", "exit\n"], wait=3)
    return h, cmd, o

# ---------------------------------------------------------------- elvish
def essai_elvish(natif=False):
    h, e = nouveau()
    ecrit(h + "/.config/elvish/rc.elv", "set E:VU_ELV = 1\nset edit:prompt = { put 'USERELV> ' }\n")
    ecrit(h + "/pupitre.elv", r'''eval (slurp < $E:XDG_CONFIG_HOME/elvish/rc.elv)
set E:VU_AFTER = $E:VU_ELV
var p = $edit:prompt
set edit:before-readline = [$@edit:before-readline { print "\e]133;D\a\e]7;file://"(hostname)$pwd"\a\e]133;A\a" }]
set edit:prompt = { $p; print "\e]133;B\a" }
set edit:after-readline = [$@edit:after-readline {|c| print "\e]133;C\a" }]
''')
    cmd = ["elvish"] + ([] if natif else ["-rc", h + "/pupitre.elv"])
    o = run(cmd, e, ["echo VE=$E:VU_ELV AFTER=$E:VU_AFTER\n", "exit\n"], wait=2)
    return h, cmd, o

# ---------------------------------------------------------------- Starship
def essai_starship():
    h, e = nouveau()
    r = {}
    for sh in ("bash", "zsh", "fish", "nu", "elvish", "xonsh", "tcsh"):
        p = subprocess.run(["starship", "init", sh, "--print-full-init"], capture_output=True, text=True, env=e)
        r[sh] = (len(p.stdout), "133" in p.stdout, p.stdout.strip().splitlines()[:1])
    p = subprocess.run(["starship", "init", "nu"], capture_output=True, text=True, env=e)
    r["nu (starship init nu)"] = (len(p.stdout), "133" in p.stdout, [])
    ecrit(h + "/.config/starship.toml", 'format = "USERTOML> "\n')
    ecrit(h + "/alt.toml", 'format = "ENVTOML> "\n')
    res = {}
    for nom, extra in (("avec STARSHIP_CONFIG", {"STARSHIP_CONFIG": h + "/alt.toml"}), ("sans STARSHIP_CONFIG", {})):
        t = h + "/st.txt"
        env = dict(e, **extra)
        p = subprocess.run(["strace", "-f", "-e", "trace=openat", "-o", t, "starship", "prompt"],
                           env=env, capture_output=True, text=True, cwd=h)
        ouverts = [l.split('"')[1] for l in open(t) if "toml" in l and '"' in l and "ENOENT" not in l]
        tentes = [l.split('"')[1] for l in open(t) if "toml" in l and '"' in l]
        res[nom] = (p.stdout.strip(), ouverts, tentes)
    return r, res

def main():
    titre("Versions")
    for c in (["bash", "--version"], ["zsh", "--version"], ["fish", "--version"], ["nu", "--version"],
              ["elvish", "-version"], ["starship", "--version"]):
        print(subprocess.run(c, capture_output=True, text=True).stdout.splitlines()[0])
    for sh in ("xonsh", "tcsh"):
        print(sh, ":", shutil.which(sh) or "NON INSTALLÉ (non testé, absent)")

    titre("1. bash : bash --rcfile <pupitre.bash> -i")
    print("fichiers lus (strace, avec --rcfile) :", bash_sysrc())
    h, cmd, o = essai_bash()
    print("commande :", " ".join(cmd)); print("marques :", marks(o)); print("sortie lisible :", sans_csi(show(o))[:900])
    print("fichiers créés dans HOME :", liste_home(h))
    titre("1b. bash + starship (init après .bashrc, marques après)")
    h, cmd, o = essai_bash(True)
    print("marques :", marks(o)); print("sortie lisible :", sans_csi(show(o))[:900])

    titre("2. zsh : ZDOTDIR=<dossier pupitre> zsh -i")
    h, cmd, o = essai_zsh()
    print("marques :", marks(o)); print("sortie lisible :", sans_csi(show(o))[:900])
    print("fichiers dans HOME hors tests :", [f for f in liste_home(h) if not f.startswith(("pup/", ".zsh"))])
    titre("2b. zsh -l (login)")
    h, cmd, o = essai_zsh(login=True); print("marques :", marks(o)); print(sans_csi(show(o))[:500])
    titre("2d. zsh, l'utilisateur avait son propre ZDOTDIR (PUPITRE_ZDOTDIR_ORIG)")
    h, cmd, o = essai_zsh(zd_utilisateur=True); print("marques :", marks(o)); print(sans_csi(show(o))[:400])
    titre("2c. zsh + starship")
    h, cmd, o = essai_zsh(True); print("marques :", marks(o)); print(sans_csi(show(o))[:700])

    titre("3a. fish : natif (aucune option)")
    h, cmd, o = essai_fish(natif=True)
    print("marques :", marks(o))
    fs = liste_home(h); print("fichiers dans HOME :", len(fs), "dont", len([f for f in fs if f.startswith(".cache/fish/")]), "dans .cache/fish (completions générées, XDG_CACHE_HOME) ; autres :", [f for f in fs if not f.startswith(".cache/fish/")])
    titre("3b. fish -C '<commandes>' (après config.fish ?)")
    h, cmd, o = essai_fish()
    print("commande :", " ".join(cmd)); print("sortie :", sans_csi(show(o))[-700:])
    titre("3c. fish -C + starship")
    h, cmd, o = essai_fish(True); print("marques :", marks(o)); print(sans_csi(show(o))[-500:])

    titre("4a. nushell natif (config par défaut + config utilisateur)")
    h, cmd, o = essai_nu("natif"); print("marques :", marks(o)); print(sans_csi(show(o))[-600:])
    titre("4b. nushell, utilisateur coupe osc133, nu -e 'source pupitre.nu' le rétablit")
    h, cmd, o = essai_nu("osc133-coupe"); print("commande :", cmd[:2], "<h>/pupitre.nu"); print("marques :", marks(o)); print(sans_csi(show(o))[-600:])
    titre("4b'. nushell, utilisateur coupe osc133, SANS notre fichier (témoin)")
    h, cmd, o = essai_nu("osc133-coupe-seul"); print("marques :", marks(o) or "(aucune)")
    titre("4c. nushell -e + starship init nu")
    h, cmd, o = essai_nu("starship"); print("marques :", marks(o)); print(sans_csi(show(o))[-600:])

    titre("5a. elvish natif")
    h, cmd, o = essai_elvish(True); print("marques :", marks(o) or "(aucune)")
    titre("5b. elvish -rc <pupitre.elv>")
    h, cmd, o = essai_elvish(); print("commande :", cmd[:2]); print("marques :", marks(o)); print("fichiers dans HOME :", liste_home(h))
    print("B dans les marques extraites :", "133;B" in marks(o), "| B visible en clair dans l'invite :", "]133;B" in show(o))

    titre("6. Starship")
    init, res = essai_starship()
    for k, v in init.items(): print(f"init {k}: {v[0]} octets, contient '133' : {v[1]}", v[2][:1])
    for k, v in res.items(): print(k, "-> invite :", repr(v[0]), "| .toml ouverts :", v[1], "| .toml tentés :", v[2])

try:
    main()
finally:
    for t in tmps: shutil.rmtree(t, ignore_errors=True)
