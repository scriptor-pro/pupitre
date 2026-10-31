#!/bin/bash
# Essai jetable : comportement d'OpenSSH 10 avec SSH_ASKPASS. Aucune clé / agent réel.
set -u
D=$(cd "$(dirname "$0")" && pwd)
T=$(mktemp -d); export HOME=$T/home; mkdir -p $HOME
unset DISPLAY WAYLAND_DISPLAY SSH_AUTH_SOCK GIT_ASKPASS SSH_ASKPASS GIT_TERMINAL_PROMPT
export ESPION_LOG=$T/log ESPION_REPONSE=$T/rep ESPION_REPONSES=$T/reps
ssh-keygen -q -t ed25519 -N secret1 -f $T/cle
ssh-agent -D -a $T/agent.sock >/dev/null & AG=$!; sleep 0.5
run() { : > $ESPION_LOG; "$@"; echo "code=$?"; echo "--- log askpass"; cat $ESPION_LOG; }
sub() { SSH_ASKPASS=$D/espion.sh SSH_ASKPASS_REQUIRE=force setsid -w "$@" </dev/null 2>&1; }
export SSH_AUTH_SOCK=$T/agent.sock
echo "### A: ssh-add bonne phrase"; echo secret1 > $T/rep; run sub ssh-add $T/cle
ssh-add -D 2>&1
echo "### B: ssh-add mauvaise puis bonne"; printf 'mauvais\nsecret1\n' > $T/reps; rm -f $T/rep; run sub ssh-add $T/cle
ssh-add -D 2>&1
echo "### C: ssh-add reponse vide"; : > $T/rep; rm -f $T/reps; run sub ssh-add $T/cle
echo "### D: ssh-add -c puis usage (agent lancé AVEC SSH_ASKPASS, sinon il refuse)"; echo secret1 > $T/rep
kill $AG; SSH_ASKPASS=$D/espion.sh SSH_ASKPASS_REQUIRE=force ssh-agent -D -a $T/agent2.sock >/dev/null 2>$T/agent2.err & AG=$!; sleep 0.5
export SSH_AUTH_SOCK=$T/agent2.sock
run sub ssh-add -c $T/cle
: > $ESPION_LOG; echo ok > $T/rep; echo hello > $T/msg
SSH_ASKPASS=$D/espion.sh SSH_ASKPASS_REQUIRE=force setsid -w ssh-keygen -Y sign -f $T/cle.pub -n test -U $T/msg </dev/null 2>&1; echo "code=$?"; cat $ESPION_LOG
echo "### D2: confirm refusé (askpass code 1)"; touch $ESPION_LOG.refus; setsid -w ssh-keygen -Y sign -f $T/cle.pub -n test2 -U $T/msg </dev/null 2>&1; echo "code=$?"
ssh-add -D 2>&1
echo "### E: ssh -i cle port fermé"; echo secret1 > $T/rep
run sub ssh -i $T/cle -o IdentitiesOnly=yes -o UserKnownHostsFile=$T/kh -o ConnectTimeout=3 nimporte@127.0.0.1 -p 1
echo "### E2: ssh localhost:22"; run sub ssh -i $T/cle -o IdentitiesOnly=yes -o UserKnownHostsFile=$T/kh -o ConnectTimeout=3 -p 22 localhost true
echo "### F: ssh-add avec fd 7 non CLOEXEC"; : > $ESPION_LOG; echo secret1 > $T/rep
( exec 7>$T/fd7; export ESPION_FD=7; SSH_ASKPASS=$D/espion.sh SSH_ASKPASS_REQUIRE=force setsid -w sh -c 'ssh-add '$T'/cle </dev/null' 2>&1 ); cat $ESPION_LOG
kill $AG; echo T=$T
