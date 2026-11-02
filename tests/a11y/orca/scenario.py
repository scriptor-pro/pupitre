# SPDX-License-Identifier: GPL-3.0-or-later
"""Liste de contrôle Orca automatisée (docs/orca-liste-de-controle.md).

Lance, sur l'écran X11 courant (normalement un Xvfb ouvert par scripts/test-orca.sh) :
un gestionnaire de fenêtres (xfwm4), Orca muet (lancer_orca.py) et Pupitre ; envoie les
touches avec xdotool et compare ce qu'Orca aurait dit (son journal) avec l'attendu.

Usage : python3 scenario.py --pupitre CHEMIN --fond CHEMIN --sortie DOSSIER
Code 0 si toutes les vérifications passent, 1 sinon. Le journal complet d'Orca et un
rapport (rapport.txt) sont laissés dans DOSSIER.

Rien n'est lu ni écrit chez l'utilisateur : dossier personnel et dossiers XDG temporaires.
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

ICI = os.path.dirname(os.path.abspath(__file__))
INVITE = "invite$ "
# Phrase de la clé SSH de test (jamais utilisée ailleurs ; clé créée dans le HOME temporaire).
PHRASE_CLE = "phrase-de-test-orca-4821"
# Une phrase d'Orca dans son journal : « SPEECH OUTPUT: 'texte' {'established': … } ».
PHRASE = re.compile(r"SPEECH OUTPUT: '(.*?)' \{'established'", re.S)


class Echec(Exception):
    pass


class Session:
    """Processus lancés pour le test et lecture du journal d'Orca."""

    def __init__(self, pupitre, fond, sortie):
        self.sortie = sortie
        self.journal = os.path.join(sortie, "orca.log")
        self.perso = tempfile.mkdtemp(prefix="pupitre-orca-")
        self.procs = []
        self.lu = 0
        env = dict(os.environ)
        dossiers = {
            "HOME": "",
            "XDG_CONFIG_HOME": ".config",
            "XDG_STATE_HOME": ".local/state",
            "XDG_DATA_HOME": ".local/share",
            "XDG_CACHE_HOME": ".cache",
            "XDG_RUNTIME_DIR": "run",
            "TMPDIR": "tmp",
        }
        for var, sous in dossiers.items():
            d = os.path.join(self.perso, sous)
            os.makedirs(d, mode=0o700, exist_ok=True)
            env[var] = d
        with open(os.path.join(self.perso, ".bashrc"), "w", encoding="utf-8") as f:
            f.write(f'PS1="{INVITE}"\nunset PROMPT_COMMAND\n')
        # L'invite du .bashrc ci-dessus est celle que le scénario attend : pas d'invite Starship
        # de Pupitre (réglage « invite = "shell" »).
        os.makedirs(os.path.join(self.perso, ".config", "pupitre"), exist_ok=True)
        with open(os.path.join(self.perso, ".config", "pupitre", "pupitre.toml"), "w", encoding="utf-8") as f:
            f.write('historique_lignes = 10000\ninvite = "shell"\n')
        # Jamais l'agent ni le trousseau de l'utilisateur : le fond lance son propre
        # ssh-agent dans XDG_RUNTIME_DIR (TMPDIR temporaire : pas de recherche dans /tmp),
        # le trousseau est celui, jetable, de scripts/trousseau-test.sh.
        for var in ("SSH_AUTH_SOCK", "SSH_AGENT_PID", "SSH_ASKPASS", "SSH_ASKPASS_REQUIRE"):
            env.pop(var, None)
        self.preparer_ssh()
        env.update(
            SHELL="/bin/bash",
            GDK_BACKEND="x11",
            GIO_USE_VFS="local",
            GDK_DEBUG="no-portals",
            # Garde-fou : même si une synthèse se lançait, aucun son ne sortirait.
            PULSE_SERVER="unix:/nonexistent",
            PIPEWIRE_REMOTE="/nonexistent",
            PUPITRE_FOND=fond,
            LANG="fr_FR.UTF-8",
            LANGUAGE="fr",
        )
        env.pop("WAYLAND_DISPLAY", None)
        self.env = env
        self.pupitre = pupitre

    def preparer_ssh(self):
        """Clé de test chiffrée, ~/.ssh/config et connexions.toml pour les vérifications 15 et 16."""
        ssh = os.path.join(self.perso, ".ssh")
        os.makedirs(ssh, mode=0o700, exist_ok=True)
        subprocess.run(["ssh-keygen", "-t", "ed25519", "-q", "-N", PHRASE_CLE,
                        "-C", "cle-de-test", "-f", os.path.join(ssh, "id_test")], check=True)
        with open(os.path.join(ssh, "config"), "w", encoding="utf-8") as f:
            f.write("Host essai-un\n  HostName 127.0.0.1\n  Port 1\n")
        dossier = os.path.join(self.perso, ".config", "pupitre")
        os.makedirs(dossier, mode=0o700, exist_ok=True)
        with open(os.path.join(dossier, "connexions.toml"), "w", encoding="utf-8") as f:
            f.write('[[connexion]]\nnom = "serveur-test"\nhote = "127.0.0.1"\n'
                    'utilisateur = "alice"\nport = 2222\n')

    # --- processus -------------------------------------------------------------

    def lancer(self, nom, argv):
        out = open(os.path.join(self.sortie, f"{nom}.out"), "wb")
        p = subprocess.Popen(argv, env=self.env, stdout=out, stderr=subprocess.STDOUT,
                             start_new_session=True)
        self.procs.append(p)
        return p

    def demarrer(self):
        self.lancer("xfwm4", ["xfwm4", "--compositor=off"])
        time.sleep(1)
        self.lancer("orca", ["python3", os.path.join(ICI, "lancer_orca.py"),
                             "-u", os.path.join(self.perso, "orca"),
                             "--debug-file", self.journal])
        if not self.attendre(r"Lecteur d.écran activé|Screen reader on", 60):
            raise Echec("Orca n'a pas démarré (voir orca.out)")
        self.pupitre_proc = self.lancer("pupitre", [self.pupitre])

    def arreter(self):
        for p in reversed(self.procs):
            try:
                os.killpg(p.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
        # Le processus de fond se détache : on le retrouve par son dossier d'exécution.
        for pid in os.listdir("/proc"):
            if not pid.isdigit():
                continue
            # Le fond, ses shells et son veilleur ont le dossier dans leur environnement ;
            # le ssh-agent de Pupitre (setgid : environnement illisible) l'a dans sa ligne
            # de commande (-a <socket>).
            for nom in ("environ", "cmdline"):
                try:
                    with open(f"/proc/{pid}/{nom}", "rb") as f:
                        trouve = self.perso.encode() in f.read()
                    if trouve:
                        os.kill(int(pid), signal.SIGTERM)
                        break
                except (OSError, ProcessLookupError):
                    pass
        time.sleep(0.5)
        shutil.rmtree(self.perso, ignore_errors=True)

    def vivant(self):
        return self.pupitre_proc.poll() is None

    # --- journal d'Orca ---------------------------------------------------------

    def _texte(self):
        try:
            with open(self.journal, encoding="utf-8", errors="replace") as f:
                return f.read()
        except FileNotFoundError:
            return ""

    def marquer(self):
        """Oublie tout ce qu'Orca a dit jusqu'ici."""
        self.lu = len(self._texte())

    def paroles(self):
        """Phrases dites depuis la dernière marque."""
        return [m.group(1) for m in PHRASE.finditer(self._texte()[self.lu:])]

    def attendre(self, motif, delai=8.0):
        """Vrai dès qu'une phrase dite depuis la marque contient `motif` (regex)."""
        r = re.compile(motif, re.S | re.I)
        limite = time.time() + delai
        while time.time() < limite:
            if any(r.search(p) for p in self.paroles()):
                return True
            time.sleep(0.2)
        return False

    def calme(self, duree=1.0, delai=10.0):
        """Attend qu'Orca n'ait rien dit pendant `duree` secondes."""
        limite = time.time() + delai
        n = len(self._texte())
        t = time.time()
        while time.time() < limite:
            time.sleep(0.2)
            m = len(self._texte())
            if m != n:
                n, t = m, time.time()
            elif time.time() - t >= duree:
                return

    # --- clavier --------------------------------------------------------------------

    def touches(self, *noms):
        for n in noms:
            subprocess.run(["xdotool", "key", "--clearmodifiers", n], env=self.env, check=True)
            time.sleep(0.3)

    def taper(self, texte):
        subprocess.run(["xdotool", "type", "--delay", "60", texte], env=self.env, check=True)
        time.sleep(0.2)

    def commande(self, texte):
        self.taper(texte)
        self.touches("Return")

    def orca(self, touche):
        """Commande Orca (disposition « ordinateur de bureau », touche Orca = Inser)."""
        subprocess.run(["xdotool", "keydown", "Insert", "key", touche, "keyup", "Insert"],
                       env=self.env, check=True)
        time.sleep(0.3)

    def fichier(self, nom):
        try:
            with open(os.path.join(self.perso, nom), encoding="utf-8") as f:
                return f.read()
        except FileNotFoundError:
            return None

    def attendre_fichier(self, nom, delai=5.0):
        limite = time.time() + delai
        while time.time() < limite:
            t = self.fichier(nom)
            if t:
                return t
            time.sleep(0.2)
        return None

    def ctrl_tab_jusqu_a(self, motif, max_essais=6):
        """Ctrl+Tab jusqu'à ce qu'Orca dise `motif`."""
        for _ in range(max_essais):
            self.marquer()
            self.touches("ctrl+Tab")
            if self.attendre(motif, 3):
                return True
        return False

    def revenir_au_terminal(self):
        """Remet le clavier dans le terminal, sur une ligne de commande vide."""
        self.touches("Escape")  # ferme un menu ou un dialogue éventuel
        self.ctrl_tab_jusqu_a(r"^Terminal Page \d", 6)
        # Ctrl+C : interrompt une commande et annule le préfixe Méta laissé par Échap
        # (sinon Échap puis « e » ferait Alt+E dans le shell et mangerait la lettre).
        self.touches("ctrl+c")
        self.calme()


# --- vérifications (numéros de docs/orca-liste-de-controle.md) ------------------------

def exiger(ok, quoi):
    if not ok:
        raise Echec(quoi)


def v01_lancement(s):
    # Avec l'intégration du shell, le titre est le dossier courant : « <dossier> — Pupitre ».
    exiger(s.attendre(r"pupitre-orca-\S* — Pupitre", 15),
           "la fenêtre « <dossier> — Pupitre » n'est pas annoncée")
    exiger(s.attendre(re.escape(INVITE.strip()), 5), "l'invite du shell n'est pas lue")


def v02_sortie(s):
    # Accents écrits en octal : xdotool ne sait taper « é » qu'en remappant une touche,
    # ce qui fait planter Orca 48 (keyval_name vide).
    s.commande("printf 'ligne \\303\\251t\\303\\251 \\303\\240 \\303\\247a\\nfin\\n'")
    s.calme()
    s.marquer()
    # Revue à plat : ligne précédente (pavé 7) jusqu'à la ligne accentuée.
    for _ in range(4):
        s.touches("KP_Home")
        if s.attendre(r"ligne été à ça", 2):
            return
    raise Echec("la sortie accentuée n'est pas lue en revue ligne par ligne")


def v03_nouvel_onglet(s):
    s.touches("ctrl+shift+t")
    exiger(s.attendre(r"Page 2|onglet", 6), "le nouvel onglet n'est pas annoncé")
    exiger(s.attendre(re.escape(INVITE.strip()), 6), "l'invite du nouvel onglet n'est pas lue")


def v04_alt_chiffres(s):
    s.touches("alt+1")
    exiger(s.attendre(r"Page 1", 5), "Alt+1 : onglet 1 non annoncé")
    s.calme()
    s.marquer()
    s.touches("alt+2")
    exiger(s.attendre(r"Page 2", 5), "Alt+2 : onglet 2 non annoncé")


def v05_titre(s):
    s.touches("ctrl+shift+t")  # au moins deux onglets, quel que soit l'ordre des essais
    s.calme()
    s.commande("printf '\\e]0;essai\\a'")
    s.calme()
    exiger(s.ctrl_tab_jusqu_a(r"essai"), "le titre « essai » n'est pas lu sur les onglets")
    s.calme()
    s.marquer()
    s.touches("Left")
    # L'autre onglet porte le dossier courant (titre donné par l'intégration du shell).
    exiger(s.attendre(r"pupitre-orca-", 4), "flèche gauche : l'autre onglet n'est pas lu")
    s.marquer()
    s.touches("Right")
    exiger(s.attendre(r"essai", 4), "flèche droite : l'onglet « essai » n'est pas lu")
    s.revenir_au_terminal()


def v06_menu(s):
    exiger(s.ctrl_tab_jusqu_a(r"Menu"), "le bouton « Menu » n'est pas atteint par Ctrl+Tab")
    exiger(s.attendre(r"bouton", 2), "le rôle du bouton Menu n'est pas dit")
    s.calme()
    s.marquer()
    s.touches("Return")
    exiger(s.attendre(r"Nouvel onglet|Nouvelle fenêtre|Copier|Coller", 5),
           "les entrées du menu ne sont pas lues")
    s.marquer()
    s.touches("Down")
    exiger(s.attendre(r"\w", 3), "flèche bas dans le menu : rien n'est lu")
    s.touches("Escape")
    s.calme()
    exiger(s.ctrl_tab_jusqu_a(r"^Terminal Page \d"), "Ctrl+Tab ne ramène pas au terminal")


def v07_dialogue(s):
    s.commande("sleep 30")
    s.calme()
    s.marquer()
    s.touches("ctrl+shift+w")
    exiger(s.attendre(r"Fermer quand même", 6), "la question du dialogue n'est pas lue")
    exiger(s.attendre(r"Annuler", 3), "le bouton « Annuler » n'est pas lu")
    s.touches("Escape")
    s.calme()
    s.touches("ctrl+c")
    exiger(s.vivant(), "Pupitre s'est fermé")


def v08_shell_termine(s):
    s.calme()
    s.marquer()
    s.commande("exit")
    if not s.attendre(r"s.est terminé", 4):
        s.marquer()
        s.touches("KP_Up")  # pavé 8 : lire la ligne courante
        exiger(s.attendre(r"s.est terminé", 4),
               "le message « Le shell s'est terminé » n'est pas lu")
    s.calme()
    s.marquer()
    s.touches("Return")
    exiger(s.attendre(re.escape(INVITE.strip()), 6), "Entrée : la nouvelle invite n'est pas lue")


def v09_config(s):
    chemin = os.path.join(s.env["XDG_CONFIG_HOME"], "pupitre", "pupitre.toml")
    exiger(os.path.exists(chemin), f"{chemin} absent")
    with open(chemin, encoding="utf-8") as f:
        origine = f.read()
    exiger("historique_lignes = 10000" in origine, "clé historique_lignes introuvable")
    s.calme()
    s.marquer()
    with open(chemin, "w", encoding="utf-8") as f:
        f.write(origine.replace("historique_lignes = 10000", 'historique_lignes = "x"'))
    ok = s.attendre(r"Configuration invalide.*ligne", 8)
    with open(chemin, "w", encoding="utf-8") as f:
        f.write(origine)
    time.sleep(2)
    exiger(s.vivant(), "Pupitre s'est fermé")
    exiger(ok, "« Configuration invalide, ligne … » n'est pas annoncé")


def v10_recherche(s):
    s.revenir_au_terminal()
    s.commande("echo motcherche")
    s.calme()
    s.marquer()
    s.touches("ctrl+shift+f")
    exiger(s.attendre(r"Rechercher dans l.historique", 5), "la saisie de recherche n'est pas annoncée")
    s.taper("motcherche")
    s.touches("Return")
    s.calme()
    s.marquer()
    s.touches("ctrl+a")
    s.taper("zzqqxx")
    s.touches("Return")
    exiger(s.attendre(r"Aucun résultat", 5), "« Aucun résultat » n'est pas annoncé")
    s.calme()
    s.touches("Escape")
    s.calme()
    s.marquer()
    s.touches("KP_Up")
    exiger(s.attendre(re.escape(INVITE.strip()), 4),
           "après Échap, le clavier n'est pas revenu au terminal (invite non lue)")


def v11_zoom(s):
    s.commande("tput cols > ~/avant")
    avant = s.attendre_fichier("avant")
    s.touches(*["ctrl+plus"] * 10)
    time.sleep(1)
    s.commande("tput cols > ~/apres")
    apres = s.attendre_fichier("apres")
    s.touches("ctrl+0")
    time.sleep(1)
    s.commande("tput cols > ~/retour")
    retour = s.attendre_fichier("retour")
    exiger(avant and apres and retour, "tput n'a rien écrit")
    a, b, c = int(avant), int(apres), int(retour)
    exiger(b < a * 0.7, f"zoom 200 % : {a} colonnes avant, {b} après (grille non réduite)")
    exiger(c == a, f"Ctrl+0 : {c} colonnes au lieu de {a}")


def v12_cloche(s):
    s.touches("ctrl+shift+t")  # au moins deux onglets
    s.calme()
    s.touches("alt+1")
    s.calme()
    s.commande("sleep 2; printf '\\a'")
    s.touches("alt+2")
    time.sleep(3.5)
    s.calme()
    exiger(s.ctrl_tab_jusqu_a(r"cloche"), "l'onglet n'est pas lu « …, cloche »")
    s.revenir_au_terminal()


def v13_f10(s):
    s.commande("cat -v > ~/f10")
    time.sleep(0.5)
    s.marquer()
    s.touches("F10", "Return", "ctrl+d")
    texte = s.attendre_fichier("f10")
    exiger(texte and "[21~" in texte, "F10 n'est pas arrivé au programme")
    exiger(not any(re.search(r"\bmenu\b", p, re.I) for p in s.paroles()),
           "F10 a ouvert le menu de Pupitre")


def v14_commandes_orca(s):
    # Attentes d'un état, jamais un délai fixe : Orca+H est reçu quand Orca a fini de lire
    # ce qui précède (machine chargée), et la revue à plat suit le curseur du terminal.
    for essai in range(3):
        s.calme()
        s.marquer()
        s.orca("h")
        if s.attendre(r"apprentissage|learn", 6):
            break
    else:
        raise Echec("Orca+H : le mode apprentissage ne s'ouvre pas")
    s.touches("Escape")
    s.calme(1.5)
    for essai in range(3):
        s.marquer()
        s.touches("KP_Up")  # pavé 8 : lire la ligne courante
        if s.attendre(re.escape(INVITE.strip()), 4):
            return
        s.calme()
    raise Echec("Orca : lecture de la ligne courante sans effet")


def v15_phrase_de_passe(s):
    s.revenir_au_terminal()
    s.marquer()
    s.commande("ssh-add ~/.ssh/id_test")
    exiger(s.attendre(r"Phrase de passe SSH", 10), "la fenêtre « Phrase de passe SSH » n'est pas annoncée")
    exiger(s.attendre(r"bo[iî]te de dialogue|dialog", 3), "le rôle « boîte de dialogue » n'est pas dit")
    exiger(s.attendre(r"Phrase de passe de la clé .*id_test", 4),
           "la question avec le chemin de la clé n'est pas lue")
    exiger(s.attendre(r"mot de passe|password", 3), "la saisie n'est pas annoncée « saisie de mot de passe »")
    s.calme()
    s.marquer()
    s.touches("Tab")
    exiger(s.attendre(r"Mémoriser dans le trousseau", 4), "la case « Mémoriser dans le trousseau » n'est pas lue")
    exiger(s.attendre(r"case à cocher|check", 2), "le rôle « case à cocher » n'est pas dit")
    exiger(s.attendre(r"coch[ée]e|checked", 2) and not s.attendre(r"non coch", 0.5),
           "l'état « cochée » n'est pas dit")
    s.calme()
    s.marquer()
    s.touches("Escape")  # vraie touche Échap : seul test qui la fait arriver au dialogue
    s.calme(1.5)
    # ssh-add a fini (sinon la commande suivante resterait en attente) et le terminal a le clavier.
    s.marquer()
    s.commande("echo fin-$((6*7))")
    exiger(s.attendre(r"fin-42", 8), "après Échap, ssh-add n'a pas fini ou le terminal n'a pas le clavier")
    exiger(not any(re.search(r"Phrase de passe SSH", p) for p in s.paroles()),
           "une seconde fenêtre de phrase de passe est apparue après Échap")


def v16_connexions(s):
    s.revenir_au_terminal()
    s.marquer()
    s.touches("ctrl+shift+o")
    exiger(s.attendre(r"Connexions SSH", 6), "la fenêtre « Connexions SSH » n'est pas annoncée")
    exiger(s.attendre(r"bo[iî]te de dialogue|dialog", 3), "le rôle « boîte de dialogue » n'est pas dit")
    exiger(s.attendre(r"serveur-test.*alice@127\.0\.0\.1:2222", 4), "la première ligne n'est pas lue")
    s.calme()
    s.marquer()
    s.touches("Down")
    exiger(s.attendre(r"essai-un.*ssh/config", 4), "flèche bas : la ligne suivante n'est pas lue")
    # Tab : les boutons, jusqu'à « Ajouter… » ; Entrée ouvre le formulaire.
    s.calme()
    s.marquer()
    s.touches("Tab", "Tab")
    exiger(s.attendre(r"Ajouter", 4), "Tab : « Ajouter… » n'est pas lu")
    s.calme()
    s.marquer()
    s.touches("Return")
    # Orca ignore l'activation du formulaire (juste après celle de la liste) : le titre est
    # annoncé par Pupitre lui-même, après le premier champ.
    exiger(s.attendre(r"Nom texte", 5), "le premier champ « Nom » n'est pas lu")
    exiger(s.attendre(r"Ajouter une connexion", 3), "le titre du formulaire d'ajout n'est pas annoncé")
    s.calme()
    s.marquer()
    s.touches("Tab", "Tab", "Tab", "Tab")
    exiger(s.attendre(r"^Clé liste déroulante", 4),
           "Tab jusqu'à la liste déroulante : « Clé liste déroulante » n'est pas lu")
    s.calme()
    s.marquer()
    s.touches("Escape")  # ferme le formulaire
    exiger(s.attendre(r"Connexions SSH", 4),
           "Échap dans le formulaire : le retour à la liste « Connexions SSH » n'est pas annoncé")
    s.calme()
    s.marquer()
    s.touches("Escape")  # ferme la liste
    s.calme()
    s.marquer()
    s.commande("echo ok-$((6*7))")
    exiger(s.attendre(r"ok-42", 6), "après Échap, le terminal n'a pas le clavier")


def v17_fin_de_commande(s):
    # Intégration du shell active (marques OSC 133) : la fin d'une commande de 6 s est annoncée.
    s.revenir_au_terminal()
    s.marquer()
    s.commande("sleep 6")
    exiger(s.attendre(r"^Commande terminée en 6 s\.?$", 14),
           "« Commande terminée en 6 s. » n'est pas annoncé après sleep 6")


def v18_navigation(s):
    s.revenir_au_terminal()
    s.commande("echo un")
    s.calme()
    s.commande("echo deux")
    s.calme(1.5)
    s.marquer()
    s.touches("ctrl+shift+Up")
    exiger(s.attendre(r"^Commande : echo deux$", 6),
           "Ctrl+Maj+↑ : « Commande : echo deux » n'est pas dit")
    s.calme()
    s.marquer()
    s.touches("ctrl+shift+Up")
    exiger(s.attendre(r"^Commande : echo un$", 6),
           "Ctrl+Maj+↑ encore : « Commande : echo un » n'est pas dit")


def v20_invite_starship(s):
    # Invite Starship par défaut : on retire « invite = "shell" » ; la configuration est relue
    # à chaud, et le nouvel onglet crée starship.toml dans le dossier de Pupitre. Le HOME est
    # celui, jetable, du test : jamais le ~/.config/starship.toml de l'utilisateur.
    chemin = os.path.join(s.env["XDG_CONFIG_HOME"], "pupitre", "pupitre.toml")
    with open(chemin, encoding="utf-8") as f:
        origine = f.read()
    with open(chemin, "w", encoding="utf-8") as f:
        f.write(origine.replace('invite = "shell"\n', ""))
    try:
        time.sleep(2)  # relecture à chaud de la configuration
        s.revenir_au_terminal()
        s.marquer()
        s.touches("ctrl+shift+t")
        # Après un nouvel onglet, Orca a le focus sur l'onglet : Ctrl+Tab le ramène au terminal
        # (comme v05 et v12). Ctrl+C redessine l'invite, que le terminal annonce : deux lignes
        # dont celle du symbole « λ ». On attend l'invite, pas un délai.
        s.calme(1.0, 10)
        exiger(s.ctrl_tab_jusqu_a(r"^Terminal Page \d"), "Ctrl+Tab ne ramène pas au terminal")
        s.calme()
        s.marquer()
        s.touches("ctrl+c")
        exiger(s.attendre(r"λ", 10), "Orca ne lit pas une ligne d'invite contenant « λ »")
        exiger(os.path.exists(os.path.join(s.env["XDG_CONFIG_HOME"], "pupitre", "starship.toml")),
               "Pupitre n'a pas créé son starship.toml")
        s.calme()
        s.marquer()
        s.commande("sleep 6")
        exiger(s.attendre(r"^Commande terminée en 6 s\.?$", 14),
               "avec l'invite Starship, « Commande terminée en 6 s. » n'est pas annoncé")
    finally:
        with open(chemin, "w", encoding="utf-8") as f:
            f.write(origine)


def v19_aide(s):
    s.revenir_au_terminal()
    s.marquer()
    s.touches("ctrl+shift+h")
    exiger(s.attendre(r"Aide de Pupitre", 6), "la fenêtre « Aide de Pupitre » n'est pas annoncée")
    exiger(s.attendre(r"bo[iî]te de dialogue|dialog", 3), "le rôle « boîte de dialogue » n'est pas dit")
    # Le focus est sur la liste des sections : la liste est nommée, puis la première
    # section est lue (« Aide de Pupitre »), après l'annonce de la fenêtre.
    exiger(s.attendre(r"Sections de l'aide.*(liste|list)", 4), "la liste « Sections de l'aide » n'est pas annoncée")
    exiger(s.attendre(r"^Aide de Pupitre\.?$", 4), "la première section « Aide de Pupitre » n'est pas lue")
    dites = s.paroles()
    fenetre = next(i for i, p in enumerate(dites) if re.search(r"Aide de Pupitre.*(bo[iî]te de dialogue|dialog)", p))
    exiger(any(re.fullmatch(r"Aide de Pupitre\.?", p) for p in dites[fenetre + 1:]),
           "le nom de la première section n'est pas dit après celui de la fenêtre")
    s.calme()
    s.marquer()
    s.touches("Down")
    exiger(s.attendre(r"Sommaire", 4), "flèche bas : la section « Sommaire » n'est pas lue")
    # Ctrl+Tab : de la liste au texte de la section, lu ligne par ligne.
    s.calme()
    s.marquer()
    s.touches("ctrl+Tab")
    exiger(s.attendre(r"Sommaire|Démarrer", 4), "Ctrl+Tab : le texte de la section n'est pas lu")
    s.calme()
    s.marquer()
    s.touches("Escape")
    s.calme(1.5)
    s.marquer()
    s.commande("echo aide-$((6*7))")
    exiger(s.attendre(r"aide-42", 8), "après Échap, le terminal n'a pas le clavier")


VERIFICATIONS = [
    (1, "Lancement : fenêtre et invite annoncées", v01_lancement),
    (2, "Sortie d'une commande lue ligne par ligne, accents compris", v02_sortie),
    (3, "Ctrl+Maj+T : nouvel onglet annoncé, invite lue", v03_nouvel_onglet),
    (4, "Alt+1 / Alt+2 : onglet annoncé", v04_alt_chiffres),
    (5, "Titre d'onglet « essai » lu ; flèches entre onglets", v05_titre),
    (6, "Menu au clavier : bouton, entrées, Échap", v06_menu),
    (7, "Dialogue de fermeture lu, Échap l'annule", v07_dialogue),
    (8, "Shell terminé : message lu, Entrée relance", v08_shell_termine),
    (9, "Configuration invalide annoncée, Pupitre reste ouvert", v09_config),
    (10, "Recherche : saisie annoncée, « Aucun résultat », Échap", v10_recherche),
    (11, "Zoom 200 % : grille réduite, Ctrl+0 revient", v11_zoom),
    (12, "Cloche : onglet lu « …, cloche »", v12_cloche),
    (13, "F10 va au programme, pas au menu", v13_f10),
    (14, "Commandes d'Orca non captées par Pupitre", v14_commandes_orca),
    (15, "ssh-add : fenêtre de phrase de passe lue, case, Échap", v15_phrase_de_passe),
    (16, "Connexions SSH : liste lue, formulaire, Échap", v16_connexions),
    (17, "Fin de commande annoncée après sleep 6", v17_fin_de_commande),
    (18, "Ctrl+Maj+↑ : « Commande : echo deux » puis « echo un »", v18_navigation),
    (19, "Aide : Ctrl+Maj+H, sections lues, Ctrl+Tab, Échap", v19_aide),
    (20, "Invite Starship par défaut : ligne contenant λ, fin de commande annoncée", v20_invite_starship),
]


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--pupitre", required=True)
    p.add_argument("--fond", required=True)
    p.add_argument("--sortie", required=True)
    p.add_argument("--seulement", type=int, nargs="*", help="numéros à faire (défaut : tous)")
    a = p.parse_args()
    os.makedirs(a.sortie, exist_ok=True)
    for outil in ("xdotool", "xfwm4", "orca"):
        if shutil.which(outil) is None:
            print(f"{outil} manquant", file=sys.stderr)
            return 2

    s = Session(os.path.abspath(a.pupitre), os.path.abspath(a.fond), a.sortie)
    lignes = []
    echecs = 0
    try:
        s.demarrer()
        s.marquer()
        for num, titre, f in VERIFICATIONS:
            if a.seulement and num not in a.seulement and num != 1:
                continue
            if not s.vivant():
                lignes.append(f"{num:2}  ÉCHEC  {titre} — Pupitre ne tourne plus")
                echecs += 1
                continue
            try:
                f(s)
                lignes.append(f"{num:2}  OK     {titre}")
            except (Echec, subprocess.CalledProcessError, ValueError) as e:
                echecs += 1
                dit = " | ".join(x.replace("\n", "⏎") for x in s.paroles()[-6:])
                lignes.append(f"{num:2}  ÉCHEC  {titre} — {e}\n      Orca a dit : {dit[:400]}")
                s.revenir_au_terminal()
            s.calme()
            s.marquer()
    except Echec as e:
        lignes.append(f"    ÉCHEC  préparation — {e}")
        echecs += 1
    finally:
        s.arreter()
    rapport = "\n".join(lignes) + f"\n\n{len(lignes) - echecs} réussie(s), {echecs} échec(s)\n"
    with open(os.path.join(a.sortie, "rapport.txt"), "w", encoding="utf-8") as f:
        f.write(rapport)
    print(rapport)
    return 1 if echecs else 0


sys.exit(main())
