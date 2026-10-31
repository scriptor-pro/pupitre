# ESSAI JETABLE — parcourt l'arbre AT-SPI de l'essai et indique si le texte du terminal est exposé.
import gi; gi.require_version("Atspi", "2.0")
from gi.repository import Atspi
bureau = Atspi.get_desktop(0); trouve = False
def parcours(n, prof=0):
    global trouve
    try: role = n.get_role_name(); nom = n.get_name()
    except Exception: return
    txt = ""
    try:
        t = n.get_text_iface()
        if t: txt = repr(Atspi.Text.get_text(n, 0, 80))
    except Exception: pass
    print(f"  a11y {'  '*prof}{role} «{nom}» texte={txt or 'aucune interface Text'}")
    if txt and "lien" in txt or "Dégradé" in txt: trouve = True
    for i in range(n.get_child_count()):
        if prof < 8: parcours(n.get_child_at_index(i), prof + 1)
for i in range(bureau.get_child_count()):
    a = bureau.get_child_at_index(i)
    if a and a.get_name() and "essai" in a.get_name().lower():
        print("application AT-SPI trouvée :", a.get_name()); parcours(a)
print("texte du terminal lisible par un lecteur d'écran :", "OUI" if trouve else "NON")
