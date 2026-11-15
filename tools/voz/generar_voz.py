"""
Voz del gaucho narrador con Qwen3-TTS (Alibaba), 100 % local.

1) La voz de referencia (voz_gaucho_referencia.wav/.txt) sale de:
   - referencia_argentina.py: hablante real con acento rioplatense (dataset de
     Google, CC BY-SA 4.0). Es la voz actual del gaucho.
   - --nueva-voz: VoiceDesign crea una voz original a partir de una
     descripción (sin acento argentino real).
2) El modelo Base clona esa voz para todas las líneas de los guiones, así el
   narrador suena siempre igual.
3) Cada línea se verifica con Whisper (si no se entiende, se regenera con otra
   semilla), se normaliza el volumen y se exporta a OGG en godot/voz/.

Uso (Python de ComfyUIStudio):
  python generar_voz.py                 # todo lo que falte
  python generar_voz.py --rehacer       # regenerar todas las líneas
  python generar_voz.py --nueva-voz     # diseñar la voz de nuevo
  python generar_voz.py --solo rancho_03
"""

import argparse
import difflib
import json
import os
import re
import shutil
import subprocess
import sys
import time
import unicodedata

sys.dont_write_bytecode = True
os.environ.setdefault("HF_HUB_DISABLE_TELEMETRY", "1")

AQUI = os.path.dirname(os.path.abspath(__file__))
RAIZ = os.path.abspath(os.path.join(AQUI, "..", ".."))
NODO_QWEN = r"E:\ComfyUI\ComfyUIStudio\App\ComfyUI\custom_nodes\ComfyUI-Qwen-TTS"
MODELO_DISENO = r"E:\ComfyUI\qwen3tts_models\Qwen3-TTS-12Hz-1.7B-VoiceDesign"
MODELO_BASE = "Qwen/Qwen3-TTS-12Hz-1.7B-Base"  # ya está en la caché de Hugging Face
GUIONES = [os.path.join(RAIZ, "godot", "guion", n) for n in ("rancho.json", "fortin.json")]
DIR_VOZ = os.path.join(RAIZ, "godot", "voz")
DIR_TRABAJO = os.path.join(AQUI, "trabajo")
REFERENCIA_WAV = os.path.join(AQUI, "voz_gaucho_referencia.wav")
REFERENCIA_TXT = os.path.join(AQUI, "voz_gaucho_referencia.txt")

INSTRUCCION = (
    "Voz masculina de un gaucho argentino de unos cincuenta y cinco años: grave, cálida y algo "
    "ronca, con acento rioplatense del campo. Habla despacio y con pausas, en tono sereno y "
    "melancólico, como quien recita versos junto al fogón a la caída de la tarde."
)
TEXTO_REFERENCIA = (
    "Aquí me pongo á cantar, al compás de la vigüela, que el hombre que lo desvela una pena "
    "estraordinaria, como la ave solitaria con el cantar se consuela."
)
IDIOMA = "spanish"
PARAMS = dict(max_new_tokens=2048, top_p=0.8, top_k=20, temperature=0.9, repetition_penalty=1.05)

sys.path.insert(0, NODO_QWEN)
sys.path.insert(0, AQUI)
import envolventes  # noqa: E402


def log(*a):
    print("[voz]", *a, flush=True)


def para_hablar(texto):
    """Convierte los versos en una frase con pausas naturales para el TTS."""
    lineas = [l.strip() for l in texto.split("\n") if l.strip()]
    partes = []
    for l in lineas:
        l = l.replace("—", ",").replace("«", "").replace("»", "")
        if not re.search(r"[.,;:!?…]$", l):
            l += ","
        partes.append(l)
    frase = " ".join(partes)
    frase = re.sub(r",\s*,", ",", frase)
    return re.sub(r",$", ".", frase)


def normalizar(t):
    t = unicodedata.normalize("NFD", t.lower())
    t = "".join(c for c in t if unicodedata.category(c) != "Mn")
    return re.sub(r"[^a-zñ ]", "", t.replace("\n", " ")).split()


def parecido(a, b):
    return difflib.SequenceMatcher(None, normalizar(a), normalizar(b)).ratio()


class Oyente:
    """Whisper local para verificar que se entienda lo que dice la voz."""

    def __init__(self):
        from transformers import pipeline
        import torch
        self.asr = pipeline("automatic-speech-recognition", model="openai/whisper-base",
                            device=0 if torch.cuda.is_available() else -1)

    def transcribir(self, wav, sr):
        import numpy as np
        import librosa
        audio = wav.astype(np.float32)
        if sr != 16000:
            audio = librosa.resample(audio, orig_sr=sr, target_sr=16000)
        r = self.asr({"raw": audio, "sampling_rate": 16000},
                     generate_kwargs={"language": "spanish", "task": "transcribe"},
                     return_timestamps=True)
        return r["text"].strip()


def liberar():
    import gc
    import torch
    gc.collect()
    if torch.cuda.is_available():
        torch.cuda.empty_cache()


def disenar_voz(oyente):
    import soundfile as sf
    import torch
    from qwen_tts import Qwen3TTSModel
    log("cargando VoiceDesign…")
    modelo = Qwen3TTSModel.from_pretrained(MODELO_DISENO, device_map="cuda:0", dtype=torch.bfloat16)
    mejor = None
    for semilla in (1872, 1879, 1834, 1886):
        torch.manual_seed(semilla)
        t0 = time.time()
        wavs, sr = modelo.generate_voice_design(text=TEXTO_REFERENCIA, instruct=INSTRUCCION,
                                                language=IDIOMA, **PARAMS)
        wav = wavs[0]
        texto = oyente.transcribir(wav, sr)
        p = parecido(texto, TEXTO_REFERENCIA)
        dur = len(wav) / sr
        log(f"candidata {semilla}: {dur:.1f}s, parecido {p:.2f} ({time.time() - t0:.0f}s) -> {texto[:70]}")
        sf.write(os.path.join(DIR_TRABAJO, f"referencia_{semilla}.wav"), wav, sr)
        # Penaliza audios demasiado cortos o largos (recitado pausado: ~9-20 s).
        puntaje = p - (0.2 if not (8.0 <= dur <= 22.0) else 0.0)
        if mejor is None or puntaje > mejor[0]:
            mejor = (puntaje, wav, sr, semilla)
    _, wav, sr, semilla = mejor
    sf.write(REFERENCIA_WAV, wav, sr)
    open(REFERENCIA_TXT, "w", encoding="utf-8").write(TEXTO_REFERENCIA)
    log(f"voz elegida: semilla {semilla} -> {REFERENCIA_WAV}")
    del modelo
    liberar()


def a_ogg(wav_path, ogg_path):
    ffmpeg = shutil.which("ffmpeg")
    if not ffmpeg:
        import imageio_ffmpeg
        ffmpeg = imageio_ffmpeg.get_ffmpeg_exe()
    # Volumen parejo entre líneas, un toque de calidez y silencio corto al principio y al final.
    filtro = ("adelay=150|150,apad=pad_dur=0.35,"
              "highpass=f=70,loudnorm=I=-17:TP=-2:LRA=9")
    subprocess.run([ffmpeg, "-y", "-loglevel", "error", "-i", wav_path, "-af", filtro,
                    "-ar", "44100", "-ac", "1", "-c:a", "libvorbis", "-q:a", "5", ogg_path], check=True)


def lineas_de_guiones(solo=None):
    out = []
    for g in GUIONES:
        for l in json.load(open(g, encoding="utf-8")):
            audio = l.get("audio", "")
            if not audio:
                continue
            nombre = os.path.splitext(os.path.basename(audio))[0]
            if solo and nombre not in solo:
                continue
            out.append((nombre, l["texto"]))
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--rehacer", action="store_true")
    ap.add_argument("--nueva-voz", action="store_true")
    ap.add_argument("--solo", help="nombres separados por coma (rancho_01,...)")
    a = ap.parse_args()
    os.makedirs(DIR_TRABAJO, exist_ok=True)
    os.makedirs(DIR_VOZ, exist_ok=True)
    solo = set(a.solo.split(",")) if a.solo else None

    import soundfile as sf
    import torch
    oyente = Oyente()

    if a.nueva_voz or not os.path.exists(REFERENCIA_WAV):
        disenar_voz(oyente)

    pendientes = [(n, t) for n, t in lineas_de_guiones(solo)
                  if a.rehacer or not os.path.exists(os.path.join(DIR_VOZ, n + ".ogg"))]
    if not pendientes:
        log("no hay líneas pendientes")
        return
    from qwen_tts import Qwen3TTSModel
    log("cargando Base (clonado de la voz)…")
    modelo = Qwen3TTSModel.from_pretrained(MODELO_BASE, device_map="cuda:0", dtype=torch.bfloat16)
    ref, sr_ref = sf.read(REFERENCIA_WAV, dtype="float32")
    texto_ref = open(REFERENCIA_TXT, encoding="utf-8").read().strip()
    prompt = modelo.create_voice_clone_prompt(ref_audio=(ref, sr_ref), ref_text=texto_ref)

    resumen = []
    for nombre, texto in pendientes:
        frase = para_hablar(texto)
        mejor = None
        for intento, semilla in enumerate((11, 23, 37, 53)):
            torch.manual_seed(semilla)
            t0 = time.time()
            wavs, sr = modelo.generate_voice_clone(text=frase, language=IDIOMA,
                                                   voice_clone_prompt=prompt, **PARAMS)
            wav = wavs[0]
            oido = oyente.transcribir(wav, sr)
            p = parecido(oido, texto)
            log(f"{nombre} intento {intento + 1}: {len(wav) / sr:.1f}s parecido {p:.2f} "
                f"({time.time() - t0:.0f}s) -> {oido[:60]}")
            if mejor is None or p > mejor[0]:
                mejor = (p, wav, sr)
            if p >= 0.72:
                break
        p, wav, sr = mejor
        wav_path = os.path.join(DIR_TRABAJO, nombre + ".wav")
        sf.write(wav_path, wav, sr)
        a_ogg(wav_path, os.path.join(DIR_VOZ, nombre + ".ogg"))
        envolventes.escribir(os.path.join(DIR_VOZ, nombre + ".ogg"))  # para mover la boca
        resumen.append((nombre, round(len(wav) / sr, 1), round(p, 2)))
    log("listo:")
    for r in resumen:
        log(f"  {r[0]:<12} {r[1]:>5}s  parecido {r[2]}")


if __name__ == "__main__":
    main()
