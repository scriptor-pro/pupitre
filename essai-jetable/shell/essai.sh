#!/bin/sh
# Essai jetable (étape 3, Task 0) : refait toutes les mesures de RESULTATS.md.
# Tout se passe dans des HOME temporaires (supprimés à la fin) ; le vrai HOME n'est jamais lu.
exec python3 -I "$(dirname "$0")/essai.py" "$@"
