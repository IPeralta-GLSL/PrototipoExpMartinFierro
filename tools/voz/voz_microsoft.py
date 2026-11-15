"""
Voz del narrador con "Tomás" (es-AR-TomasNeural), voz neural masculina
argentina de Microsoft, vía edge-tts (servicio online: solo se envían los
versos del guion, que son de dominio público).

Uso (Python de ComfyUIStudio; edge-tts está en tools/voz/.deps):
  python voz_microsoft.py --muestras            # muestras en trabajo/muestra_tomas_*.wav
  python voz_microsoft.py --estilo gaucho       # genera todas las líneas en godot/voz/
  python voz_microsoft.py --estilo normal --solo rancho_01

Para un uso comercial, la misma voz está en Azure Speech (de pago).
"""

import argparse
import asyncio
import os
import subprocess
import sys

AQUI = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(AQUI, ".deps"))
sys.path.insert(0, AQUI)
import edge_tts  # noqa: E402
import generar_voz as gv  # noqa: E402

VOZ = "es-AR-TomasNeural"
# rate/pitch en formato de edge-tts (SSML prosody).
ESTILOS = {
    "normal": dict(rate="+0%", pitch="+0Hz"),
    "pausado": dict(rate="-12%", pitch="-4Hz"),
    "gaucho": dict(rate="-16%", pitch="-10Hz"),
}


def log(*a):
    print("[tomas]", *a, flush=True)


async def sintetizar(texto, ruta_mp3, estilo):
    com = edge_tts.Communicate(texto, VOZ, **ESTILOS[estilo])
    await com.save(ruta_mp3)


def ffmpeg():
    import shutil
    exe = shutil.which("ffmpeg")
    if not exe:
        import imageio_ffmpeg
        exe = imageio_ffmpeg.get_ffmpeg_exe()
    return exe


def a_wav(mp3, wav):
    subprocess.run([ffmpeg(), "-y", "-loglevel", "error", "-i", mp3, "-ar", "24000", "-ac", "1", wav],
                   check=True)


async def verificar_voz():
    voces = await edge_tts.list_voices()
    nombres = {v["ShortName"] for v in voces}
    if VOZ not in nombres:
        ar = sorted(n for n in nombres if n.startswith("es-AR"))
        sys.exit(f"La voz {VOZ} no está disponible. Voces es-AR: {ar}")
    log(f"voz disponible: {VOZ}")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--muestras", action="store_true")
    ap.add_argument("--estilo", choices=sorted(ESTILOS), default="pausado")
    ap.add_argument("--solo", help="nombres separados por coma (rancho_01,...)")
    a = ap.parse_args()
    os.makedirs(gv.DIR_TRABAJO, exist_ok=True)
    asyncio.run(verificar_voz())

    if a.muestras:
        frase = gv.para_hablar(gv.TEXTO_REFERENCIA.replace(", ", ",\n"))
        for estilo in ESTILOS:
            mp3 = os.path.join(gv.DIR_TRABAJO, f"muestra_tomas_{estilo}.mp3")
            asyncio.run(sintetizar(frase, mp3, estilo))
            a_wav(mp3, mp3[:-4] + ".wav")
            os.remove(mp3)
            log(f"muestra {estilo}: trabajo/muestra_tomas_{estilo}.wav")
        return

    solo = set(a.solo.split(",")) if a.solo else None
    for nombre, texto in gv.lineas_de_guiones(solo):
        mp3 = os.path.join(gv.DIR_TRABAJO, f"tomas_{nombre}.mp3")
        wav = mp3[:-4] + ".wav"
        asyncio.run(sintetizar(gv.para_hablar(texto), mp3, a.estilo))
        a_wav(mp3, wav)
        os.remove(mp3)
        gv.a_ogg(wav, os.path.join(gv.DIR_VOZ, nombre + ".ogg"))
        gv.envolventes.escribir(os.path.join(gv.DIR_VOZ, nombre + ".ogg"))
        log(f"{nombre} ({a.estilo}) -> godot/voz/{nombre}.ogg")


if __name__ == "__main__":
    main()
