# SPDX-License-Identifier: GPL-3.0-or-later
"""Sonde AT-SPI de Pupitre : lit l'arbre d'accessibilité d'une application.

Usage :
  python3 sonde.py --app NOM                     affiche l'arbre, une ligne par élément :
                                                 rôle<TAB>nom<TAB>texte<TAB>états
  python3 sonde.py --app NOM --attendu F.json    vérifie l'arbre ; code 0 si conforme

NOM : nom exact de l'application (casse ignorée). Code 1 si l'application est absente
ou l'arbre non conforme.

Fichier d'attentes : liste de règles, chacune avec :
  "role"   rôle AT-SPI (ex. "terminal", "page tab", "push button")       obligatoire
  "nom"    "texte exact", "~contient", "!vide" (non vide)                facultatif
  "texte"  mêmes formes, sur le texte de l'élément                       facultatif
  "etat"   état AT-SPI requis (ex. "focused", "showing")                 facultatif
  "tous"   true : chaque élément de ce rôle doit respecter la règle      défaut false
  "min"    nombre minimal d'éléments conformes                           défaut 1
"""
import argparse
import json
import sys

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi  # noqa: E402

PROFONDEUR_MAX = 30


def etats(noeud):
    try:
        ens = noeud.get_state_set()
        return [Atspi.StateType(s).value_nick for s in ens.get_states()]
    except Exception:
        return []


def elements(noeud, prof=0):
    try:
        role = noeud.get_role_name()
        nom = noeud.get_name() or ""
    except Exception:
        return
    texte = ""
    try:
        if noeud.get_text_iface() is not None:
            texte = Atspi.Text.get_text(noeud, 0, -1) or ""
    except Exception:
        pass
    yield {"role": role, "nom": nom, "texte": texte, "etats": etats(noeud)}
    if prof >= PROFONDEUR_MAX:
        return
    for i in range(noeud.get_child_count()):
        enfant = noeud.get_child_at_index(i)
        if enfant is not None:
            yield from elements(enfant, prof + 1)


def application(nom):
    bureau = Atspi.get_desktop(0)
    for i in range(bureau.get_child_count()):
        appli = bureau.get_child_at_index(i)
        if appli is not None and (appli.get_name() or "").lower() == nom.lower():
            return appli
    return None


def correspond(valeur, attendu):
    if attendu is None:
        return True
    if attendu == "!vide":
        return valeur.strip() != ""
    if attendu.startswith("~"):
        return attendu[1:] in valeur
    return valeur == attendu


def conforme(e, regle):
    return (
        correspond(e["nom"], regle.get("nom"))
        and correspond(e["texte"], regle.get("texte"))
        and (regle.get("etat") is None or regle["etat"] in e["etats"])
    )


def verifier(liste, regles):
    erreurs = []
    for r in regles:
        du_role = [e for e in liste if e["role"] == r["role"]]
        bons = [e for e in du_role if conforme(e, r)]
        if r.get("tous"):
            mauvais = [e for e in du_role if not conforme(e, r)]
            for e in mauvais:
                erreurs.append(f"{r['role']} non conforme à {r} : nom={e['nom']!r}")
        if len(bons) < r.get("min", 1):
            erreurs.append(
                f"règle {r} : {len(bons)} élément(s) conforme(s) sur {len(du_role)} "
                f"de rôle {r['role']!r}"
            )
    return erreurs


def ligne(e):
    texte = e["texte"].replace("\\", "\\\\").replace("\n", "\\n")
    return f"{e['role']}\t{e['nom']}\t{texte}\t{','.join(e['etats'])}"


def main():
    p = argparse.ArgumentParser(description="Sonde AT-SPI de Pupitre")
    p.add_argument("--app", required=True)
    p.add_argument("--attendu")
    a = p.parse_args()
    appli = application(a.app)
    if appli is None:
        print(f"application « {a.app} » absente du bus d'accessibilité", file=sys.stderr)
        return 1
    liste = list(elements(appli))
    if a.attendu is None:
        for e in liste:
            print(ligne(e))
        return 0
    with open(a.attendu, encoding="utf-8") as f:
        regles = json.load(f)
    erreurs = verifier(liste, regles)
    for err in erreurs:
        print(f"ÉCART : {err}")
    if erreurs:
        print("--- arbre lu ---")
        for e in liste:
            print(ligne(e))
        return 1
    print(f"arbre conforme ({len(regles)} règles, {len(liste)} éléments)")
    return 0


sys.exit(main())
