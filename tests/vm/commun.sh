# SPDX-License-Identifier: GPL-3.0-or-later
# Déroulé commun des essais en VM, chargé par tests/vm/essai-<bureau>.sh après qu'il a
# démarré le bureau et défini : taper TEXTE (puis Entrée), ecrire TEXTE, touches COMBO…
# (ex. ctrl+shift+t, escape), fenetres (« id — titre — focus=… »), capture FICHIER,
# presse_papiers TEXTE. Chaque essai écrit son résultat dans un fichier ; le script finit
# par « BILAN : n réussi(s), m échec(s) » et un code de sortie non nul en cas d'échec.

R=/tmp/essai
REUSSIS=0
ECHECS=0
verdict() { # nom, attendu (motif grep -E), fichier
  if grep -qE "$2" "$3" 2>/dev/null; then
    echo "  ok      $1"; REUSSIS=$((REUSSIS + 1))
  else
    echo "  ÉCHEC   $1 (attendu /$2/, obtenu « $(head -c 200 "$3" 2>/dev/null) »)"; ECHECS=$((ECHECS + 1))
  fi
}

lancer_pupitre() {
  systemctl --user stop pupitre 2>/dev/null
  systemctl --user reset-failed pupitre 2>/dev/null
  pkill -u "$(id -u)" -f '[p]upitre-fond' 2>/dev/null
  rm -f ~/.ssh/id_test ~/.ssh/id_test.pub
  systemd-run --user -q --unit=pupitre --setenv=WAYLAND_DISPLAY="$WAYLAND_DISPLAY" \
    --setenv=PUPITRE_ASSETS="$PUPITRE_ASSETS" --setenv=LANG="$LANG" ~/pupitre/bin/pupitre
  sleep 6
}

derouler() {
  fenetres > "$R/ouverture"
  verdict "fenêtre ouverte, focalisée, identifiant fr.pupitre.Pupitre" '^fr\.pupitre\.Pupitre — .* — focus=true' "$R/ouverture"
  capture "$R/1-ouverture.png"

  taper "echo ok-42 > $R/saisie; tty > $R/tty1"
  verdict "saisie et Entrée" '^ok-42$' "$R/saisie"

  touches ctrl+shift+t
  taper "tty > $R/tty2"
  if [ -s "$R/tty2" ] && ! cmp -s "$R/tty1" "$R/tty2"; then echo autre > "$R/onglet"; fi
  verdict "Ctrl+Maj+T : nouvel onglet (autre terminal)" '^autre$' "$R/onglet"

  presse_papiers 'texte-colle-42'
  ecrire "echo "; touches ctrl+shift+v; taper " > $R/colle"
  verdict "Ctrl+Maj+V : collage du presse-papiers" '^texte-colle-42$' "$R/colle"

  touches ctrl+prior
  taper "tty > $R/tty-retour"
  verdict "Ctrl+PgPréc : retour au premier onglet" "^$(cat "$R/tty1" 2>/dev/null)\$" "$R/tty-retour"

  # Fenêtre de phrase de passe : elle doit recevoir le clavier sous Wayland.
  taper "ssh-keygen -q -t ed25519 -N phrase-test -f ~/.ssh/id_test; ssh-add ~/.ssh/id_test; ssh-add -l > $R/agent"
  sleep 4
  fenetres > "$R/demande"
  capture "$R/2-demande-ssh.png"
  verdict "fenêtre de phrase de passe focalisée" '^fr\.pupitre\.Pupitre — Phrase de passe SSH — focus=true' "$R/demande"
  taper "phrase-test"
  sleep 3
  verdict "phrase acceptée, clé chargée dans l'agent" 'ED25519' "$R/agent"
  fenetres > "$R/apres-demande"
  verdict "clavier rendu au terminal" '^fr\.pupitre\.Pupitre — .* — Pupitre — focus=true' "$R/apres-demande"

  touches ctrl+shift+o
  sleep 1.5
  fenetres > "$R/connexions"
  capture "$R/3-connexions.png"
  verdict "Ctrl+Maj+O : Connexions SSH focalisée" '^fr\.pupitre\.Pupitre — Connexions SSH — focus=true' "$R/connexions"
  touches escape
  sleep 1.5
  fenetres > "$R/apres-connexions"
  if grep -q "Connexions SSH" "$R/apres-connexions"; then echo encore > "$R/ferme"; else echo fermee > "$R/ferme"; fi
  verdict "Échap ferme Connexions SSH" '^fermee$' "$R/ferme"

  # Accessibilité : ce qu'Orca lirait.
  python3 - > "$R/atspi" 2>&1 <<'PY'
import pyatspi
def voir(n, p=0, prof=0):
    if n is None or prof > 8:
        return
    try:
        r, nom = n.getRoleName(), n.name
    except Exception:
        return
    if r not in ("panel", "filler") or nom:
        print("  " * p + f"{r} « {nom} »")
        p += 1
    for i in range(min(n.childCount, 20)):
        voir(n.getChildAtIndex(i), p, prof + 1)
for app in pyatspi.Registry.getDesktop(0):
    if app and app.name and "upitre" in app.name:
        voir(app)
PY
  verdict "accessibilité : liste d'onglets exposée" 'page tab list' "$R/atspi"
  verdict "accessibilité : terminal exposé" '^ *terminal « ' "$R/atspi"
  verdict "accessibilité : bouton Menu nommé" 'button « Menu »' "$R/atspi"

  systemctl --user stop pupitre
  echo "BILAN : $REUSSIS réussi(s), $ECHECS échec(s)"
  [ "$ECHECS" -eq 0 ]
}
