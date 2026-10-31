# ESSAI JETABLE — cherche le widget terminal de l'essai dans l'arbre AT-SPI et lit son texte.
import gi; gi.require_version("Atspi", "2.0")
from gi.repository import Atspi
bureau = Atspi.get_desktop(0); verdict = "NON"
def parcours(n, prof=0):
    global verdict
    try: role = n.get_role_name(); nom = n.get_name()
    except Exception: return
    txt = ""
    try:
        if n.get_text_iface(): txt = Atspi.Text.get_text(n, 0, -1)
    except Exception: pass
    print(f"{'  '*prof}{role} «{nom}» texte={txt[:120]!r}")
    if role == "terminal" and "MARQUEUR-A11Y" in txt: verdict = "OUI"
    for i in range(n.get_child_count()):
        if prof < 10: parcours(n.get_child_at_index(i), prof + 1)
for i in range(bureau.get_child_count()):
    a = bureau.get_child_at_index(i)
    if a and a.get_name() and "essai" in a.get_name().lower(): print("application :", a.get_name()); parcours(a)
print("VERDICT texte du terminal lisible (rôle terminal) :", verdict)
