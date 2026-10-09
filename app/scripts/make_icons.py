"""Erzeugt App- und Tray-Icons für LLM Bridge (benötigt Pillow).

    python3 app/scripts/make_icons.py

Alternativ mit eigenem Motiv: `cargo tauri icon pfad/zum/logo.png`.
"""
from pathlib import Path
from PIL import Image, ImageDraw

OUT = Path(__file__).resolve().parent.parent / "src-tauri" / "icons"
OUT.mkdir(parents=True, exist_ok=True)
S = 1024


def lerp(a, b, t):
    return tuple(int(a[i] + (b[i] - a[i]) * t) for i in range(3))


def glyph(draw, size, color, offset=(0, 0), scale=1.0):
    """Zwei Knoten, verbunden durch einen Brückenbogen."""
    ox, oy = offset
    u = size * scale
    cx = ox + size / 2
    cy = oy + size / 2
    r = u * 0.085
    lx, rx = cx - u * 0.27, cx + u * 0.27
    y = cy + u * 0.12
    w = max(2, int(u * 0.075))
    a = (rx - lx) / 2
    h = u * 0.30
    # Bogen: obere Ellipsenhälfte, Endpunkte liegen auf den Knoten
    draw.arc([lx, y - h, rx, y + h], start=180, end=360, fill=color, width=w)
    # Fahrbahn
    draw.line([lx, y, rx, y], fill=color, width=w)
    # Hänger zwischen Bogen und Fahrbahn
    for dx in (-0.5, 0.0, 0.5):
        x = cx + dx * a
        top = y - h * (1 - dx * dx) ** 0.5
        draw.line([x, top, x, y], fill=color, width=max(2, int(w * 0.55)))
    for x in (lx, rx):
        draw.ellipse([x - r, y - r, x + r, y + r], fill=color)


def app_icon():
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    bg = Image.new("RGBA", (S, S))
    top, bottom = (36, 44, 92), (14, 116, 120)
    px = bg.load()
    for yy in range(S):
        for xx in range(S):
            t = (xx * 0.35 + yy * 0.65) / S
            px[xx, yy] = (*lerp(top, bottom, t), 255)
    mask = Image.new("L", (S, S), 0)
    margin = int(S * 0.08)
    ImageDraw.Draw(mask).rounded_rectangle([margin, margin, S - margin, S - margin], radius=int(S * 0.2), fill=255)
    img.paste(bg, (0, 0), mask)
    glyph(ImageDraw.Draw(img), S, (255, 255, 255, 255), scale=0.82)
    return img


def tray_icon(px):
    big = Image.new("RGBA", (px * 8, px * 8), (0, 0, 0, 0))
    glyph(ImageDraw.Draw(big), px * 8, (0, 0, 0, 255), scale=1.18)
    return big.resize((px, px), Image.LANCZOS)


icon = app_icon()
for name, size in [("32x32.png", 32), ("128x128.png", 128), ("128x128@2x.png", 256), ("icon.png", 512)]:
    icon.resize((size, size), Image.LANCZOS).save(OUT / name)
icon.save(OUT / "icon.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
icon.save(OUT / "icon.icns")
tray_icon(44).save(OUT / "tray.png")
print("Icons geschrieben nach", OUT)
