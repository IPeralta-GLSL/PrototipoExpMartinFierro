"""
Envolvente de volumen de cada línea del narrador, para mover la boca del
gaucho al ritmo de la voz (Narrador + Mandibula en Godot).

Escribe godot/voz/<línea>.boca.json: lista de valores 0..1, 30 por segundo.
generar_voz.py la escribe sola al generar cada línea; este script rehace todas.

Uso (Python de ComfyUIStudio):  python envolventes.py
"""

import glob
import json
import os

import numpy as np
import soundfile as sf

AQUI = os.path.dirname(os.path.abspath(__file__))
DIR_VOZ = os.path.abspath(os.path.join(AQUI, "..", "..", "godot", "voz"))
FPS = 30


def envolvente(ruta_audio):
    y, sr = sf.read(ruta_audio, dtype="float32")
    if y.ndim > 1:
        y = y.mean(axis=1)
    paso = sr // FPS
    n = int(np.ceil(len(y) / paso))
    rms = np.array([np.sqrt(np.mean(y[i * paso:(i + 1) * paso] ** 2) + 1e-12) for i in range(n)])
    # Escala en dB relativa a lo más fuerte de la línea; debajo de -30 dB, boca cerrada.
    db = 20 * np.log10(rms / max(np.percentile(rms, 97), 1e-6))
    v = np.clip((db + 30.0) / 26.0, 0.0, 1.0)
    # Suavizado ataque/caída para que no tiemble.
    out, a = [], 0.0
    for x in v:
        a += (x - a) * (0.75 if x > a else 0.45)
        out.append(round(float(a), 2))
    return out


def escribir(ruta_ogg):
    datos = envolvente(ruta_ogg)
    destino = os.path.splitext(ruta_ogg)[0] + ".boca.json"
    with open(destino, "w", encoding="utf-8", newline="\n") as f:
        json.dump(datos, f, separators=(",", ":"))
    return destino, len(datos)


def main():
    for ogg in sorted(glob.glob(os.path.join(DIR_VOZ, "*.ogg"))):
        destino, n = escribir(ogg)
        print(f"[boca] {os.path.basename(destino)}: {n} cuadros ({n / FPS:.1f} s)")


if __name__ == "__main__":
    main()
