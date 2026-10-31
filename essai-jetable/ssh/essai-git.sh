#!/bin/bash
# git appelle-t-il SSH_ASKPASS pour un mot de passe HTTPS ? (sans GIT_ASKPASS, core.askPass, terminal)
D=$(cd "$(dirname "$0")" && pwd); T=$(mktemp -d); export HOME=$T
unset DISPLAY WAYLAND_DISPLAY GIT_ASKPASS GIT_TERMINAL_PROMPT
export GIT_CONFIG_NOSYSTEM=1 ESPION_LOG=$T/log ESPION_REPONSE=$T/rep; echo x > $T/rep; : > $ESPION_LOG
python3 -I $D/serveur401.py 18401 & S=$!; sleep 0.5
echo "--- git sans terminal, SSH_ASKPASS=espion (REQUIRE=force)"
SSH_ASKPASS=$D/espion.sh SSH_ASKPASS_REQUIRE=force setsid -w git ls-remote http://127.0.0.1:18401/x </dev/null 2>&1; echo "code=$?"; cat $ESPION_LOG
echo "--- idem, sans SSH_ASKPASS_REQUIRE"; : > $ESPION_LOG
SSH_ASKPASS=$D/espion.sh setsid -w git ls-remote http://127.0.0.1:18401/x </dev/null 2>&1; echo "code=$?"; cat $ESPION_LOG
kill $S
