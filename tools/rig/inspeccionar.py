"""Imprime datos de un GLB (para ajustar el riggeo). blender -b -P inspeccionar.py -- modelo.glb"""
import sys
import bpy
import numpy as np

ruta = sys.argv[sys.argv.index("--") + 1]
bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.gltf(filepath=ruta)
for o in bpy.context.scene.objects:
    print("OBJ", o.name, o.type, tuple(round(v, 3) for v in o.location), tuple(round(v, 3) for v in o.dimensions))
    if o.type == "MESH":
        m = o.data
        co = np.array([(o.matrix_world @ v.co)[:] for v in m.vertices])
        print("  verts", len(co), "polys", len(m.polygons), "mats", [s.material.name if s.material else None for s in o.material_slots])
        print("  min", co.min(0).round(3), "max", co.max(0).round(3))
        H = co[:, 2].max()
        for f in (0.05, 0.15, 0.25, 0.35, 0.45, 0.5, 0.55, 0.6, 0.65, 0.7, 0.75, 0.8, 0.85, 0.9, 0.95):
            banda = co[np.abs(co[:, 2] - f * H) < 0.01 * H]
            if len(banda):
                xs = np.sort(banda[:, 0])
                gaps = np.diff(xs)
                g = np.argsort(gaps)[-3:]
                print(f"  z={f:.2f}H n={len(banda):5d} x[{xs[0]:+.3f},{xs[-1]:+.3f}] y[{banda[:,1].min():+.3f},{banda[:,1].max():+.3f}] "
                      "huecos " + " ".join(f"{xs[i]:+.3f}..{xs[i+1]:+.3f}" for i in sorted(g) if gaps[i] > 0.02))
        # Nariz: punto más adelantado (−Y) cerca de la altura de la cara.
        cara = co[np.abs(co[:, 2] - 0.9 * H) < 0.03 * H]
        print("  cara y-min", cara[:, 1].min().round(3), "y-max", cara[:, 1].max().round(3))
