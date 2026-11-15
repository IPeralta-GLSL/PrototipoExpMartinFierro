"""
Voz con acento argentino (rioplatense) para el narrador.

Qwen3-TTS no trae ninguna voz argentina (ni en el modelo local ni en la API):
el acento se toma de grabaciones reales. Se usan clips del
"Crowdsourced high-quality Argentinian Spanish speech data set" de Google
(OpenSLR SLR61, hablantes voluntarios de Buenos Aires, licencia CC BY-SA 4.0),
publicado en Hugging Face como ylacombe/google-argentinian-spanish.

1) Arma una referencia de ~14 s por hablante (clips concatenados + su texto).
2) --probar: el modelo Base de Qwen3-TTS clona cada candidato recitando la
   primera estrofa; Whisper verifica que se entienda. Las muestras quedan en
   trabajo/muestra_ar_<hablante>.wav para escucharlas.
3) --usar <hablante>: esa referencia pasa a ser la voz del gaucho
   (voz_gaucho_referencia.wav/.txt). Después: generar_voz.py --rehacer.

Uso (Python de ComfyUIStudio):
  python referencia_argentina.py --probar                  # candidatos por defecto
  python referencia_argentina.py --hablantes 8784,2484 --probar
  python referencia_argentina.py --usar 8784
"""

import argparse
import json
import os
import shutil
import sys
import time
import urllib.request

AQUI = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, AQUI)
import generar_voz as gv  # noqa: E402

DATASET = "ylacombe/google-argentinian-spanish"
CONFIG = "male"
DIR_DATOS = os.path.join(gv.DIR_TRABAJO, "dataset_ar")
# Los hablantes masculinos de tono más grave (F0 mediana ~98-105 Hz).
CANDIDATOS = ["8784", "2484", "7049", "1523"]
SEGUNDOS = 14.0
SR = 24000
DISENADA_WAV = os.path.join(AQUI, "voz_gaucho_disenada.wav")
DISENADA_TXT = os.path.join(AQUI, "voz_gaucho_disenada.txt")
CREDITO = os.path.join(AQUI, "voz_gaucho_referencia_fuente.txt")


def log(*a):
    print("[voz-ar]", *a, flush=True)


def pagina(offset, largo=100):
    """Filas del dataset (texto, hablante, URL del audio) vía datasets-server, con caché."""
    os.makedirs(DIR_DATOS, exist_ok=True)
    ruta = os.path.join(DIR_DATOS, f"filas_{offset}.json")
    if os.path.exists(ruta):
        return json.load(open(ruta, encoding="utf-8"))
    url = (f"https://datasets-server.huggingface.co/rows?dataset={DATASET}&config={CONFIG}"
           f"&split=train&offset={offset}&length={largo}")
    for intento in range(4):
        try:
            req = urllib.request.Request(url, headers={"User-Agent": "dataset-rows/1.0"})
            with urllib.request.urlopen(req, timeout=60) as r:
                datos = json.loads(r.read().decode("utf-8"))
            filas = [(f["row_idx"], str(f["row"]["speaker_id"]), f["row"]["text"],
                      f["row"]["audio"][0]["src"]) for f in datos["rows"]]
            json.dump(filas, open(ruta, "w", encoding="utf-8"), ensure_ascii=False)
            return filas
        except Exception as e:  # el servidor a veces devuelve 502
            log(f"  página {offset}: {e} (reintento {intento + 1})")
            time.sleep(3 + 4 * intento)
    return []


def clips_de(hablantes, por_hablante=8, total=1818):
    """Busca clips de los hablantes pedidos recorriendo el dataset."""
    encontrados = {h: [] for h in hablantes}
    for offset in range(0, total, 100):
        if all(len(v) >= por_hablante for v in encontrados.values()):
            break
        for idx, spk, texto, src in pagina(offset):
            if spk in encontrados and len(encontrados[spk]) < por_hablante:
                encontrados[spk].append((idx, texto, src))
    return encontrados


def bajar(src, ruta):
    if not os.path.exists(ruta):
        req = urllib.request.Request(src, headers={"User-Agent": "audio-fetch/1.0"})
        with urllib.request.urlopen(req, timeout=60) as r:
            open(ruta, "wb").write(r.read())
    return ruta


def armar_referencia(spk, clips):
    """Concatena clips recortados (con pausas cortas) hasta ~SEGUNDOS."""
    import numpy as np
    import soundfile as sf
    import librosa
    dir_audio = os.path.join(DIR_DATOS, "audio")
    os.makedirs(dir_audio, exist_ok=True)
    piezas, textos, dur = [], [], 0.0
    pausa = np.zeros(int(0.35 * SR), dtype=np.float32)
    # Preferir frases afirmativas de largo medio (suenan más a relato).
    clips = sorted(clips, key=lambda c: ("?" in c[1] or "¿" in c[1], abs(len(c[1]) - 55)))
    for idx, texto, src in clips:
        y, sr = sf.read(bajar(src, os.path.join(dir_audio, f"{spk}_{idx}.wav")), dtype="float32")
        if y.ndim > 1:
            y = y.mean(axis=1)
        y = librosa.resample(y, orig_sr=sr, target_sr=SR)
        y, _ = librosa.effects.trim(y, top_db=35)
        if piezas:
            piezas.append(pausa)
        piezas.append(y)
        t = texto.strip()
        if t[-1] not in ".!?…":
            t += "."
        textos.append(t)
        dur += len(y) / SR + 0.35
        if dur >= SEGUNDOS:
            break
    wav = np.concatenate(piezas)
    wav = 0.9 * wav / max(1e-6, float(np.abs(wav).max()))
    ruta = os.path.join(gv.DIR_TRABAJO, f"ref_ar_{spk}.wav")
    sf.write(ruta, wav, SR)
    texto = " ".join(textos)
    open(ruta[:-4] + ".txt", "w", encoding="utf-8").write(texto)
    log(f"referencia {spk}: {len(wav) / SR:.1f} s -> {texto[:80]}…")
    return ruta, texto


def probar(hablantes):
    import soundfile as sf
    import torch
    from qwen_tts import Qwen3TTSModel
    oyente = gv.Oyente()
    log("cargando Base…")
    modelo = Qwen3TTSModel.from_pretrained(gv.MODELO_BASE, device_map="cuda:0", dtype=torch.bfloat16)
    frase = gv.para_hablar(gv.TEXTO_REFERENCIA.replace(", ", ",\n"))
    resultados = []
    for spk in hablantes:
        ruta = os.path.join(gv.DIR_TRABAJO, f"ref_ar_{spk}.wav")
        ref, sr_ref = sf.read(ruta, dtype="float32")
        texto_ref = open(ruta[:-4] + ".txt", encoding="utf-8").read().strip()
        prompt = modelo.create_voice_clone_prompt(ref_audio=(ref, sr_ref), ref_text=texto_ref)
        mejor = None
        for semilla in (11, 23):
            torch.manual_seed(semilla)
            wavs, sr = modelo.generate_voice_clone(text=frase, language=gv.IDIOMA,
                                                   voice_clone_prompt=prompt, **gv.PARAMS)
            p = gv.parecido(oyente.transcribir(wavs[0], sr), gv.TEXTO_REFERENCIA)
            if mejor is None or p > mejor[0]:
                mejor = (p, wavs[0], sr)
        p, wav, sr = mejor
        sf.write(os.path.join(gv.DIR_TRABAJO, f"muestra_ar_{spk}.wav"), wav, sr)
        resultados.append((spk, p, len(wav) / sr))
        log(f"hablante {spk}: parecido {p:.2f}, {len(wav) / sr:.1f} s -> trabajo/muestra_ar_{spk}.wav")
    resultados.sort(key=lambda r: -r[1])
    log("ranking: " + ", ".join(f"{s} ({p:.2f})" for s, p, _ in resultados))
    return resultados


def usar(spk):
    ruta = os.path.join(gv.DIR_TRABAJO, f"ref_ar_{spk}.wav")
    if not os.path.exists(ruta):
        sys.exit(f"No existe {ruta}: corré primero con --hablantes {spk}")
    if os.path.exists(gv.REFERENCIA_WAV) and not os.path.exists(DISENADA_WAV):
        # Guardar la voz diseñada con VoiceDesign por si se quiere volver a ella.
        shutil.copy(gv.REFERENCIA_WAV, DISENADA_WAV)
        shutil.copy(gv.REFERENCIA_TXT, DISENADA_TXT)
    shutil.copy(ruta, gv.REFERENCIA_WAV)
    shutil.copy(ruta[:-4] + ".txt", gv.REFERENCIA_TXT)
    open(CREDITO, "w", encoding="utf-8").write(
        f"Voz del narrador: clonada (Qwen3-TTS Base) del hablante {spk} del\n"
        "\"Crowdsourced high-quality Argentinian Spanish speech data set\" (Google, OpenSLR SLR61),\n"
        "licencia CC BY-SA 4.0. Guevara-Rukoz et al., \"Crowdsourcing Latin American Spanish for\n"
        "Low-Resource Text-to-Speech\", LREC 2020. https://www.openslr.org/61/\n")
    log(f"voz del gaucho = hablante {spk}. Ahora: generar_voz.py --rehacer")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--hablantes", default=",".join(CANDIDATOS))
    ap.add_argument("--probar", action="store_true")
    ap.add_argument("--usar")
    a = ap.parse_args()
    os.makedirs(gv.DIR_TRABAJO, exist_ok=True)
    if a.usar:
        usar(a.usar)
        return
    hablantes = [h.strip() for h in a.hablantes.split(",") if h.strip()]
    for spk, clips in clips_de(hablantes).items():
        if not clips:
            log(f"hablante {spk}: sin clips")
            continue
        armar_referencia(spk, clips)
    if a.probar:
        probar(hablantes)


if __name__ == "__main__":
    main()
