"""Synthetic inputs for the Hunyuan3D smoke test: a shaded red mug drawn with PIL on white (RGB, no alpha)."""

import os
import sys

from PIL import Image, ImageDraw, ImageFilter

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "data")


def red_mug(size=768):
    img = Image.new("RGB", (size, size), (255, 255, 255))
    d = ImageDraw.Draw(img)
    s = size / 768.0
    body = [int(v * s) for v in (230, 200, 520, 600)]
    # handle: thick ring on the right
    d.ellipse([int(470 * s), int(290 * s), int(640 * s), int(510 * s)], fill=(150, 15, 20))
    d.ellipse([int(515 * s), int(335 * s), int(595 * s), int(465 * s)], fill=(255, 255, 255))
    # body with horizontal shading (cylinder look)
    x0, y0, x1, y1 = body
    for x in range(x0, x1):
        t = (x - x0) / max(1, x1 - x0)
        shade = 1.0 - 0.55 * abs(t - 0.35) ** 1.2
        d.line([(x, y0 + int(18 * s)), (x, y1 - int(18 * s))], fill=(int(205 * shade), int(25 * shade), int(30 * shade)))
    d.ellipse([x0, y1 - int(40 * s), x1, y1], fill=(120, 10, 15))  # bottom rim
    d.ellipse([x0, y0, x1, y0 + int(44 * s)], fill=(170, 20, 25))  # top rim
    d.ellipse([x0 + int(14 * s), y0 + int(8 * s), x1 - int(14 * s), y0 + int(36 * s)], fill=(60, 5, 8))  # opening
    return img.filter(ImageFilter.SMOOTH)


def mate_rgba(size=768):
    """A gourd 'mate' with a metal 'bombilla', drawn on a transparent background (RGBA -> no rembg)."""
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    s = size / 768.0
    d.line([(int(400 * s), int(330 * s)), (int(520 * s), int(120 * s))], fill=(190, 190, 200, 255), width=int(16 * s))
    d.ellipse([int(500 * s), int(100 * s), int(540 * s), int(135 * s)], fill=(170, 170, 180, 255))
    for i in range(60):  # shaded gourd body
        t = i / 59.0
        r = int(185 * s * (1.0 - 0.45 * t))
        c = int(140 - 60 * t)
        d.ellipse([int(384 * s) - r, int(470 * s) - r + int(40 * t * s), int(384 * s) + r, int(470 * s) + r],
                  fill=(c, int(c * 0.62), int(c * 0.3), 255))
    d.ellipse([int(255 * s), int(295 * s), int(513 * s), int(355 * s)], fill=(95, 60, 25, 255))  # rim
    d.ellipse([int(275 * s), int(305 * s), int(493 * s), int(345 * s)], fill=(60, 110, 40, 255))  # yerba
    return img


def main():
    os.makedirs(OUT, exist_ok=True)
    rgb = red_mug()
    path = os.path.join(OUT, "red_mug.png")
    rgb.save(path)
    print(path)
    path = os.path.join(OUT, "mate_rgba.png")
    mate_rgba().save(path)
    print(path)
    return 0


if __name__ == "__main__":
    sys.exit(main())
