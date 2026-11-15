"""Arma una hoja de contactos con las imágenes de referencia (para revisar de un vistazo)."""
import math
import os
import sys

from PIL import Image, ImageDraw, ImageFont

AQUI = os.path.dirname(os.path.abspath(__file__))
carpeta = sys.argv[1] if len(sys.argv) > 1 else os.path.join(AQUI, "salida", "referencias")
salida = sys.argv[2] if len(sys.argv) > 2 else os.path.join(AQUI, "salida", "hoja_referencias.jpg")
celda = int(sys.argv[3]) if len(sys.argv) > 3 else 300

archivos = sorted(f for f in os.listdir(carpeta) if f.lower().endswith(".png"))
if not archivos:
    sys.exit("sin imágenes")
cols = min(6, len(archivos))
filas = math.ceil(len(archivos) / cols)
hoja = Image.new("RGB", (cols * celda, filas * (celda + 22)), (40, 40, 40))
dib = ImageDraw.Draw(hoja)
try:
    fuente = ImageFont.truetype("arial.ttf", 16)
except OSError:
    fuente = ImageFont.load_default()
for i, nombre in enumerate(archivos):
    img = Image.open(os.path.join(carpeta, nombre)).convert("RGB")
    img.thumbnail((celda, celda))
    x = (i % cols) * celda
    y = (i // cols) * (celda + 22)
    hoja.paste(img, (x + (celda - img.width) // 2, y + 22 + (celda - img.height) // 2))
    dib.text((x + 6, y + 3), os.path.splitext(nombre)[0], fill=(255, 220, 160), font=fuente)
hoja.save(salida, quality=88)
print(salida)
