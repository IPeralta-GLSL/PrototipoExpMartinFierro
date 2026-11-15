"""
Riggeo y animaciones automáticas para personajes generados con IA en pose A.

  blender -b --factory-startup -P riggear.py -- entrada.glb salida.glb [--previas DIR]

1) Estima las articulaciones a partir de la malla (pose A: brazos a ~45° y
   piernas separadas): cadera, columna, pecho, cuello, cabeza, clavículas,
   hombros, codos, muñecas, rodillas, tobillos y punta del pie.
2) Crea el esqueleto y pesa la malla con los pesos automáticos de Blender
   (bone heat). Los vértices que queden sin peso toman el hueso más cercano.
3) Anima por código (cinemática directa + IK de dos huesos para los brazos):
   idle, hablar, saludar, mostrar, senalar, mate, caminar, asar, mirar, cavar.
   Todas son ciclos; Godot las reproduce por nombre (ModeloIA.reproducir).
4) Exporta un GLB con piel y animaciones. Con --previas renderiza cuadros de
   cada animación para revisarlas.

Ejes (Blender): +X = izquierda del personaje, -Y = adelante, +Z = arriba.
"""

import math
import os
import sys

import bpy
import numpy as np
from mathutils import Quaternion, Vector

FPS = 30


# ---------------------------------------------------------------- utilidades
def v(*c):
    return Vector(c)


def unit(*c):
    return Vector(c).normalized()


def leer_argumentos():
    a = sys.argv[sys.argv.index("--") + 1:]
    previas = a[a.index("--previas") + 1] if "--previas" in a else None
    return a[0], a[1], previas


# ---------------------------------------------------------------- carga
def cargar(ruta):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.context.scene.render.fps = FPS
    bpy.ops.import_scene.gltf(filepath=ruta)
    mallas = [o for o in bpy.context.scene.objects if o.type == "MESH"]
    for o in mallas:
        mw = o.matrix_world.copy()
        o.parent = None
        o.matrix_world = mw
    bpy.ops.object.select_all(action="DESELECT")
    for o in mallas:
        o.select_set(True)
    bpy.context.view_layer.objects.active = mallas[0]
    if len(mallas) > 1:
        bpy.ops.object.join()
    malla = bpy.context.view_layer.objects.active
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
    for o in list(bpy.context.scene.objects):
        if o.type == "EMPTY":
            bpy.data.objects.remove(o)
    malla.name = "Cuerpo"
    return malla


# ---------------------------------------------------------------- articulaciones
def estimar_articulaciones(malla):
    co = np.array([vv.co[:] for vv in malla.data.vertices])
    x, y, z = co[:, 0], co[:, 1], co[:, 2]
    H = float(z.max())
    k = H / 1.76

    # Medio ancho del torso: en 0.57–0.62 H los brazos (pose A) están separados.
    banda = np.sort(np.abs(x[(z > 0.57 * H) & (z < 0.62 * H)]))
    wt = None
    for i in range(1, len(banda)):
        if banda[i] - banda[i - 1] > 0.025 and banda[i - 1] > 0.06 * k:
            wt = float(banda[i - 1])
            break
    if wt is None:
        wt = 0.12 * H
    torso = co[(np.abs(x) < wt) & (z > 0.5 * H) & (z < 0.8 * H)]
    yc = float(torso[:, 1].mean())
    cab = co[(np.abs(x) < 0.06 * k) & (z > 0.86 * H) & (z < 0.9 * H)]
    yh = float(cab[:, 1].mean()) if len(cab) else yc

    J = {"H": H, "k": k, "wt": wt, "yc": yc, "yh": yh}
    for lado, s in (("L", 1.0), ("R", -1.0)):
        # Brazo: vértices afuera del torso, debajo de la axila.
        P = co[(s * x > wt + 0.03 * k) & (z > 0.45 * H) & (z < 0.74 * H)]
        c = P.mean(0)
        _, _, vt = np.linalg.svd(P - c)
        d = vt[0]
        if d[0] * s < 0:
            d = -d
        t = (P - c) @ d
        punta = c + d * t.max()
        kk = (s * 0.8 * wt - c[0]) / d[0]
        hombro = c + d * kk
        hombro[2] = float(np.clip(hombro[2], 0.74 * H, 0.83 * H))
        hombro[1] = yc
        muneca = punta - d * 0.10 * H
        codo = hombro + 0.52 * (muneca - hombro)
        codo[1] += 0.015 * k
        J["hombro" + lado] = Vector(hombro)
        J["codo" + lado] = Vector(codo)
        J["muneca" + lado] = Vector(muneca)
        J["punta" + lado] = Vector(punta)

        # Pierna.
        lad = s * x > 0.01 * k
        rod = co[lad & (np.abs(z - 0.28 * H) < 0.02 * H)].mean(0)
        tob = co[lad & (np.abs(z - 0.065 * H) < 0.015 * H)].mean(0)
        pie = co[lad & (z < 0.035 * H)]
        dedo = np.array([pie[:, 0].mean(), pie[:, 1].min() + 0.03 * k, 0.02 * k])
        cadera = np.array([s * max(0.055 * H, 0.45 * abs(rod[0])), yc, 0.50 * H])
        J["cadera" + lado] = Vector(cadera)
        J["rodilla" + lado] = Vector(rod)
        J["tobillo" + lado] = Vector(tob)
        J["dedo" + lado] = Vector(dedo)
    return J


# ---------------------------------------------------------------- esqueleto
HUESOS = []  # (nombre, cabeza, cola, padre, conectado) — se llena en crear_esqueleto


def crear_esqueleto(J):
    H, yc, yh = J["H"], J["yc"], J["yh"]
    c = lambda zz, yy=yc: Vector((0.0, yy, zz * H))  # noqa: E731
    HUESOS[:] = [
        ("cadera", c(0.50), c(0.58), None, False),
        ("columna", c(0.58), c(0.67), "cadera", True),
        ("pecho", c(0.67), c(0.79), "columna", True),
        ("cuello", c(0.79), c(0.86, (yc + yh) / 2), "pecho", True),
        ("cabeza", c(0.86, (yc + yh) / 2), c(1.0, yh), "cuello", True),
    ]
    for lado, s in (("L", 1.0), ("R", -1.0)):
        HUESOS.extend([
            ("clavicula." + lado, Vector((s * 0.04 * J["k"], yc, 0.79 * H)), J["hombro" + lado], "pecho", False),
            ("brazo." + lado, J["hombro" + lado], J["codo" + lado], "clavicula." + lado, True),
            ("antebrazo." + lado, J["codo" + lado], J["muneca" + lado], "brazo." + lado, True),
            ("mano." + lado, J["muneca" + lado], J["punta" + lado], "antebrazo." + lado, True),
            ("muslo." + lado, J["cadera" + lado], J["rodilla" + lado], "cadera", False),
            ("pierna." + lado, J["rodilla" + lado], J["tobillo" + lado], "muslo." + lado, True),
            ("pie." + lado, J["tobillo" + lado], J["dedo" + lado], "pierna." + lado, True),
        ])
    datos = bpy.data.armatures.new("Esqueleto")
    arm = bpy.data.objects.new("Esqueleto", datos)
    bpy.context.scene.collection.objects.link(arm)
    bpy.context.view_layer.objects.active = arm
    bpy.ops.object.mode_set(mode="EDIT")
    eb = datos.edit_bones
    for n, h, t, p, con in HUESOS:
        b = eb.new(n)
        b.head, b.tail = h, t
        b.roll = 0.0
        if p:
            b.parent = eb[p]
            b.use_connect = con
    bpy.ops.object.mode_set(mode="OBJECT")
    return arm


def pesar(malla, arm):
    bpy.ops.object.select_all(action="DESELECT")
    malla.select_set(True)
    arm.select_set(True)
    bpy.context.view_layer.objects.active = arm
    bpy.ops.object.parent_set(type="ARMATURE_AUTO")
    # Vértices sin peso (si bone heat falla en alguna zona) -> hueso más cercano.
    segs = [(b.name, b.head_local.copy(), b.tail_local.copy()) for b in arm.data.bones]
    grupos = {g.name: g for g in malla.vertex_groups}
    sin = 0
    for vv in malla.data.vertices:
        if sum(g.weight for g in vv.groups) > 1e-4:
            continue
        p = vv.co
        mejor, dmin = None, 1e9
        for n, a, b in segs:
            ab = b - a
            t = max(0.0, min(1.0, (p - a).dot(ab) / max(ab.length_squared, 1e-9)))
            d = (a + ab * t - p).length
            if d < dmin:
                mejor, dmin = n, d
        g = grupos.get(mejor) or malla.vertex_groups.new(name=mejor)
        grupos[mejor] = g
        g.add([vv.index], 1.0, "REPLACE")
        sin += 1
    bpy.context.view_layer.objects.active = malla
    bpy.ops.object.mode_set(mode="WEIGHT_PAINT")
    bpy.ops.object.vertex_group_limit_total(limit=4)
    bpy.ops.object.vertex_group_normalize_all(lock_active=False)
    bpy.ops.object.mode_set(mode="OBJECT")
    return sin


# ---------------------------------------------------------------- poses
class Poser:
    """Convierte una "pose" declarativa en rotaciones locales de los huesos.

    pose = {hueso: [op, ...], "_loc": Vector}. Operaciones (en orden):
      ("aim", dir)        el hueso apunta a dir (espacio del esqueleto)
      ("aimp", dir)       ídem pero relativo al padre (como si el padre estuviera en reposo)
      ("rot", eje, grados) gira alrededor de eje (marco del padre)
      ("ik", objetivo, polo)  sólo en brazo.X: muñeca a 'objetivo', codo hacia 'polo'
    """

    def __init__(self, arm):
        self.arm = arm
        self.huesos = arm.data.bones
        self.R = {b.name: b.matrix_local.to_3x3().normalized() for b in self.huesos}
        self.cab = {b.name: b.head_local.copy() for b in self.huesos}
        self.dir = {b.name: (b.tail_local - b.head_local).normalized() for b in self.huesos}
        self.largo = {b.name: b.length for b in self.huesos}
        self.orden = []

        def recorrer(b):
            self.orden.append(b.name)
            for h in b.children:
                recorrer(h)

        for b in self.huesos:
            if b.parent is None:
                recorrer(b)

    def evaluar(self, pose):
        C, P, Q, pend = {}, {}, {}, {}
        loc = pose.get("_loc", Vector((0, 0, 0)))
        for n in self.orden:
            b = self.huesos[n]
            p = b.parent.name if b.parent else None
            Cp = C[p] if p else Quaternion()
            P[n] = (P[p] + Cp @ (self.cab[n] - self.cab[p])) if p else self.cab[n] + loc
            D = Quaternion()
            ops = list(pose.get(n, []))
            if n in pend:
                ops = [("aim", pend.pop(n))] + ops
            for op in ops:
                if op[0] == "aim":
                    D = self.dir[n].rotation_difference(Cp.inverted() @ Vector(op[1]).normalized())
                elif op[0] == "aimp":
                    D = self.dir[n].rotation_difference(Vector(op[1]).normalized())
                elif op[0] == "rot":
                    D = Quaternion(Vector(op[1]).normalized(), math.radians(op[2])) @ D
                elif op[0] == "ik":
                    S = P[n]
                    lado = n.split(".")[1]
                    L1, L2 = self.largo[n], self.largo["antebrazo." + lado]
                    T = Vector(op[1])
                    dvec = T - S
                    dist = min(max(dvec.length, abs(L1 - L2) + 1e-3), (L1 + L2) * 0.995)
                    nrm = dvec.normalized()
                    a = (L1 * L1 - L2 * L2 + dist * dist) / (2 * dist)
                    h = math.sqrt(max(L1 * L1 - a * a, 0.0))
                    polo = Vector(op[2]).normalized()
                    pp = (polo - nrm * polo.dot(nrm))
                    pp = pp.normalized() if pp.length > 1e-6 else Vector((0, 1, 0))
                    E = S + nrm * a + pp * h
                    W = S + nrm * dist
                    D = self.dir[n].rotation_difference(Cp.inverted() @ (E - S).normalized())
                    pend["antebrazo." + lado] = (W - E).normalized()
            C[n] = Cp @ D
            Rb = self.R[n]
            Q[n] = (Rb.inverted() @ D.to_matrix() @ Rb).to_quaternion()
        return Q, loc


# ---------------------------------------------------------------- animación
def fcurves_de(accion):
    try:
        return list(accion.fcurves)
    except Exception:
        curvas = []
        for capa in accion.layers:
            for tira in capa.strips:
                for slot in accion.slots:
                    cb = tira.channelbag(slot)
                    if cb:
                        curvas += list(cb.fcurves)
        return curvas


def crear_accion(arm, poser, nombre, claves, cuadros):
    accion = bpy.data.actions.new(nombre)
    accion.use_fake_user = True
    arm.animation_data_create()
    arm.animation_data.action = accion
    previo = {}
    for f, pose in claves:
        Q, loc = poser.evaluar(pose)
        for n, q in Q.items():
            pb = arm.pose.bones[n]
            pb.rotation_mode = "QUATERNION"
            if n in previo and previo[n].dot(q) < 0:
                q = Quaternion((-q.w, -q.x, -q.y, -q.z))
            previo[n] = q
            pb.rotation_quaternion = q
            pb.keyframe_insert("rotation_quaternion", frame=f)
        pb = arm.pose.bones["cadera"]
        pb.location = poser.R["cadera"].inverted() @ loc
        pb.keyframe_insert("location", frame=f)
    for fc in fcurves_de(accion):
        fc.modifiers.new("CYCLES")
    accion.use_frame_range = True
    accion.frame_start, accion.frame_end = 0, cuadros
    accion.use_cyclic = True
    return accion


def animaciones(J, poser):
    """Define las animaciones en función de las medidas del personaje."""
    k, H, yc, yh = J["k"], J["H"], J["yc"], J["yh"]
    S = {"L": poser.cab["brazo.L"], "R": poser.cab["brazo.R"]}
    largo = {l: poser.largo["brazo." + l] + poser.largo["antebrazo." + l] for l in "LR"}
    sg = {"L": 1.0, "R": -1.0}
    polo = {l: unit(sg[l] * 0.45, 0.55, -1.0) for l in "LR"}

    def muslo_base(l):
        return [("aim", unit(sg[l] * 0.07, 0.0, -1.0))]

    def pie_base(l):
        d = poser.dir["pie." + l]
        return [("aim", unit(d.x * 0.3, d.y, d.z))]

    def base():
        p = {}
        for l in "LR":
            p["muslo." + l] = muslo_base(l)
            p["pierna." + l] = [("aimp", unit(sg[l] * 0.02, 0.03, -1.0))]
            p["pie." + l] = pie_base(l)
            p["brazo." + l] = [("ik", relajado(l), polo[l])]
        return p

    def relajado(l, extra=v(0, 0, 0)):
        return S[l] + unit(sg[l] * 0.17, -0.05, -1.0) * largo[l] * 0.96 + extra

    def gesto(l, alto=0.0, adelante=0.35, afuera=0.18):
        return S[l] + v(sg[l] * afuera * k, -adelante * k, (-0.30 + alto) * k)

    def con(p, **cambios):
        q = {n: list(ops) for n, ops in p.items()}
        for n, ops in cambios.items():
            n = n.replace("_L", ".L").replace("_R", ".R")
            q[n] = ops if n != "_loc" else ops
        return q

    def brazo(l, objetivo, pol=None):
        return [("ik", objetivo, pol or polo[l])]

    X, Y, Z = (1, 0, 0), (0, 1, 0), (0, 0, 1)
    A = {}

    # IDLE: respira, se balancea apenas y mira.
    A["idle"] = (120, [
        (0, con(base(), cabeza=[("rot", Z, 2)])),
        (60, con(base(), pecho=[("rot", X, -2)], columna=[("rot", Y, 1)],
                 cabeza=[("rot", Z, -2), ("rot", X, 2)],
                 brazo_L=brazo("L", relajado("L", v(0, 0, 0.012))),
                 brazo_R=brazo("R", relajado("R", v(0, 0, 0.012))))),
        (120, con(base(), cabeza=[("rot", Z, 2)])),
    ])

    # HABLAR: gestos con la derecha (y a veces la izquierda), cabeceos.
    A["hablar"] = (180, [
        (0, base()),
        (20, con(base(), brazo_R=brazo("R", gesto("R", 0.05, 0.32, 0.20)), cabeza=[("rot", X, 5)])),
        (45, con(base(), brazo_R=brazo("R", gesto("R", 0.13, 0.36, 0.26)), pecho=[("rot", Z, -4)],
                 cabeza=[("rot", Z, -5)])),
        (70, con(base(), brazo_R=brazo("R", gesto("R", 0.02, 0.30, 0.16)),
                 brazo_L=brazo("L", gesto("L", 0.0, 0.28, 0.20)), cabeza=[("rot", X, 4)])),
        (100, con(base(), brazo_R=brazo("R", gesto("R", 0.10, 0.38, 0.30)),
                  brazo_L=brazo("L", gesto("L", 0.06, 0.30, 0.24)), pecho=[("rot", Z, 3)],
                  cabeza=[("rot", Z, 6)])),
        (130, con(base(), brazo_R=brazo("R", gesto("R", 0.0, 0.26, 0.14)), cabeza=[("rot", X, 3)])),
        (180, base()),
    ])

    # SALUDAR: se toca el ala del sombrero y abre la mano.
    ala = v(-0.07 * k, yh - 0.16 * k, 0.93 * H)
    polo_alto = unit(-0.9, 0.25, -0.25)
    A["saludar"] = (120, [
        (0, base()),
        (22, con(base(), brazo_R=brazo("R", ala, polo_alto))),
        (36, con(base(), brazo_R=brazo("R", ala, polo_alto), cabeza=[("rot", X, 9)], pecho=[("rot", X, 3)])),
        (50, con(base(), brazo_R=brazo("R", ala + v(0, 0, 0.01), polo_alto))),
        (66, con(base(), brazo_R=brazo("R", gesto("R", 0.18, 0.32, 0.36)))),
        (88, con(base(), brazo_R=brazo("R", gesto("R", 0.06, 0.30, 0.22)))),
        (120, base()),
    ])

    # MOSTRAR: barre el horizonte con el brazo derecho ("hasta donde da la vista").
    def horizonte(grados, bajar=0.12):
        a = math.radians(grados)
        return S["R"] + unit(-math.sin(a), -math.cos(a), -bajar) * largo["R"] * 0.95

    A["mostrar"] = (180, [
        (0, base()),
        (25, con(base(), brazo_R=brazo("R", horizonte(5)), cabeza=[("rot", X, -3)])),
        (70, con(base(), brazo_R=brazo("R", horizonte(35)), pecho=[("rot", Z, -6)], cabeza=[("rot", Z, -10)])),
        (110, con(base(), brazo_R=brazo("R", horizonte(70)), pecho=[("rot", Z, -10)], cabeza=[("rot", Z, -18)])),
        (140, con(base(), brazo_R=brazo("R", horizonte(55, 0.3)), pecho=[("rot", Z, -5)], cabeza=[("rot", Z, -8)])),
        (180, base()),
    ])

    # SEÑALAR: apunta adelante y abajo (el narrador se gira hacia lo que señala).
    def apuntar(dx, dz):
        return S["R"] + unit(dx, -1.0, dz) * largo["R"] * 0.98

    A["senalar"] = (150, [
        (0, base()),
        (20, con(base(), brazo_R=brazo("R", apuntar(-0.12, -0.45)), pecho=[("rot", X, 6)], cabeza=[("rot", X, 8)])),
        (70, con(base(), brazo_R=brazo("R", apuntar(-0.16, -0.40)), pecho=[("rot", X, 6)], cabeza=[("rot", X, 6)])),
        (120, con(base(), brazo_R=brazo("R", apuntar(-0.10, -0.48)), pecho=[("rot", X, 7)], cabeza=[("rot", X, 9)])),
        (150, base()),
    ])

    # MATE: lleva la mano derecha a la boca, sorbe y baja.
    pecho_mate = v(-0.10 * k, yc - 0.30 * k, 0.66 * H)
    boca = v(-0.03 * k, yh - 0.20 * k, 0.85 * H)
    polo_mate = unit(-0.8, 0.3, -0.6)
    A["mate"] = (180, [
        (0, con(base(), brazo_R=brazo("R", pecho_mate, polo_mate))),
        (40, con(base(), brazo_R=brazo("R", boca, polo_mate))),
        (55, con(base(), brazo_R=brazo("R", boca, polo_mate), cabeza=[("rot", X, -6)])),
        (95, con(base(), brazo_R=brazo("R", boca, polo_mate), cabeza=[("rot", X, -4)])),
        (125, con(base(), brazo_R=brazo("R", pecho_mate, polo_mate))),
        (180, con(base(), brazo_R=brazo("R", pecho_mate, polo_mate))),
    ])

    # CAMINAR: ciclo de 1.2 s (el nodo se mueve en Godot).
    def paso(f_der, f_izq, rod_der, rod_izq, bob, giro):
        p = base()
        for l, ang, rod in (("R", f_der, rod_der), ("L", f_izq, rod_izq)):
            p["muslo." + l] = muslo_base(l) + [("rot", X, ang)]
            p["pierna." + l] = [("aimp", unit(sg[l] * 0.02, 0.03, -1.0)), ("rot", X, rod)]
            p["pie." + l] = pie_base(l) + [("rot", X, -(ang + rod) * 0.8)]
            # El brazo va al revés que la pierna del mismo lado.
            off = relajado(l) - S[l]
            off = Quaternion(Vector(X), math.radians(-ang * 0.7)) @ off
            p["brazo." + l] = [("ik", S[l] + off * 0.97, polo[l])]
        p["cadera"] = [("rot", Z, giro)]
        p["pecho"] = [("rot", Z, -giro * 1.3), ("rot", X, 3)]
        p["_loc"] = v(0, 0, bob * k)
        return p

    A["caminar"] = (36, [
        (0, paso(-22, 18, 5, 12, -0.02, 4)),
        (9, paso(0, -12, 5, 45, 0.01, 0)),
        (18, paso(18, -22, 12, 5, -0.02, -4)),
        (27, paso(-12, 0, 45, 5, 0.01, 0)),
        (36, paso(-22, 18, 5, 12, -0.02, 4)),
    ])

    # ASAR: inclinado hacia el asado, acomoda la carne; la otra mano en la cintura.
    cintura = v(J["wt"] * 0.95, yc + 0.02 * k, 0.56 * H)
    polo_cintura = unit(1.0, 0.3, 0.0)
    carne = S["R"] + v(-0.05 * k, -0.45 * k, -0.30 * k)
    inclinado = dict(columna=[("rot", X, 8)], pecho=[("rot", X, 10)], cabeza=[("rot", X, 10)])
    A["asar"] = (120, [
        (0, con(base(), brazo_R=brazo("R", carne), brazo_L=brazo("L", cintura, polo_cintura), **inclinado)),
        (30, con(base(), brazo_R=brazo("R", carne + v(0, 0, 0.04 * k)), brazo_L=brazo("L", cintura, polo_cintura), **inclinado)),
        (60, con(base(), brazo_R=brazo("R", carne + v(-0.04 * k, 0.02, 0.0)), brazo_L=brazo("L", cintura, polo_cintura), **inclinado)),
        (90, con(base(), brazo_R=brazo("R", carne + v(0, 0, -0.03 * k)), brazo_L=brazo("L", cintura, polo_cintura), **inclinado)),
        (120, con(base(), brazo_R=brazo("R", carne), brazo_L=brazo("L", cintura, polo_cintura), **inclinado)),
    ])

    # MIRAR: centinela que otea el horizonte (mano izquierda de visera, la derecha con el fusil).
    fusil = v(S["R"].x - 0.03 * k, yc - 0.10 * k, 0.55 * H)
    visera = v(0.06 * k, yh - 0.16 * k, 0.93 * H)
    polo_visera = unit(0.9, 0.25, -0.25)

    def otear(giro, mano_visera):
        izq = brazo("L", visera, polo_visera) if mano_visera else brazo("L", relajado("L"))
        return con(base(), brazo_R=brazo("R", fusil), brazo_L=izq,
                   pecho=[("rot", Z, giro * 0.35)], cuello=[("rot", Z, giro * 0.3)],
                   cabeza=[("rot", Z, giro * 0.45), ("rot", X, -2)])

    A["mirar"] = (240, [
        (0, otear(0, False)),
        (40, otear(20, True)),
        (75, otear(38, True)),
        (110, otear(36, True)),
        (140, otear(0, False)),
        (180, otear(-36, False)),
        (210, otear(-34, False)),
        (240, otear(0, False)),
    ])

    # CAVAR: con la pala; clava, empuja y tira la tierra al costado.
    def cavar(r, l, lean, rodilla, baja, giro):
        p = con(base(), brazo_R=brazo("R", r), brazo_L=brazo("L", l),
                columna=[("rot", X, lean * 0.5)], pecho=[("rot", X, lean * 0.5), ("rot", Z, giro)],
                cabeza=[("rot", X, lean * 0.3)])
        for lado in "LR":
            p["muslo." + lado] = muslo_base(lado) + [("rot", X, -rodilla * 0.6)]
            p["pierna." + lado] = [("aimp", unit(sg[lado] * 0.02, 0.03, -1.0)), ("rot", X, rodilla)]
            p["pie." + lado] = pie_base(lado) + [("rot", X, -rodilla * 0.4)]
        p["_loc"] = v(0, 0, -baja * k)
        return p

    abajo_r = v(-0.05 * k, yc - 0.42 * k, 0.40 * H)
    abajo_l = v(0.05 * k, yc - 0.32 * k, 0.60 * H)
    A["cavar"] = (72, [
        (0, cavar(abajo_r, abajo_l, 25, 15, 0.03, 0)),
        (20, cavar(abajo_r + v(0, 0, -0.08 * H), abajo_l + v(0, 0, -0.07 * H), 38, 30, 0.07, 0)),
        (45, cavar(v(-0.18 * k, yc - 0.32 * k, 0.62 * H), v(-0.02 * k, yc - 0.30 * k, 0.74 * H), 12, 8, 0.01, -18)),
        (72, cavar(abajo_r, abajo_l, 25, 15, 0.03, 0)),
    ])
    return A


# ---------------------------------------------------------------- previas
def renderizar_previas(arm, acciones, carpeta):
    os.makedirs(carpeta, exist_ok=True)
    sc = bpy.context.scene
    cam = bpy.data.objects.new("cam", bpy.data.cameras.new("cam"))
    sc.collection.objects.link(cam)
    sc.camera = cam
    H = max(vv.co.z for vv in bpy.data.objects["Cuerpo"].data.vertices)
    cam.location = (2.3, -3.4, 0.62 * H + 0.15)
    direccion = Vector((0, 0, 0.5 * H)) - cam.location
    cam.rotation_euler = direccion.to_track_quat("-Z", "Y").to_euler()
    cam.data.lens = 50
    sc.render.engine = "BLENDER_WORKBENCH"
    sc.display.shading.color_type = "TEXTURE"
    sc.display.shading.light = "STUDIO"
    sc.render.resolution_x, sc.render.resolution_y = 240, 320
    sc.render.film_transparent = False
    for nombre, (cuadros, claves) in acciones.items():
        arm.animation_data.action = bpy.data.actions[nombre]
        for f in sorted({claves[len(claves) // 4][0], claves[len(claves) // 2][0], claves[(3 * len(claves)) // 4][0]}):
            sc.frame_set(f)
            sc.render.filepath = os.path.join(carpeta, f"{nombre}_{f:03d}.png")
            bpy.ops.render.render(write_still=True)


# ---------------------------------------------------------------- principal
def main():
    entrada, salida, previas = leer_argumentos()
    malla = cargar(entrada)
    J = estimar_articulaciones(malla)
    print("[rig] alto %.2f m, torso %.3f, hombros %s / %s" % (
        J["H"], J["wt"], tuple(round(c, 3) for c in J["hombroL"]), tuple(round(c, 3) for c in J["hombroR"])))
    arm = crear_esqueleto(J)
    sin = pesar(malla, arm)
    print(f"[rig] pesos automáticos listos ({sin} vértices sin peso corregidos)")
    poser = Poser(arm)
    defs = animaciones(J, poser)
    for nombre, (cuadros, claves) in defs.items():
        crear_accion(arm, poser, nombre, claves, cuadros)
    arm.animation_data.action = bpy.data.actions["idle"]
    print("[rig] animaciones:", ", ".join(defs))
    if previas:
        renderizar_previas(arm, defs, previas)
    bpy.ops.object.select_all(action="DESELECT")
    malla.select_set(True)
    arm.select_set(True)
    os.makedirs(os.path.dirname(os.path.abspath(salida)), exist_ok=True)
    bpy.ops.export_scene.gltf(
        filepath=salida, export_format="GLB", use_selection=True,
        export_animations=True, export_animation_mode="ACTIONS", export_force_sampling=True,
        export_skins=True, export_yup=True, export_apply=False, export_def_bones=False,
        export_optimize_animation_size=True, export_image_format="AUTO",
    )
    print(f"[rig] exportado: {salida}")


if __name__ == "__main__":
    main()
