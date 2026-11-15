"""
Orquestador de assets del prototipo "Martín Fierro VR".

Pipeline 100 % local con modelos chinos:
  1) Z-Image-Turbo (Alibaba / Tongyi-MAI) vía ComfyUI  -> imagen de referencia
  2) Hunyuan3D-2 (Tencent)                               -> malla + textura (GLB)
  3) Normalización                                       -> escala real, apoyado en y=0,
                                                            texturas livianas para el Quest

Uso (con el Python de ComfyUIStudio):
  python generar_assets.py --imagenes            # fase 1
  python generar_assets.py --mallas              # fase 2
  python generar_assets.py --normalizar          # fase 3
  python generar_assets.py --todo                # 1 + 2 + 3
  python generar_assets.py --todo --solo rancho,guitarra
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
import time

sys.dont_write_bytecode = True

AQUI = os.path.dirname(os.path.abspath(__file__))
RAIZ = os.path.abspath(os.path.join(AQUI, "..", ".."))
ASSETS_JSON = os.path.join(AQUI, "assets.json")
DIR_REFS = os.path.join(AQUI, "salida", "referencias")
DIR_CRUDOS = os.path.join(AQUI, "salida", "crudos")
DIR_MODELOS = os.path.join(RAIZ, "godot", "modelos")
PY = sys.executable


def cargar_assets(solo=None, lote=None):
    with open(ASSETS_JSON, encoding="utf-8") as f:
        datos = json.load(f)
    assets = datos["assets"]
    if lote:
        lotes = {s.strip().upper() for s in lote.split(",") if s.strip()}
        assets = [a for a in assets if str(a.get("lote", "A")).upper() in lotes]
    if solo:
        nombres = {s.strip() for s in solo.split(",") if s.strip()}
        assets = [a for a in assets if a["nombre"] in nombres]
        faltan = nombres - {a["nombre"] for a in assets}
        if faltan:
            sys.exit(f"Assets desconocidos: {', '.join(sorted(faltan))}")
    return datos, assets


def fase_imagenes(datos, assets, rehacer=False):
    os.makedirs(DIR_REFS, exist_ok=True)
    trabajos = []
    for a in assets:
        destino = os.path.join(DIR_REFS, a["nombre"] + ".png")
        if os.path.exists(destino) and not rehacer:
            print(f"[imagen] {a['nombre']}: ya existe, se conserva")
            continue
        trabajos.append({
            "name": a["nombre"],
            "prompt": datos.get("prefijo_prompt", "") + a["prompt"] + datos.get("sufijo_prompt", ""),
            "seed": int(a.get("semilla", 42)),
            "width": int(a.get("ancho", 1024)),
            "height": int(a.get("alto", 1024)),
        })
    if not trabajos:
        return
    archivo = os.path.join(AQUI, "salida", "trabajos_zimage.json")
    with open(archivo, "w", encoding="utf-8") as f:
        json.dump(trabajos, f, ensure_ascii=False, indent=2)
    t0 = time.time()
    cmd = [PY, "-B", os.path.join(AQUI, "zimage_generate.py"),
           "--jobs", archivo, "--outdir", DIR_REFS, "--stop"]
    print("[imagen] ", " ".join(cmd), flush=True)
    r = subprocess.run(cmd)
    if r.returncode != 0:
        sys.exit(f"Z-Image falló (código {r.returncode})")
    print(f"[imagen] {len(trabajos)} imágenes en {time.time() - t0:.0f} s")


def fase_mallas(datos, assets, rehacer=False, textura=True):
    os.makedirs(DIR_CRUDOS, exist_ok=True)
    grupos_cfg = datos.get("grupos", {})
    # Una tanda por grupo de calidad (octree / tamaño de textura son globales
    # en hunyuan3d_generate.py), en el orden en que aparecen los assets.
    por_grupo = {}
    for a in assets:
        imagen = os.path.join(DIR_REFS, a["nombre"] + ".png")
        destino = os.path.join(DIR_CRUDOS, a["nombre"] + ".glb")
        if not os.path.exists(imagen):
            print(f"[malla] {a['nombre']}: falta la imagen {imagen}, se omite")
            continue
        if os.path.exists(destino) and not rehacer:
            print(f"[malla] {a['nombre']}: ya existe, se conserva")
            continue
        por_grupo.setdefault(a.get("grupo", "chico"), []).append({
            "image": imagen,
            "out": destino,
            "faces": int(a.get("caras", 15000)),
            "seed": int(a.get("semilla", 1234)),
            "texture": bool(textura),
        })
    for grupo, lote in por_grupo.items():
        cfg = grupos_cfg.get(grupo, {})
        archivo = os.path.join(AQUI, "salida", f"lote_hunyuan_{grupo}.json")
        with open(archivo, "w", encoding="utf-8") as f:
            json.dump(lote, f, ensure_ascii=False, indent=2)
        t0 = time.time()
        cmd = [PY, "-B", os.path.join(AQUI, "hunyuan3d_generate.py"), "--batch", archivo,
               "--box-v", "1.05",
               "--octree", str(cfg.get("octree", 256)),
               "--texture-size", str(cfg.get("texture_size", 1024)),
               "--render-size", str(cfg.get("render_size", 1024))]
        print(f"[malla] grupo {grupo} ({len(lote)}): ", " ".join(cmd), flush=True)
        r = subprocess.run(cmd)
        if r.returncode != 0:
            print(f"[malla] Hunyuan3D (grupo {grupo}) terminó con código {r.returncode}; se sigue con lo que haya.")
        print(f"[malla] grupo {grupo}: {len(lote)} en {time.time() - t0:.0f} s", flush=True)


def _reducir_textura(img, max_lado):
    from PIL import Image
    if img is None:
        return None
    if max(img.size) <= max_lado:
        return img
    escala = max_lado / max(img.size)
    nuevo = (max(1, int(img.size[0] * escala)), max(1, int(img.size[1] * escala)))
    return img.resize(nuevo, Image.LANCZOS)


def normalizar_glb(entrada, salida, eje, metros, max_textura=1024):
    import numpy as np
    import trimesh
    from trimesh.visual.material import PBRMaterial

    escena = trimesh.load(entrada, force="scene")
    if not escena.geometry:
        raise RuntimeError(f"{entrada}: sin geometría")
    bmin, bmax = escena.bounds
    ext = bmax - bmin
    actual = ext[1] if eje == "alto" else max(ext[0], ext[2])
    if actual <= 1e-6:
        raise RuntimeError(f"{entrada}: tamaño nulo")
    s = float(metros) / float(actual)
    centro = (bmin + bmax) / 2.0
    T = np.eye(4)
    T[:3, 3] = [-centro[0], -bmin[1], -centro[2]]
    S = np.diag([s, s, s, 1.0])
    escena.apply_transform(S @ T)

    caras = 0
    for geom in escena.geometry.values():
        caras += len(getattr(geom, "faces", []))
        vis = getattr(geom, "visual", None)
        mat = getattr(vis, "material", None)
        if mat is None:
            continue
        # Textura base (PBR o material simple)
        tex = getattr(mat, "baseColorTexture", None)
        if tex is None:
            tex = getattr(mat, "image", None)
        tex = _reducir_textura(tex, max_textura)
        nuevo = PBRMaterial(
            name=getattr(mat, "name", None) or "material",
            baseColorTexture=tex,
            baseColorFactor=[255, 255, 255, 255],
            metallicFactor=0.0,   # glTF asume 1.0 si falta: se vería metálico
            roughnessFactor=0.92,
            doubleSided=False,
        )
        vis.material = nuevo

    os.makedirs(os.path.dirname(salida), exist_ok=True)
    escena.export(salida)
    b2 = escena.bounds
    return {
        "escala": s,
        "caras": caras,
        "tamano_m": [round(float(v), 3) for v in (b2[1] - b2[0])],
    }


def fase_normalizar(assets):
    os.makedirs(DIR_MODELOS, exist_ok=True)
    resumen = []
    for a in assets:
        crudo = os.path.join(DIR_CRUDOS, a["nombre"] + ".glb")
        if not os.path.exists(crudo):
            print(f"[normalizar] {a['nombre']}: no hay GLB crudo, se omite")
            continue
        eje = a.get("escala", {}).get("eje", "alto")
        metros = a.get("escala", {}).get("metros", 1.0)
        grande = metros >= 3.0
        info = normalizar_glb(
            crudo,
            os.path.join(DIR_MODELOS, a["nombre"] + ".glb"),
            eje,
            metros,
            max_textura=2048 if grande else 1024,
        )
        info["nombre"] = a["nombre"]
        resumen.append(info)
        print(f"[normalizar] {a['nombre']}: {info}")
    with open(os.path.join(AQUI, "salida", "resumen_modelos.json"), "w", encoding="utf-8") as f:
        json.dump(resumen, f, ensure_ascii=False, indent=2)


def fase_previas(assets):
    """Renderiza 4 vistas de cada modelo final y arma una hoja de contactos."""
    dir_prev = os.path.join(AQUI, "salida", "previas")
    os.makedirs(dir_prev, exist_ok=True)
    for a in assets:
        glb = os.path.join(DIR_MODELOS, a["nombre"] + ".glb")
        if not os.path.exists(glb):
            continue
        png = os.path.join(dir_prev, a["nombre"] + ".png")
        r = subprocess.run([PY, "-B", "-s", os.path.join(AQUI, "tests", "render_preview.py"), glb, png,
                            "--size", "256"], capture_output=True, text=True)
        if r.returncode != 0:
            print(f"[previa] {a['nombre']}: falló\n{r.stderr[-800:]}")
    subprocess.run([PY, "-B", os.path.join(AQUI, "hoja_contactos.py"), dir_prev,
                    os.path.join(AQUI, "salida", "hoja_modelos.jpg"), "520"])


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--imagenes", action="store_true")
    p.add_argument("--mallas", action="store_true")
    p.add_argument("--normalizar", action="store_true")
    p.add_argument("--previas", action="store_true", help="hoja de contactos de los modelos finales")
    p.add_argument("--todo", action="store_true")
    p.add_argument("--solo", help="lista separada por comas de assets a procesar")
    p.add_argument("--lote", help="A, B o A,B (orden de prioridad en assets.json)")
    p.add_argument("--rehacer", action="store_true", help="regenerar aunque ya existan")
    p.add_argument("--sin-textura", action="store_true", help="solo forma (más rápido)")
    a = p.parse_args()
    if not (a.imagenes or a.mallas or a.normalizar or a.previas or a.todo):
        p.print_help()
        return
    datos, assets = cargar_assets(a.solo, a.lote)
    if a.imagenes or a.todo:
        fase_imagenes(datos, assets, a.rehacer)
    if a.mallas or a.todo:
        fase_mallas(datos, assets, a.rehacer, textura=not a.sin_textura)
    if a.normalizar or a.todo:
        fase_normalizar(assets)
    if a.previas or a.todo:
        fase_previas(assets)


if __name__ == "__main__":
    main()
