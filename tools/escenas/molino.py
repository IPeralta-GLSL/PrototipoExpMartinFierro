"""
Molino de viento procedural para las escenas (.tscn): torre de patas
inclinadas con travesaños, plataforma, la rueda de aspas (nodo Giratorio, gira
con rachas), la cola veleta, la varilla de la bomba y el bebedero.

  sub, nodos = molino("Molino", "Campamento", (40, 0, 62), -145)

`sub` va antes del primer [node] de la escena y `nodos` al final. La rueda mira
hacia +Z del molino: se lo gira para que quede de frente a quien mira.
"""

import math

ALTO = 9.0
BASE = 1.1      # medio lado de la base
CIMA = 0.28     # medio lado arriba


def molino(nombre, padre, pos, rot, sufijo=None):
    sufijo = sufijo or nombre.lower()
    sub, nodos = [], []
    n = [0]

    def malla(t, mat):
        n[0] += 1
        mid = f"m_{sufijo}_{n[0]}"
        sub.append(f'[sub_resource type="BoxMesh" id="{mid}"]\nmaterial = SubResource("mat_{sufijo}")\n'
                   f"size = Vector3({t[0]:.3f}, {t[1]:.3f}, {t[2]:.3f})\n" if mat == "metal" else
                   f'[sub_resource type="BoxMesh" id="{mid}"]\nmaterial = SubResource("mat_{sufijo}_madera")\n'
                   f"size = Vector3({t[0]:.3f}, {t[1]:.3f}, {t[2]:.3f})\n")
        return mid

    def pieza(nom, p, t, rot_xyz=(0, 0, 0), mat="metal", padre_rel=""):
        mid = malla(t, mat)
        par = f"{padre}/{nombre}" + (f"/{padre_rel}" if padre_rel else "")
        r = "" if rot_xyz == (0, 0, 0) else f"rotation_degrees = Vector3({rot_xyz[0]:.2f}, {rot_xyz[1]:.2f}, {rot_xyz[2]:.2f})\n"
        nodos.append(f'\n[node name="{nom}" type="MeshInstance3D" parent="{par}"]\n'
                     f"position = Vector3({p[0]:.3f}, {p[1]:.3f}, {p[2]:.3f})\n{r}mesh = SubResource(\"{mid}\")\n")

    sub.append(f'[sub_resource type="StandardMaterial3D" id="mat_{sufijo}"]\nalbedo_color = Color(0.37, 0.35, 0.32, 1)\n'
               "metallic = 0.35\nroughness = 0.75\n")
    sub.append(f'[sub_resource type="StandardMaterial3D" id="mat_{sufijo}_madera"]\nalbedo_color = Color(0.3, 0.21, 0.13, 1)\n'
               "roughness = 0.95\n")
    nodos.append(f'\n[node name="{nombre}" type="Node3D" parent="{padre}" groups=["apoyar_en_suelo"]]\n'
                 f"position = Vector3({pos[0]}, {pos[1]}, {pos[2]})\nrotation_degrees = Vector3(0, {rot}, 0)\n")

    # Patas inclinadas (de las esquinas de la base hacia la cima).
    for i, (sx, sz) in enumerate(((1, 1), (-1, 1), (-1, -1), (1, -1))):
        b = (sx * BASE, sz * BASE)
        c = (sx * CIMA, sz * CIMA)
        dx, dz = c[0] - b[0], c[1] - b[1]
        largo = math.sqrt(ALTO ** 2 + dx * dx + dz * dz)
        rx = math.degrees(math.atan2(dz, ALTO))
        rz = -math.degrees(math.atan2(dx, ALTO))
        pieza(f"Pata{i + 1}", ((b[0] + c[0]) / 2, ALTO / 2, (b[1] + c[1]) / 2), (0.09, largo, 0.09), (rx, 0, rz))
    # Travesaños.
    for j, h in enumerate((2.4, 4.9, 7.2)):
        s = BASE + (CIMA - BASE) * h / ALTO
        for k, (p, t) in enumerate((((0, h, s), (2 * s, 0.06, 0.06)), ((0, h, -s), (2 * s, 0.06, 0.06)),
                                    ((s, h, 0), (0.06, 0.06, 2 * s)), ((-s, h, 0), (0.06, 0.06, 2 * s)))):
            pieza(f"Travesano{j + 1}_{k + 1}", p, t)
    pieza("Plataforma", (0, ALTO, 0), (0.85, 0.05, 0.85), mat="madera")
    pieza("Varilla", (0, ALTO / 2 + 0.2, 0), (0.03, ALTO - 0.4, 0.03))
    pieza("Cola", (0, ALTO + 0.35, -1.15), (0.02, 0.7, 1.8))
    pieza("Bebedero", (1.9, 0.22, 1.0), (2.2, 0.45, 0.6), mat="madera")
    # Rueda de aspas (gira con el viento).
    nodos.append(f'\n[node name="Rueda" type="Giratorio" parent="{padre}/{nombre}"]\n'
                 f"position = Vector3(0, {ALTO + 0.35:.2f}, 0.3)\nvelocidad = 0.32\n")
    pieza("Cubo", (0, 0, 0), (0.24, 0.24, 0.24), padre_rel="Rueda")
    aspas = 16
    for i in range(aspas):
        th = 360.0 * i / aspas
        a = math.radians(th)
        pieza(f"Aspa{i + 1}", (-math.sin(a) * 0.85, math.cos(a) * 0.85, 0), (0.22, 1.25, 0.015),
              (0, 0, th), padre_rel="Rueda")
    return "\n".join(sub) + "\n", "".join(nodos)
