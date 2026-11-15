"""
Genera godot/escenas/pulperia.tscn: el interior de una pulpería grande, de
noche, con ventanas enrejadas que dejan ver la pampa.

Parte de fortin.tscn (cielo nocturno, terreno, pasto, sonido, ciclo y jugador),
quita el fuerte y el fogón, y construye:
- el salón (14 x 9 m, 3.5 m de alto): paredes de adobe encalado con ventanas
  enrejadas y la puerta abierta, vigas, cumbrera, horcones, techo de paja y piso
  de tierra (materiales procedurales, sin texturas);
- el mostrador con la reja, estantes con botellas, ristras colgando, barriles,
  damajuanas, bolsas de yerba, cajones, dos mesas de truco, bancos, faroles con luz;
- la gente: el pulpero, dos paisanos al truco, el sargento y un soldado en la otra
  mesa, un tropero sentado en el banco, un soldado acodado al mostrador, un oyente
  con mate, un muchacho que va y viene (Caminante) y el payador con la guitarra que
  suena (GuitarraCriolla);
- afuera, a la vista por las ventanas: palenque con caballos, carreta y ombú.

No hay fogón: el portal de vuelta al pago hace crecer la luz del farol colgado.

Uso:  python tools/escenas/crear_pulperia.py
"""

import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from molino import molino  # noqa: E402

RAIZ = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".."))
ORIGEN = os.path.join(RAIZ, "godot", "escenas", "fortin.tscn")
DESTINO = os.path.join(RAIZ, "godot", "escenas", "pulperia.tscn")

# Salón (interior): x en [-X, X], z en [-Z, Z]
X, Z, H, E = 7.0, 4.5, 3.5, 0.4


def filtrar_fortin(texto):
    """Se queda con el entorno, el narrador y el jugador; saca el fuerte y el fogón."""
    bloques = re.split(r"(?m)^(?=\[)", texto)
    salida = []
    for b in bloques:
        if b.startswith("[node"):
            cabecera = b.split("\n", 1)[0]
            nombre = re.match(r'\[node name="([^"]+)"', cabecera).group(1)
            mp = re.search(r'parent="([^"]*)"', cabecera)
            padre = mp.group(1) if mp else None
            if padre and padre.startswith("Fuerte"):
                if not (padre == "Fuerte" and nombre == "Narrador") and not padre.startswith("Fuerte/Narrador"):
                    continue
                if padre.startswith("Fuerte/Narrador") and nombre != "Gaucho":
                    continue
        salida.append(b)
    return "".join(salida)


def reemplazar(s, viejo, nuevo, contar=1):
    assert s.count(viejo) >= contar, f"no encontré: {viejo!r}"
    return s.replace(viejo, nuevo)


# ------------------------------------------------------------------ materiales
def materiales():
    def ruido(id_, tipo, frec, octavas, extra=""):
        return (f'[sub_resource type="FastNoiseLite" id="{id_}"]\nnoise_type = {tipo}\nfrequency = {frec}\n'
                f"fractal_octaves = {octavas}\n{extra}\n")

    def textura(id_, ruido_id, colores=None, ancho=512, alto=512, bump=None):
        t = f'[sub_resource type="NoiseTexture2D" id="{id_}"]\nwidth = {ancho}\nheight = {alto}\nseamless = true\n'
        t += f"as_normal_map = true\nbump_strength = {bump}\n" if bump else f'color_ramp = SubResource("{colores}")\n'
        return t + f'noise = SubResource("{ruido_id}")\n\n'

    def degradado(id_, a, b):
        return (f'[sub_resource type="Gradient" id="{id_}"]\n'
                f"colors = PackedColorArray({a[0]}, {a[1]}, {a[2]}, 1, {b[0]}, {b[1]}, {b[2]}, 1)\n\n")

    def material(id_, tex, nor, escala, normal=0.6):
        return (f'[sub_resource type="StandardMaterial3D" id="{id_}"]\nalbedo_texture = SubResource("{tex}")\n'
                f"roughness = 1.0\nnormal_enabled = true\nnormal_scale = {normal}\n"
                f'normal_texture = SubResource("{nor}")\nuv1_scale = Vector3({escala}, {escala}, {escala})\n'
                "uv1_triplanar = true\nuv1_world_triplanar = true\n\n")

    m = ""
    m += ruido("ruido_cal", 1, 0.035, 5) + degradado("grad_cal", (0.78, 0.75, 0.68), (0.93, 0.91, 0.86))
    m += textura("tex_cal", "ruido_cal", "grad_cal") + textura("nor_cal", "ruido_cal", bump=7.0)
    m += material("mat_cal", "tex_cal", "nor_cal", 0.45)
    m += ruido("ruido_tierra", 3, 0.03, 4) + degradado("grad_tierra", (0.26, 0.19, 0.13), (0.42, 0.33, 0.23))
    m += textura("tex_tierra", "ruido_tierra", "grad_tierra") + textura("nor_tierra", "ruido_tierra", bump=4.0)
    m += material("mat_tierra", "tex_tierra", "nor_tierra", 0.6, 0.8)
    m += ruido("ruido_madera", 3, 0.05, 3, "fractal_lacunarity = 3.0")
    m += degradado("grad_madera", (0.16, 0.10, 0.06), (0.34, 0.23, 0.14))
    m += textura("tex_madera", "ruido_madera", "grad_madera", 128, 1024) + textura("nor_madera", "ruido_madera", None, 128, 1024, 3.0)
    m += material("mat_madera_salon", "tex_madera", "nor_madera", 0.8)
    m += ruido("ruido_paja", 2, 0.08, 3) + degradado("grad_paja", (0.28, 0.21, 0.11), (0.52, 0.41, 0.24))
    m += textura("tex_paja", "ruido_paja", "grad_paja") + textura("nor_paja", "ruido_paja", bump=8.0)
    m += material("mat_paja_salon", "tex_paja", "nor_paja", 1.2, 1.0)
    m += ('[sub_resource type="StandardMaterial3D" id="mat_hierro"]\nalbedo_color = Color(0.08, 0.08, 0.08, 1)\n'
          "metallic = 0.6\nroughness = 0.55\n\n")
    m += ('[sub_resource type="StandardMaterial3D" id="mat_cuerda"]\nalbedo_color = Color(0.3, 0.24, 0.16, 1)\n'
          "roughness = 1.0\n\n")
    return m


# ------------------------------------------------------------------ geometría
class Piezas:
    def __init__(self):
        self.sub, self.nodos, self.n = "", "", 0

    def caja(self, nombre, c, t, mat, padre="Salon/Habitacion"):
        self.n += 1
        mid = f"m_salon_{self.n}"
        self.sub += (f'[sub_resource type="BoxMesh" id="{mid}"]\nmaterial = SubResource("{mat}")\n'
                     f"size = Vector3({t[0]:.3f}, {t[1]:.3f}, {t[2]:.3f})\n\n")
        self.nodos += (f'\n[node name="{nombre}" type="MeshInstance3D" parent="{padre}"]\n'
                       f'position = Vector3({c[0]:.3f}, {c[1]:.3f}, {c[2]:.3f})\nmesh = SubResource("{mid}")\n')

    def cilindro(self, nombre, c, radio, alto, mat, padre="Salon/Habitacion"):
        self.n += 1
        mid = f"m_salon_{self.n}"
        self.sub += (f'[sub_resource type="CylinderMesh" id="{mid}"]\nmaterial = SubResource("{mat}")\n'
                     f"top_radius = {radio * 0.85:.3f}\nbottom_radius = {radio:.3f}\nheight = {alto:.3f}\n"
                     "radial_segments = 10\nrings = 1\n\n")
        self.nodos += (f'\n[node name="{nombre}" type="MeshInstance3D" parent="{padre}"]\n'
                       f'position = Vector3({c[0]:.3f}, {c[1]:.3f}, {c[2]:.3f})\nmesh = SubResource("{mid}")\n')

    def pared(self, nombre, eje, fijo, desde, hasta, aberturas):
        """Pared a lo largo de 'x' o 'z' en la coordenada 'fijo', con aberturas
        (a, b, y0, y1): ventanas (y0 > 0, con reja) o puertas (y0 = 0)."""
        def pieza(i, a, b, y0, y1):
            largo, alto, cy = b - a, y1 - y0, (y0 + y1) / 2
            cm = (a + b) / 2
            if eje == "x":
                self.caja(f"{nombre}{i}", (cm, cy, fijo), (largo, alto, E), "mat_cal")
            else:
                self.caja(f"{nombre}{i}", (fijo, cy, cm), (E, alto, largo), "mat_cal")

        i, cursor = 0, desde
        for a, b, y0, y1 in sorted(aberturas):
            if a > cursor:
                i += 1
                pieza(i, cursor, a, 0, H)
            if y0 > 0:
                i += 1
                pieza(i, a, b, 0, y0)
            i += 1
            pieza(i, a, b, y1, H)
            cursor = b
            if y0 > 0:  # reja y alféizar
                n = max(3, int(round((b - a) / 0.2)))
                for k in range(n):
                    p = a + (b - a) * (k + 0.5) / n
                    c = (p, (y0 + y1) / 2, fijo) if eje == "x" else (fijo, (y0 + y1) / 2, p)
                    self.caja(f"{nombre}Reja{i}_{k}", c, (0.03, y1 - y0, 0.03), "mat_hierro")
                c = ((a + b) / 2, y0 + 0.03, fijo) if eje == "x" else (fijo, y0 + 0.03, (a + b) / 2)
                t = (b - a + 0.12, 0.06, E + 0.08) if eje == "x" else (E + 0.08, 0.06, b - a + 0.12)
                self.caja(f"{nombre}Alfeizar{i}", c, t, "mat_madera_salon")
        if hasta > cursor:
            i += 1
            pieza(i, cursor, hasta, 0, H)


def construir_salon():
    p = Piezas()
    zf, xf = Z + E / 2, X + E / 2
    ventana = (1.0, 2.3)
    p.pared("Fondo", "x", -zf, -X - E, X + E, [(-6.0, -4.6, *ventana), (4.6, 6.0, *ventana)])
    p.pared("Frente", "x", zf, -X - E, X + E, [(-5.2, -3.8, *ventana), (-0.6, 0.6, 0.0, 2.3), (3.8, 5.2, *ventana)])
    p.pared("Izquierda", "z", -xf, -Z, Z, [(-2.4, -1.0, *ventana), (1.2, 2.6, *ventana)])
    p.pared("Derecha", "z", xf, -Z, Z, [(-2.4, -1.0, *ventana), (1.2, 2.6, *ventana)])
    p.caja("Puerta", (1.2, 1.15, Z - 0.05), (1.2, 2.3, 0.06), "mat_madera_salon")
    p.caja("Piso", (0, 0.0, 0), (2 * X, 0.04, 2 * Z), "mat_tierra")
    p.caja("Techo", (0, H + 0.06, 0), (2 * X + 2 * E, 0.12, 2 * Z + 2 * E), "mat_paja_salon")
    for i, x in enumerate(range(-6, 7, 2)):
        p.caja(f"Viga{i + 1}", (x, H - 0.11, 0), (0.18, 0.22, 2 * Z), "mat_madera_salon")
    p.caja("Cumbrera", (0, H - 0.34, 0), (2 * X, 0.24, 0.24), "mat_madera_salon")
    for i, x in enumerate((-3.5, 3.5)):
        p.cilindro(f"Horcon{i + 1}", (x, (H - 0.46) / 2, 0), 0.13, H - 0.46, "mat_madera_salon")
    return p


# ------------------------------------------------------------------ contenido
def modelo(nombre, padre, pos, modelo_, rot=0.0, extra="", suelo=True):
    g = ' groups=["apoyar_en_suelo"]' if suelo else ""
    r = f"rotation_degrees = Vector3(0, {rot}, 0)\n" if rot else ""
    return (f'\n[node name="{nombre}" type="ModeloIA" parent="{padre}"{g}]\n'
            f"position = Vector3({pos[0]}, {pos[1]}, {pos[2]})\n{r}modelo = \"{modelo_}\"\n{extra}")


def luz(padre, pos, energia, alcance, sombra=False, nombre="Luz"):
    s = "shadow_enabled = true\n" if sombra else ""
    return (f'\n[node name="{nombre}" type="OmniLight3D" parent="{padre}"]\n'
            f"position = Vector3({pos[0]}, {pos[1]}, {pos[2]})\nlight_color = Color(1, 0.7, 0.42, 1)\n"
            f"light_energy = {energia}\n{s}omni_range = {alcance}\nomni_attenuation = 1.15\n")


def animal(nombre, padre, pos, modelo_, rot, rutina, reaccion, sonido, punto, radio):
    """Animal que alterna animaciones y reacciona a las caricias (con su ModeloIA hijo)."""
    return (f'\n[node name="{nombre}" type="Animal" parent="{padre}" node_paths=PackedStringArray("modelo") '
            f'groups=["apoyar_en_suelo"]]\nposition = Vector3({pos[0]}, {pos[1]}, {pos[2]})\n'
            f'rotation_degrees = Vector3(0, {rot}, 0)\nmodelo = NodePath("Modelo")\nrutina = "{rutina}"\n'
            f'reaccion = "{reaccion}"\nsonido = "{sonido}"\n'
            f"punto_caricia = Vector3({punto[0]}, {punto[1]}, {punto[2]})\nradio_caricia = {radio}\n"
            + modelo("Modelo", f"{padre}/{nombre}", (0, 0, 0), modelo_, suelo=False))


def agarrable(nombre, padre, pos, modelo_, tipo, rot=0, radio=0.18):
    """Objeto que el jugador puede agarrar y tirar."""
    return (f'\n[node name="{nombre}" type="Agarrable" parent="{padre}"]\n'
            f"position = Vector3({pos[0]}, {pos[1]}, {pos[2]})\nrotation_degrees = Vector3(0, {rot}, 0)\n"
            f'tipo = "{tipo}"\nradio_agarre = {radio}\n'
            + modelo("Modelo", f"{padre}/{nombre}", (0, 0, 0), modelo_, suelo=False))


# Superficies físicas (tablero de las mesas y del mostrador): (centro, tamaño).
SUPERFICIES = [((-4.4, 0.81, -0.6), (0.9, 0.06, 1.7)), ((4.6, 0.81, 1.3), (0.9, 0.06, 1.7)),
               ((0.0, 0.92, -2.6), (3.4, 0.1, 1.2)), ((-3.0, 0.81, 2.6), (0.9, 0.06, 1.7))]


def colisores():
    t = '\n[node name="Superficies" type="StaticBody3D" parent="Salon"]\n'
    for i, (pos, _) in enumerate(SUPERFICIES, 1):
        t += (f'\n[node name="Caja{i}" type="CollisionShape3D" parent="Salon/Superficies"]\n'
              f'position = Vector3({pos[0]}, {pos[1]}, {pos[2]})\nshape = SubResource("col_{i}")\n')
    return t


def formas_superficies():
    return "".join(f'[sub_resource type="BoxShape3D" id="col_{i}"]\nsize = Vector3({t[0]}, {t[1]}, {t[2]})\n\n'
                   for i, (_, t) in enumerate(SUPERFICIES, 1))


def accesorio(nombre, padre, portador, pos=(0, -0.07, 0), rot=None, extra=""):
    r = f"rotation_degrees = Vector3({rot[0]}, {rot[1]}, {rot[2]})\n" if rot else ""
    return (f'\n[node name="{nombre}" type="Accesorio" parent="{padre}" node_paths=PackedStringArray("portador")]\n'
            f"position = Vector3({pos[0]}, {pos[1]}, {pos[2]})\n{r}portador = NodePath(\"{portador}\")\n{extra}")


def contenido(p):
    c = ""
    TABLERO, MOSTRADOR = 0.84, 0.97
    # --- El despacho: mostrador con reja, estantes con botellas y el pulpero.
    c += modelo("Mostrador", "Salon", (0, 0, -2.6), "mostrador")
    c += modelo("Estante1", "Salon", (-1.35, 0, -4.13), "estante")
    c += modelo("Estante2", "Salon", (1.45, 0, -4.13), "estante")
    c += modelo("Pulpero", "Salon", (0.55, 0, -3.48), "pulpero", extra='animacion = "escuchar"\n')
    c += modelo("BotellasMostrador", "Salon", (0.95, MOSTRADOR, -2.45), "botellas", 20, "sombras = false\n", suelo=False)
    for i, x in enumerate((-2.0, 0.0, 2.0)):
        c += modelo(f"Ristras{i + 1}", "Salon", (x, round(H - 0.22 - 0.82, 3), -3.75), "ristras", 25 * i,
                    "sombras = false\n", suelo=False)
    # --- Barriles, damajuanas, bolsas y cajones contra las paredes.
    for i, (pos, rot) in enumerate((((-3.05, 0, -4.05), 0), ((-3.75, 0, -4.1), 30), ((3.2, 0, -4.05), 40),
                                    ((6.35, 0, -4.0), 10), ((-6.35, 0, 0.3), 70))):
        c += modelo(f"Barril{i + 1}", "Salon", pos, "barril", rot)
    for i, (pos, rot) in enumerate((((-2.55, 0, -3.45), 0), ((2.45, 0, -3.5), 30), ((6.4, 0, 3.9), 0),
                                    ((6.0, 0, 4.15), -40), ((-6.4, 0, 4.0), 20), ((-6.1, 0, 0.75), 50))):
        c += modelo(f"Damajuana{i + 1}", "Salon", pos, "damajuana", rot, "sombras = false\n")
    c += modelo("Bolsas1", "Salon", (5.3, 0, -3.85), "bolsas", -10)
    c += modelo("Bolsas2", "Salon", (-5.6, 0, -3.8), "bolsas", 25)
    c += modelo("Cajones1", "Salon", (6.35, 0, -3.1), "cajones", 15)
    c += modelo("Cajones2", "Salon", (-4.6, 0, -4.0), "cajones", -20)
    c += modelo("Cuero", "Salon", (3.6, 0, 4.2), "cuero", 180)
    c += modelo("Recado", "Salon", (6.2, 0, 3.0), "recado", -60)
    # --- Mesa del truco 1: paisano y viejo.
    c += modelo("Mesa", "Salon", (-4.4, 0, -0.6), "mesa_truco")
    c += modelo("Paisano", "Salon", (-5.02, 0, -0.85), "paisano", 90, 'animacion = "truco"\n')
    c += modelo("Viejo", "Salon", (-3.78, 0, -0.85), "viejo", -90, 'animacion = "truco"\n')
    c += modelo("BotellasMesa", "Salon", (-4.4, TABLERO, -0.15), "botellas", -30, "sombras = false\n", suelo=False)
    c += modelo("FarolMesa", "Salon", (-4.4, TABLERO, 0.35), "farol", extra="sombras = false\n", suelo=False)
    c += luz("Salon/FarolMesa", (0, 0.22, 0), 1.0, 4.5)
    # --- Mesa del truco 2: el sargento y un soldado de licencia.
    c += modelo("Mesa2", "Salon", (4.6, 0, 1.3), "mesa_truco")
    c += modelo("Sargento", "Salon", (3.98, 0, 1.05), "sargento", 90, 'animacion = "truco"\n')
    c += modelo("Soldado", "Salon", (5.22, 0, 1.05), "soldado", -90, 'animacion = "truco"\n')
    c += modelo("BotellasMesa2", "Salon", (4.6, TABLERO, 1.7), "botellas", 60, "sombras = false\n", suelo=False)
    c += agarrable("Sombrero", "Salon", (4.5, TABLERO, 2.0), "sombrero", "sombrero", 40, 0.25)
    c += agarrable("MateMostrador", "Salon", (-0.4, MOSTRADOR, -2.4), "mate", "mate", 15)
    c += modelo("FarolMesa2", "Salon", (4.7, TABLERO, 0.75), "farol", extra="sombras = false\n", suelo=False)
    c += luz("Salon/FarolMesa2", (0, 0.22, 0), 0.9, 4.5)
    # --- Banco bajo la ventana izquierda: el tropero sentado. Otro banco a la derecha.
    c += modelo("Banco", "Salon", (-6.55, 0, 1.9), "banco", 90)
    c += modelo("Tropero", "Salon", (-6.55, 0, 1.7), "tropero", 90, 'animacion = "sentado"\n')
    c += modelo("Banco2", "Salon", (6.55, 0, 1.9), "banco", -90)
    # --- Detrás del jugador (pared de la puerta): que no quede nada vacío.
    c += modelo("BarrilPuerta1", "Salon", (-1.35, 0, 4.05), "barril", 15)
    c += modelo("BarrilPuerta2", "Salon", (1.45, 0, 4.1), "barril", -30)
    c += modelo("PilaLena", "Salon", (-2.75, 0, 4.12), "pila_lena", 0)
    c += modelo("Mesa3", "Salon", (-3.0, 0, 2.6), "mesa_truco")
    c += modelo("BotellasMesa3", "Salon", (-3.0, TABLERO, 2.3), "botellas", 45, "sombras = false\n", suelo=False)
    c += modelo("SombreroMesa3", "Salon", (-3.1, TABLERO, 2.9), "sombrero", -20, "sombras = false\n", suelo=False)
    c += modelo("Bolsas3", "Salon", (5.6, 0, 3.9), "bolsas", 160)
    c += modelo("Cajones3", "Salon", (2.6, 0, 4.0), "cajones", 10)
    c += modelo("Recado2", "Salon", (-5.9, 0, 3.6), "recado", 120)
    c += modelo("GuitarraPared", "Salon", (4.5, 1.25, 4.38), "guitarra", 180, "sombras = false\n", suelo=False)
    # --- De pie: el oyente con mate y un soldado acodado al mostrador.
    c += modelo("Oyente", "Salon", (-1.5, 0, -1.25), "levado", 70, 'animacion = "mate"\n')
    c += accesorio("Mate", "Salon/Oyente", "..", extra='vertical = true\nsolo_durante = "mate"\n')
    c += modelo("Modelo", "Salon/Oyente/Mate", (0, 0, 0), "mate", extra="sombras = false\n", suelo=False)
    c += modelo("Acodado", "Salon", (-0.75, 0, -1.72), "soldado", 165, 'animacion = "escuchar"\n')
    c += animal("Perro", "Salon", (2.4, 0, 0.6), "perro", -60, "idle:8-14,dormir:10-20", "contento", "gemido",
                (0.25, 0.35, 0), 0.45)
    c += colisores()
    # --- El muchacho mandadero va y viene entre el mostrador y las mesas.
    c += ('\n[node name="Mandadero" type="Caminante" parent="Salon" node_paths=PackedStringArray("modelo", "terreno") '
          'groups=["apoyar_en_suelo"]]\nposition = Vector3(-2.2, 0, -1.6)\nmodelo = NodePath("Muchacho")\n'
          "puntos = PackedVector3Array(-2.2, 0, -1.6, -3.3, 0, 0.7, -0.6, 0, 1.3, 2.9, 0, 0.3, -0.6, 0, 1.3, -3.3, 0, 0.7)\n"
          "velocidad = 0.9\npausa = 2.5\nterreno = NodePath(\"../../Terreno\")\n")
    c += modelo("Muchacho", "Salon/Mandadero", (0, 0, 0), "muchacho", extra='animacion = "idle"\n', suelo=False)
    # --- Guitarra del payador (se ve mientras toca; suena con la GuitarraCriolla).
    c += accesorio("Guitarra", "Salon/Narrador", "../Gaucho", (-0.75, 0, 0), (0, 0, -90),
                   'hueso = "mano.L"\nhacia_hueso = "mano.R"\nsolo_durante = "tocar"\n')
    c += modelo("Modelo", "Salon/Narrador/Guitarra", (0, 0, 0), "guitarra", extra="sombras = false\n", suelo=False)
    c += '\n[node name="Cuerdas" type="GuitarraCriolla" parent="Salon/Narrador/Guitarra"]\nvolumen_db = -2.0\n'
    # --- Faroles: mostrador y colgados de las vigas.
    c += modelo("FarolMostrador", "Salon", (-0.95, MOSTRADOR, -2.5), "farol", extra="sombras = false\n", suelo=False)
    c += luz("Salon/FarolMostrador", (0, 0.2, 0.2), 1.3, 5.5)
    alto_farol = H - 0.22 - 0.16 - 0.38
    for nombre, x, z, e, r, sombra in (("FarolColgado", 0, -1.0, 1.9, 9.0, True),
                                       ("FarolColgado2", -4, 1.8, 1.2, 6.5, False),
                                       ("FarolColgado3", 4, 1.8, 1.2, 6.5, False)):
        c += modelo(nombre, "Salon", (x, round(alto_farol, 3), z), "farol", extra="sombras = false\n", suelo=False)
        c += luz(f"Salon/{nombre}", (0, 0.15, 0), e, r, sombra)
        p.caja(f"Cuerda{nombre}", (x, H - 0.22 - 0.08, z), (0.012, 0.16, 0.012), "mat_cuerda")
    # Portal: sin fogón, crece la luz del farol colgado y la vista se funde.
    c += ('\n[node name="Portal" type="PortalFogon" parent="Salon" node_paths=PackedStringArray("luz")]\n'
          'escena_destino = "res://escenas/pampa.tscn"\nluz = NodePath("../FarolColgado/Luz")\ntiempo_maximo = 150.0\n')
    # --- Afuera, a la vista por las ventanas.
    c += '\n[node name="Afuera" type="Node3D" parent="."]\n'
    c += modelo("Palenque", "Afuera", (-5.3, 0, -7.4), "palenque")
    c += animal("CaballoMoro", "Afuera", (-6.1, 0, -8.3), "caballo_moro", 95, "idle:6-12,pastar:8-15", "cabecear",
                "resoplido", (0.9, 1.6, 0.1), 0.65)
    c += animal("CaballoGateado", "Afuera", (-4.5, 0, -8.6), "caballo_gateado", 75, "pastar:8-15,idle:6-12", "cabecear",
                "resoplido", (0.9, 1.6, 0.1), 0.65)
    c += modelo("Carreta", "Afuera", (11.0, 0, -1.2), "carreta", 25)
    c += modelo("Ombu", "Afuera", (17.0, 0, -19.0), "ombu", 30)
    c += luz("Afuera", (-5.3, 1.7, -5.4), 0.7, 6.0, nombre="LuzVentana")
    c += luz("Afuera", (0, 2.2, 5.6), 0.6, 5.0, nombre="LuzPuerta")
    return c


def main():
    s = filtrar_fortin(open(ORIGEN, encoding="utf-8").read())
    s = reemplazar(s, '[node name="Fortin" type="Node3D"]', '[node name="Pulperia" type="Node3D"]')
    s = s.replace('parent="Fuerte', 'parent="Salon')
    s = reemplazar(s, '[node name="Fuerte" type="Node3D" parent="."]', '[node name="Salon" type="Node3D" parent="."]')
    # El ciclo ya no tiene fogón.
    s = re.sub(r'\nfogon = NodePath\("\.\./Fuerte/Fogon"\)', "", s)
    s = re.sub(r'(\[node name="CicloDia"[^\]]*node_paths=PackedStringArray\([^)]*?), "fogon"', r"\1", s)
    # Terreno: tierra pisada alrededor, sendero desde la puerta; el pasto empieza afuera.
    s = re.sub(r"manchas = PackedVector3Array\([^)]*\)", "manchas = PackedVector3Array(0, 0, 10.5)", s)
    s = re.sub(r"senderos = PackedVector3Array\([^)]*\)", "senderos = PackedVector3Array(0, 5.2, 1.8, -3, 26, 0)", s)
    s = reemplazar(s, "radio_interno = 8.6", "radio_interno = 9.6")
    # Narrador: payador en el medio del salón.
    s = re.sub(r'(\[node name="Narrador" type="Narrador" parent="Salon" node_paths=PackedStringArray\([^)]*)\)',
               r'\1, "guitarra")', s)
    s = re.sub(r'(\[node name="Narrador" type="Narrador" parent="Salon"[^\n]*\n)position = [^\n]*\nrotation_degrees = [^\n]*\n',
               r"\1position = Vector3(1.3, 0, -0.7)\nrotation_degrees = Vector3(0, -15, 0)\n", s)
    s = reemplazar(s, 'guion = "res://guion/fortin.json"', 'guion = "res://guion/pulperia.json"\nguitarra = NodePath("Guitarra/Cuerdas")')
    s = reemplazar(s, 'portal = NodePath("../Fogon/Portal")', 'portal = NodePath("../Portal")')
    # Título, jugador y luciérnagas (adentro no).
    s = reemplazar(s, 'text = "El fortín\nLa frontera, hacia 1872"', 'text = "La pulpería\nNoche de guitarreada"')
    s = re.sub(r'(\[node name="Titulo" type="Label3D" parent="\."\]\n)position = [^\n]*\n',
               r"\1position = Vector3(0, 1.75, 1.6)\n", s)
    s = re.sub(r'(\[node name="Jugador" type="JugadorXR"[^\n]*\n)position = [^\n]*\n',
               r"\1position = Vector3(0, 0, 3.0)\n", s)
    s = reemplazar(s, "radio_limite = 30.0", "radio_limite = 6.0")
    s = re.sub(r'(\[node name="Luciernagas" type="GPUParticles3D" parent="\."\]\n)', r"\1visible = false\n", s)

    p = construir_salon()
    c = contenido(p)
    primera = s.index("[node ")
    # Un molino lejos, a la vista por la ventana del fondo a la derecha.
    sub_molino, nodos_molino = molino("Molino", "Afuera", (34, 0, -46), -34.8)
    c += nodos_molino
    s = s[:primera] + materiales() + p.sub + formas_superficies() + sub_molino + s[primera:]
    s = s.rstrip("\n") + "\n\n[node name=\"Habitacion\" type=\"Node3D\" parent=\"Salon\"]\n" + p.nodos + c
    open(DESTINO, "w", encoding="utf-8", newline="\n").write(s)
    print(f"[pulpería] {DESTINO}: {p.n} piezas del salón")


if __name__ == "__main__":
    main()
