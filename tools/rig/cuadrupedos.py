"""
Riggeo y animaciones de cuadrúpedos (caballos, perros y ovejas) para riggear.py.

  blender -b -P riggear.py -- entrada.glb salida.glb --cuadrupedo caballo|perro|perro_echado|oveja

Estima el esqueleto a partir de la malla (parada de perfil): el extremo con la
cabeza (más alto), las cuatro patas (vértices junto al suelo, separados en
delanteras/traseras e izquierda/derecha), el lomo, el cuello hasta la nuca, la
cabeza hasta el hocico y la cola. El perro echado no lleva patas (van con el
cuerpo).

Animaciones:
  caballo: idle (cola, cabeza, respira), pastar, cabecear
  perro (parado): idle, caminar, olfatear, contento
  perro_echado: idle, dormir, contento
  oveja: idle, pastar, cabecear (cuello corto: la cabeza hace casi todo)
"""


import bpy
import numpy as np
from mathutils import Vector


def estimar(malla, tipo):
    co = np.array([vv.co[:] for vv in malla.data.vertices])
    mn, mx = co.min(0), co.max(0)
    ax = 0 if (mx[0] - mn[0]) >= (mx[1] - mn[1]) else 1
    lat = 1 - ax
    H, L = float(mx[2]), float(mx[ax] - mn[ax])
    c0 = float((mn[ax] + mx[ax]) / 2)
    ext = 0.15 * L
    zpos = co[co[:, ax] > mx[ax] - ext][:, 2].max()
    zneg = co[co[:, ax] < mn[ax] + ext][:, 2].max()
    s = 1.0 if zpos >= zneg else -1.0           # +u = hacia la cabeza
    u = s * (co[:, ax] - c0)
    w = co[:, lat]
    z = co[:, 2]
    F = Vector((0.0, 0.0, 0.0))
    F[ax] = s
    izq = Vector((0.0, 0.0, 1.0)).cross(F)       # izquierda del animal
    wc = float(np.median(w))

    def P(uu, ww, zz):
        p = [0.0, 0.0, 0.0]
        p[ax] = c0 + s * uu
        p[lat] = ww
        p[2] = zz
        return Vector(p)

    def franja(u0, ancho=0.05):
        return co[np.abs(u - u0) < ancho * L]

    def lomo_vientre(u0):
        f = franja(u0)
        f = f[f[:, 2] > 0.3 * H] if len(f[f[:, 2] > 0.3 * H]) > 10 else f
        return float(f[:, 2].max()), float(f[:, 2].min())

    J = {"H": H, "L": L, "F": F, "izq": izq, "tipo": tipo, "patas": {}}
    if tipo in ("caballo", "perro", "oveja"):
        suelo = z < 0.08 * H
        patas = co[suelo]
        up, wp = s * (patas[:, ax] - c0), patas[:, lat]
        u_del = float(np.median(up[up > 0])) if np.any(up > 0) else 0.3 * L
        u_tras = float(np.median(up[up < 0])) if np.any(up < 0) else -0.3 * L
        for nombre, sel_u in (("del", up > 0), ("tras", up < 0)):
            for lado, signo in (("L", 1.0), ("R", -1.0)):
                sel = sel_u & ((wp - wc) * izq[lat] * signo > 0)
                if not np.any(sel):
                    continue
                pu, pw = float(up[sel].mean()), float(wp[sel].mean())
                lomo, vientre = lomo_vientre(pu)
                arriba = vientre + 0.3 * (lomo - vientre)
                ww = wc + (pw - wc) * 0.7
                rod = 0.45 * arriba
                du = 0.02 * L if nombre == "del" else -0.03 * L
                J["patas"][f"{nombre}.{lado}"] = (P(pu, ww, arriba), P(pu + du, pw, rod), P(pu, pw, 0.02 * H))
    else:
        u_del, u_tras = 0.15 * L, -0.25 * L

    lomo_t, vientre_t = lomo_vientre(u_tras)
    lomo_d, vientre_d = lomo_vientre(u_del)
    z_t = vientre_t + 0.55 * (lomo_t - vientre_t)
    z_d = vientre_d + 0.55 * (lomo_d - vientre_d)
    J["cadera"] = (P(u_tras, wc, z_t), P((u_tras + u_del) / 2, wc, (z_t + z_d) / 2))
    J["pecho"] = (J["cadera"][1], P(u_del, wc, z_d))

    # Cabeza: la nuca es el punto más alto adelante; el hocico, lo más adelantado arriba.
    frente = co[u > u_del + 0.05 * L]
    uf = s * (frente[:, ax] - c0)
    i_nuca = int(np.argmax(frente[:, 2]))
    u_nuca, z_nuca = float(uf[i_nuca]), float(frente[i_nuca, 2])
    cab = frente[frente[:, 2] > 0.6 * z_nuca]
    ucab = s * (cab[:, ax] - c0)
    w_cab = float(np.median(cab[ucab > u_nuca - 0.06 * L][:, lat])) if np.any(ucab > u_nuca - 0.06 * L) else wc
    hoc = cab[ucab > ucab.max() - 0.04 * L]
    u_hoc, z_hoc = float(ucab.max()), float(hoc[:, 2].mean())
    tam = max(z_nuca - z_hoc, 0.12 * H)
    nuca = P(u_nuca - 0.03 * L, w_cab, z_nuca - 0.35 * tam)
    J["cuello"] = (P(u_del + 0.03 * L, wc, z_d + 0.3 * (lomo_d - vientre_d)), nuca)
    J["cabeza"] = (nuca, P(u_hoc, w_cab, z_hoc))

    # Cola: el punto de la parte trasera más lejano a la base.
    atras = co[(u < u.min() + 0.14 * L) & (z > 0.03 * H)]
    if len(atras) > 10:
        lomo_c, _ = lomo_vientre(float(u.min()) + 0.12 * L)
        base = P(float(u.min()) + 0.13 * L, wc, lomo_c - 0.08 * (lomo_c - vientre_t))
        d = np.linalg.norm(atras - np.array(base[:]), axis=1)
        punta = Vector(atras[int(np.argmax(d))].tolist())
        J["cola"] = (base, base.lerp(punta, 0.5), punta)
    return J


def crear_esqueleto(J):
    huesos = [
        ("cadera", J["cadera"][0], J["cadera"][1], None, False),
        ("pecho", J["pecho"][0], J["pecho"][1], "cadera", True),
        # Cuello en dos huesos: la curva se reparte (pastar sin quebrarse).
        ("cuello1", J["cuello"][0], J["cuello"][0].lerp(J["cuello"][1], 0.5), "pecho", False),
        ("cuello2", J["cuello"][0].lerp(J["cuello"][1], 0.5), J["cuello"][1], "cuello1", True),
        ("cabeza", J["cabeza"][0], J["cabeza"][1], "cuello2", True),
    ]
    if "cola" in J:
        b, m, t = J["cola"]
        huesos += [("cola1", b, m, "cadera", False), ("cola2", m, t, "cola1", True)]
    for nombre, (arriba, rod, casco) in J["patas"].items():
        parte, lado = nombre.split(".")
        padre = "pecho" if parte == "del" else "cadera"
        huesos += [(f"{parte}_sup.{lado}", arriba, rod, padre, False),
                   (f"{parte}_inf.{lado}", rod, casco, f"{parte}_sup.{lado}", True)]
    datos = bpy.data.armatures.new("Esqueleto")
    arm = bpy.data.objects.new("Esqueleto", datos)
    bpy.context.scene.collection.objects.link(arm)
    bpy.context.view_layer.objects.active = arm
    bpy.ops.object.mode_set(mode="EDIT")
    eb = datos.edit_bones
    for n, h, t, p, con in huesos:
        b = eb.new(n)
        b.head, b.tail = h, t
        if (t - h).length < 1e-3:
            b.tail = h + Vector((0, 0, 0.05))
        if p:
            b.parent = eb[p]
            b.use_connect = con
    bpy.ops.object.mode_set(mode="OBJECT")
    return arm


def animaciones(J, poser):
    tipo = J["tipo"]
    izq, arriba = J["izq"], Vector((0.0, 0.0, 1.0))
    PITCH, YAW = izq[:], arriba[:]            # +PITCH = hocico abajo; +YAW = gira a la izquierda

    def eje_cola():
        if "cola" not in J:
            return YAW
        d = (J["cola"][2] - J["cola"][0]).normalized()
        e = d.cross(izq)
        return e[:] if e.length > 1e-3 else YAW

    MENEO = eje_cola()
    hay_cola = "cola" in J

    def pose(cuello=0.0, cabeza=0.0, giro=0.0, cola=0.0, pecho=0.0, patas=None, cadera=0.0, ladeo=0.0):
        p = {"cuello1": [("rot", PITCH, cuello * 0.55), ("rot", YAW, giro * 0.1)],
             "cuello2": [("rot", PITCH, cuello * 0.45), ("rot", YAW, giro * 0.25)],
             "cabeza": [("rot", PITCH, cabeza), ("rot", YAW, giro * 0.65), ("rot", J["F"][:], ladeo * 0.5)],
             "pecho": [("rot", PITCH, pecho)],
             "cadera": [("rot", YAW, cadera)]}
        if hay_cola:
            p["cola1"] = [("rot", MENEO, cola * 0.6)]
            p["cola2"] = [("rot", MENEO, cola * 0.4)]
        if patas:
            for clave, (sup, inf) in patas.items():
                parte, lado = clave.split(".")
                if f"{parte}_sup.{lado}" in poser.dir:
                    p[f"{parte}_sup.{lado}"] = [("rot", PITCH, sup)]
                    p[f"{parte}_inf.{lado}"] = [("rot", PITCH, inf)]
        return p

    A = {}
    if tipo == "caballo":
        A["idle"] = (240, [
            (0, pose()),
            (50, pose(cuello=-3, giro=8, cola=14, pecho=0.8)),
            (80, pose(cuello=-2, giro=6, cola=-10)),
            (130, pose(cuello=1, cabeza=3, giro=-4, cola=4, pecho=-0.5)),
            (185, pose(cuello=-2, giro=-8, cola=-14, ladeo=3)),
            (240, pose()),
        ])
        A["pastar"] = (240, [
            (0, pose()),
            # El cuello baja hasta el pasto y la cabeza compensa (hocico al suelo).
            (45, pose(cuello=62, cabeza=-30, pecho=3, cola=8)),
            (85, pose(cuello=66, cabeza=-32, pecho=3, giro=6, cola=-8)),
            (125, pose(cuello=63, cabeza=-28, pecho=3, giro=-5, cola=10)),
            (165, pose(cuello=66, cabeza=-31, pecho=3, giro=3, cola=-6)),
            (200, pose(cuello=62, cabeza=-30, pecho=3, cola=6)),
            (240, pose()),
        ])
        A["cabecear"] = (90, [
            (0, pose()),
            (12, pose(cuello=-16, cabeza=-10, cola=12)),
            (24, pose(cuello=6, cabeza=4, giro=9, cola=-12)),
            (38, pose(cuello=-12, cabeza=-6, giro=-9, cola=10)),
            (56, pose(cuello=2, giro=3)),
            (90, pose()),
        ])
    elif tipo == "perro":
        A["idle"] = (240, [
            (0, pose(cola=10)), (40, pose(cola=-10, giro=7, pecho=0.5)), (80, pose(cola=10, giro=8)),
            (130, pose(cola=-8, giro=-6, cabeza=-3)), (180, pose(cola=8, giro=-7, pecho=-0.5)),
            (240, pose(cola=10)),
        ])

        def paso(fase):
            a = 15.0
            s1, s2 = (-a, a) if fase == 0 else (a, -a)
            return {"del.L": (s1, 10 if s1 < 0 else 0), "tras.R": (s1, -12 if s1 < 0 else 0),
                    "del.R": (s2, 10 if s2 < 0 else 0), "tras.L": (s2, -12 if s2 < 0 else 0)}

        A["caminar"] = (24, [
            (0, pose(patas=paso(0), cola=10, cuello=4)),
            (6, pose(patas={k: (0, 0) for k in ("del.L", "del.R", "tras.L", "tras.R")}, cola=-10, cuello=2)),
            (12, pose(patas=paso(1), cola=10, cuello=4)),
            (18, pose(patas={k: (0, 0) for k in ("del.L", "del.R", "tras.L", "tras.R")}, cola=-10, cuello=2)),
            (24, pose(patas=paso(0), cola=10, cuello=4)),
        ])
        A["olfatear"] = (180, [
            (0, pose(cuello=24, cabeza=14, cola=6)), (40, pose(cuello=27, cabeza=17, giro=6, cola=-5)),
            (80, pose(cuello=25, cabeza=15, giro=-5, cola=6)), (120, pose(cuello=28, cabeza=18, giro=3)),
            (150, pose(cuello=25, cabeza=15, giro=-4, cola=-5)), (180, pose(cuello=24, cabeza=14, cola=6)),
        ])
        A["contento"] = (48, [
            (0, pose(cuello=-10, cabeza=-6, cola=30, cadera=4)), (6, pose(cuello=-12, cabeza=-4, cola=-30, cadera=-4, ladeo=4)),
            (12, pose(cuello=-10, cabeza=-6, cola=30, cadera=4)), (18, pose(cuello=-12, cabeza=-4, cola=-30, cadera=-4, ladeo=-4)),
            (24, pose(cuello=-10, cabeza=-6, cola=30, cadera=4)), (30, pose(cuello=-12, cabeza=-4, cola=-30, cadera=-4, ladeo=4)),
            (36, pose(cuello=-10, cabeza=-6, cola=30, cadera=4)), (42, pose(cuello=-12, cabeza=-4, cola=-30, cadera=-4)),
            (48, pose(cuello=-10, cabeza=-6, cola=30, cadera=4)),
        ])
    elif tipo == "oveja":
        A["idle"] = (200, [
            (0, pose()),
            (45, pose(giro=6, pecho=0.6, cola=6)),
            (90, pose(cuello=-2, giro=4, cabeza=-2)),
            (140, pose(giro=-5, cola=-6, pecho=-0.4)),
            (200, pose()),
        ])
        # Pastar: cuello corto y lanudo; baja el hocico al pasto y mastica.
        A["pastar"] = (220, [
            (0, pose()),
            (40, pose(cuello=34, cabeza=22, pecho=2)),
            (70, pose(cuello=36, cabeza=25, pecho=2, giro=4)),
            (100, pose(cuello=35, cabeza=22, pecho=2, giro=-3, cola=5)),
            (130, pose(cuello=37, cabeza=25, pecho=2, giro=2)),
            (165, pose(cuello=34, cabeza=22, pecho=2)),
            (220, pose()),
        ])
        A["cabecear"] = (80, [
            (0, pose()),
            (14, pose(cuello=-8, cabeza=-6, cola=6)),
            (30, pose(cuello=4, cabeza=3, giro=7)),
            (48, pose(cuello=-5, cabeza=-3, giro=-6, cola=-6)),
            (80, pose()),
        ])
    else:  # perro_echado
        A["idle"] = (180, [
            (0, pose()), (40, pose(giro=12, pecho=0.8)), (80, pose(giro=10, cola=8)),
            (100, pose(giro=0, cola=-8, pecho=-0.6)), (140, pose(giro=-10, cabeza=-3)), (180, pose()),
        ])
        A["dormir"] = (240, [
            (0, pose(cuello=18, cabeza=14)), (120, pose(cuello=19, cabeza=15, pecho=1.2)),
            (180, pose(cuello=18, cabeza=14, cola=5)), (240, pose(cuello=18, cabeza=14)),
        ])
        A["contento"] = (48, [
            (0, pose(cuello=-14, cabeza=-8, cola=26, ladeo=6)), (6, pose(cuello=-15, cabeza=-8, cola=-26, ladeo=6)),
            (12, pose(cuello=-14, cabeza=-8, cola=26, ladeo=-4)), (18, pose(cuello=-15, cabeza=-8, cola=-26, ladeo=-4)),
            (24, pose(cuello=-14, cabeza=-8, cola=26, ladeo=4)), (30, pose(cuello=-15, cabeza=-8, cola=-26, ladeo=4)),
            (36, pose(cuello=-14, cabeza=-8, cola=26)), (42, pose(cuello=-15, cabeza=-8, cola=-26)),
            (48, pose(cuello=-14, cabeza=-8, cola=26, ladeo=6)),
        ])
    return A


def corregir_pesos(malla, J):
    """El perro echado tiene las patas delanteras justo delante del hocico: lo que
    está pegado al piso no debe seguir al cuello ni a la cabeza (pasa al pecho)."""
    if J["tipo"] != "perro_echado":
        return 0
    grupos = {g.name: g for g in malla.vertex_groups}
    destino = grupos.get("pecho")
    cabeza = [grupos[n] for n in ("cuello1", "cuello2", "cabeza") if n in grupos]
    if destino is None or not cabeza:
        return 0
    limite = 0.4 * J["H"]
    indices = {g.index for g in cabeza}
    movidos = 0
    for vv in malla.data.vertices:
        if vv.co.z >= limite:
            continue
        peso = sum(g.weight for g in vv.groups if g.group in indices)
        if peso <= 0:
            continue
        for g in cabeza:
            g.remove([vv.index])
        destino.add([vv.index], peso, "ADD")
        movidos += 1
    return movidos
