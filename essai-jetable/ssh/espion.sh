#!/bin/sh
# Askpass espion : consigne argv[1], SSH_ASKPASS_PROMPT, PID parent ; répond le contenu de $ESPION_REPONSE
# (si ESPION_REPONSES existe : une réponse par appel, ligne N)
{
  printf 'ARGV1=%s\n' "$1" | cat -A
  printf 'PROMPT=%s\n' "${SSH_ASKPASS_PROMPT-<absent>}"
  printf 'PPID=%s DISPLAY=%s WAYLAND=%s\n' "$PPID" "${DISPLAY-<absent>}" "${WAYLAND_DISPLAY-<absent>}"
  printf 'FD=%s\n' "$(ls -l /proc/$$/fd/${ESPION_FD:-3} 2>&1 | sed 's/.*-> //')"
  echo ---
} >> "$ESPION_LOG"
# confirm : ne rien écrire sur stdout (toute sortie => refus constaté), code 0 = autoriser
if [ "${SSH_ASKPASS_PROMPT-}" = confirm ]; then [ -e "$ESPION_LOG.refus" ] && exit 1; exit 0; fi
n=$(grep -c '^ARGV1' "$ESPION_LOG")
if [ -f "$ESPION_REPONSES" ]; then sed -n "${n}p" "$ESPION_REPONSES"; else cat "$ESPION_REPONSE"; fi
exit 0
