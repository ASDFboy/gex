#!/usr/bin/env python3
"""Makes GEX's built-in placeholder pack (gex://...): shapes, patterns, colors, UI pieces,
icons, characters, particles and skies. Simple, original, free to use in any game.

    python3 tools/make_placeholders.py

Writes crates/gex-assets/builtin/ (wiping it first). Needs Pillow.
"""
import math
import os
import random
import shutil
import sys

from PIL import Image, ImageDraw, ImageFilter

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "crates", "gex-assets", "builtin")
SS = 4  # supersampling for smooth edges

PALETTE = {
    "red": (230, 64, 64), "orange": (245, 145, 45), "yellow": (245, 210, 60), "lime": (150, 215, 60),
    "green": (60, 180, 90), "teal": (40, 185, 170), "cyan": (60, 200, 235), "blue": (60, 120, 230),
    "purple": (145, 85, 220), "pink": (235, 100, 175), "brown": (140, 95, 60), "gray": (128, 132, 140),
    "black": (30, 32, 38),
}
COUNT = 0


def save(img, rel):
    global COUNT
    p = os.path.join(OUT, rel)
    os.makedirs(os.path.dirname(p), exist_ok=True)
    img.save(p, optimize=True)
    COUNT += 1


def canvas(n, bg=(0, 0, 0, 0)):
    im = Image.new("RGBA", (n * SS, n * SS), bg)
    return im, ImageDraw.Draw(im)


def down(im, n):
    return im.resize((n, n), Image.LANCZOS)


def lighten(c, k):
    return tuple(int(v + (255 - v) * k) for v in c[:3])


def darken(c, k):
    return tuple(int(v * (1 - k)) for v in c[:3])


# ---------------------------------------------------------------- shapes
def poly(cx, cy, r, n, rot=-90):
    return [(cx + r * math.cos(math.radians(rot + i * 360 / n)), cy + r * math.sin(math.radians(rot + i * 360 / n))) for i in range(n)]


def star_pts(cx, cy, r1, r2, n):
    pts = []
    for i in range(n * 2):
        r = r1 if i % 2 == 0 else r2
        a = math.radians(-90 + i * 180 / n)
        pts.append((cx + r * math.cos(a), cy + r * math.sin(a)))
    return pts


def heart_pts(cx, cy, s):
    pts = []
    for i in range(200):
        t = i / 200 * 2 * math.pi
        x = 16 * math.sin(t) ** 3
        y = 13 * math.cos(t) - 5 * math.cos(2 * t) - 2 * math.cos(3 * t) - math.cos(4 * t)
        pts.append((cx + x * s, cy - y * s - s * 1.5))
    return pts


def shape(d, name, n, fill, outline_w=0):
    """Draws shape `name` on an n*SS canvas. outline_w > 0 draws only the outline."""
    S = n * SS
    m = S * 0.06
    c = S / 2
    r = S / 2 - m
    w = int(outline_w * SS)
    kw = dict(fill=None, outline=fill, width=w) if w else dict(fill=fill)

    def P(pts):
        if w:
            d.line(pts + [pts[0]], fill=fill, width=w, joint="curve")
        else:
            d.polygon(pts, fill=fill)

    if name == "circle":
        d.ellipse([m, m, S - m, S - m], **kw)
    elif name == "square":
        d.rectangle([m, m, S - m, S - m], **kw)
    elif name == "rounded_square":
        d.rounded_rectangle([m, m, S - m, S - m], radius=S * 0.18, **kw)
    elif name == "triangle":
        P(poly(c, c + S * 0.08, r * 1.05, 3))
    elif name == "right_triangle":
        P([(m, S - m), (S - m, S - m), (m, m)])
    elif name == "diamond":
        P([(c, m), (S - m, c), (c, S - m), (m, c)])
    elif name in ("pentagon", "hexagon", "octagon"):
        P(poly(c, c, r, {"pentagon": 5, "hexagon": 6, "octagon": 8}[name], -90 if name != "octagon" else -67.5))
    elif name == "star":
        P(star_pts(c, c + S * 0.03, r, r * 0.45, 5))
    elif name == "star6":
        P(star_pts(c, c, r, r * 0.55, 6))
    elif name == "burst":
        P(star_pts(c, c, r, r * 0.72, 12))
    elif name == "heart":
        P(heart_pts(c, c + S * 0.05, S / 38))
    elif name == "ring":
        d.ellipse([m, m, S - m, S - m], outline=fill, width=int(S * 0.14) if not w else w)
    elif name == "plus":
        t = S * 0.16
        pts = [(c - t, m), (c + t, m), (c + t, c - t), (S - m, c - t), (S - m, c + t), (c + t, c + t), (c + t, S - m), (c - t, S - m), (c - t, c + t), (m, c + t), (m, c - t), (c - t, c - t)]
        P(pts)
    elif name == "cross":
        t = S * 0.16
        pts = [(c - t, m), (c + t, m), (c + t, c - t), (S - m, c - t), (S - m, c + t), (c + t, c + t), (c + t, S - m), (c - t, S - m), (c - t, c + t), (m, c + t), (m, c - t), (c - t, c - t)]
        pts = [(c + (x - c) * 0.95 * math.cos(math.radians(45)) - (y - c) * 0.95 * math.sin(math.radians(45)), c + (x - c) * 0.95 * math.sin(math.radians(45)) + (y - c) * 0.95 * math.cos(math.radians(45))) for x, y in pts]
        P(pts)
    elif name == "arrow":
        P([(c, m), (S - m, c), (c + S * 0.14 * 0 + S * 0.62 - S * 0.12, c), (S * 0.62, S - m), (S * 0.38, S - m), (S * 0.38, c), (m, c)][:0] or
          [(c, m), (S - m, c * 1.0), (S * 0.64, c), (S * 0.64, S - m), (S * 0.36, S - m), (S * 0.36, c), (m, c)])
    elif name == "chevron":
        t = S * 0.2
        P([(m, c + t), (c, c - t * 1.4), (S - m, c + t), (S - m, c + t * 2.4), (c, c), (m, c + t * 2.4)])
    elif name == "capsule":
        d.rounded_rectangle([m, S * 0.3, S - m, S * 0.7], radius=S * 0.2, **kw)
    elif name == "half_circle":
        box = [c - r, c - r / 2, c + r, c + r * 1.5]
        if w:
            d.arc(box, 180, 360, fill=fill, width=w)
            d.line([(c - r, c + r / 2), (c + r, c + r / 2)], fill=fill, width=w)
        else:
            d.pieslice(box, 180, 360, fill=fill)
    elif name == "crescent":
        d.ellipse([m, m, S - m, S - m], fill=fill)
        d.ellipse([m + S * 0.28, m - S * 0.06, S - m + S * 0.28, S - m - S * 0.06], fill=(0, 0, 0, 0))
        if w:  # outline version: draw outline of the result instead
            pass
    elif name == "drop":
        pts = []
        for i in range(120):
            t = i / 120 * 2 * math.pi
            x = math.sin(t) * (1 - math.cos(t)) * 0.5
            y = -math.cos(t)
            pts.append((c + x * r * 1.1, c + S * 0.08 + y * r * 0.9))
        P(pts)
    elif name == "check":
        d.line([(S * 0.18, S * 0.52), (S * 0.42, S * 0.76), (S * 0.84, S * 0.26)], fill=fill, width=int(S * 0.14) if not w else w, joint="curve")
    elif name == "lightning":
        P([(S * 0.58, m), (S * 0.22, S * 0.56), (S * 0.48, S * 0.56), (S * 0.38, S - m), (S * 0.8, S * 0.4), (S * 0.53, S * 0.4)])
    elif name == "parallelogram":
        P([(S * 0.3, S * 0.25), (S - m, S * 0.25), (S * 0.7, S * 0.75), (m, S * 0.75)])
    elif name == "trapezoid":
        P([(S * 0.28, S * 0.25), (S * 0.72, S * 0.25), (S - m, S * 0.75), (m, S * 0.75)])
    elif name == "semicircle_ring":
        d.arc([m, m, S - m, S - m], 180, 360, fill=fill, width=int(S * 0.14) if not w else w)
    else:
        raise ValueError(name)


SHAPES = ["circle", "square", "rounded_square", "triangle", "right_triangle", "diamond", "pentagon", "hexagon",
          "octagon", "star", "star6", "burst", "heart", "ring", "plus", "cross", "arrow", "chevron", "capsule",
          "half_circle", "crescent", "drop", "check", "lightning", "parallelogram", "trapezoid"]


def make_shapes():
    n = 128
    for name in SHAPES:
        im, d = canvas(n)
        shape(d, name, n, (255, 255, 255, 255))
        save(down(im, n), f"shapes/white/{name}.png")
        if name not in ("ring", "check", "crescent", "semicircle_ring"):
            im, d = canvas(n)
            shape(d, name, n, (255, 255, 255, 255), outline_w=7)
            save(down(im, n), f"shapes/outline/{name}.png")
        for cname, col in PALETTE.items():
            im, d = canvas(n)
            shape(d, name, n, col + (255,))
            save(down(im, n), f"shapes/{cname}/{name}.png")


# ---------------------------------------------------------------- patterns (tileable)
def tile(n=128):
    return Image.new("RGBA", (n, n)), n


def make_patterns():
    pairs = {
        "gray": ((200, 204, 210), (150, 156, 166)), "dark": ((60, 64, 74), (40, 43, 50)),
        "blue": ((120, 170, 240), (70, 120, 210)), "green": ((140, 210, 120), (90, 170, 80)),
        "red": ((240, 120, 110), (200, 70, 70)), "purple": ((190, 150, 240), (140, 100, 210)),
        "bw": ((255, 255, 255), (20, 20, 20)), "orange": ((250, 190, 110), (230, 140, 50)),
    }
    for name, (a, b) in pairs.items():
        for cells in (2, 4, 8):
            im, n = tile()
            d = ImageDraw.Draw(im)
            s = n // cells
            for y in range(cells):
                for x in range(cells):
                    d.rectangle([x * s, y * s, x * s + s - 1, y * s + s - 1], fill=(a if (x + y) % 2 == 0 else b) + (255,))
            save(im, f"patterns/checker/{name}_{cells}.png")
        # stripes
        for kind in ("horizontal", "vertical", "diagonal"):
            im, n = tile()
            px = im.load()
            for y in range(n):
                for x in range(n):
                    v = {"horizontal": y, "vertical": x, "diagonal": (x + y)}[kind]
                    px[x, y] = (a if (v // 16) % 2 == 0 else b) + (255,)
            save(im, f"patterns/stripes/{name}_{kind}.png")
        # dots
        im, n = tile()
        d = ImageDraw.Draw(im)
        d.rectangle([0, 0, n, n], fill=a + (255,))
        for (cx, cy) in [(32, 32), (96, 96), (96, 32), (32, 96)]:
            d.ellipse([cx - 12, cy - 12, cx + 12, cy + 12], fill=b + (255,))
        save(im, f"patterns/dots/{name}.png")
        # grid lines
        im, n = tile()
        d = ImageDraw.Draw(im)
        d.rectangle([0, 0, n, n], fill=a + (255,))
        for k in range(0, n, 32):
            d.line([(k, 0), (k, n)], fill=b + (255,), width=2)
            d.line([(0, k), (n, k)], fill=b + (255,), width=2)
        save(im, f"patterns/grid/{name}.png")

    # Prototype ("dev") textures: a grid with a heavier line every metre and a label.
    for name, col in [("orange", (235, 140, 50)), ("gray", (150, 150, 155)), ("blue", (70, 130, 220)),
                      ("green", (80, 170, 90)), ("dark", (55, 58, 66)), ("white", (225, 228, 232)),
                      ("red", (210, 70, 70)), ("purple", (140, 90, 210))]:
        n = 256
        im = Image.new("RGBA", (n, n), col + (255,))
        d = ImageDraw.Draw(im)
        line = darken(col, 0.25) if sum(col) > 300 else lighten(col, 0.25)
        for k in range(0, n, 32):
            d.line([(k, 0), (k, n)], fill=line + (255,), width=1)
            d.line([(0, k), (n, k)], fill=line + (255,), width=1)
        d.rectangle([0, 0, n - 1, n - 1], outline=line + (255,), width=4)
        d.rectangle([8, 8, 84, 30], fill=darken(col, 0.35) + (255,) if sum(col) > 300 else lighten(col, 0.35) + (255,))
        d.text((14, 12), "1m x 1m", fill=(255, 255, 255, 255) if sum(col) < 600 else (30, 30, 30, 255))
        save(im, f"patterns/prototype/{name}.png")

    rnd = random.Random(42)

    def noise_tile(base, var, n=128, scale=8, name=""):
        # value noise, tileable by wrapping the lattice
        g = [[rnd.random() for _ in range(scale)] for _ in range(scale)]
        im = Image.new("RGBA", (n, n))
        px = im.load()
        for y in range(n):
            for x in range(n):
                fx, fy = x / n * scale, y / n * scale
                x0, y0 = int(fx) % scale, int(fy) % scale
                x1, y1 = (x0 + 1) % scale, (y0 + 1) % scale
                tx, ty = fx - int(fx), fy - int(fy)
                tx, ty = tx * tx * (3 - 2 * tx), ty * ty * (3 - 2 * ty)
                v = (g[y0][x0] * (1 - tx) + g[y0][x1] * tx) * (1 - ty) + (g[y1][x0] * (1 - tx) + g[y1][x1] * tx) * ty
                v = v * 0.7 + rnd.random() * 0.3
                k = (v - 0.5) * var
                px[x, y] = tuple(max(0, min(255, int(c + k))) for c in base) + (255,)
        return im

    for name, base, var, sc in [("grass", (80, 150, 60), 60, 16), ("dirt", (120, 85, 55), 55, 12), ("sand", (215, 190, 130), 40, 16),
                                ("stone", (125, 125, 130), 50, 8), ("water", (50, 110, 190), 50, 8), ("snow", (232, 238, 245), 22, 8),
                                ("lava", (200, 70, 20), 90, 8), ("gray", (128, 128, 128), 70, 8), ("moss", (70, 110, 60), 50, 12)]:
        save(noise_tile(base, var, scale=sc), f"patterns/noise/{name}.png")

    # Bricks
    for name, (mortar, brick) in {"red": ((190, 180, 170), (170, 70, 55)), "gray": ((90, 90, 95), (150, 150, 155)),
                                  "white": ((160, 160, 165), (225, 225, 228)), "dark": ((35, 35, 40), (85, 80, 85)),
                                  "yellow": ((150, 130, 90), (210, 180, 100))}.items():
        im, n = tile()
        d = ImageDraw.Draw(im)
        d.rectangle([0, 0, n, n], fill=mortar + (255,))
        bh, bw = 16, 32
        for row in range(n // bh):
            off = (bw // 2) if row % 2 else 0
            for col in range(-1, n // bw + 1):
                x = col * bw + off
                shade = rnd.uniform(-14, 14)
                c = tuple(max(0, min(255, int(v + shade))) for v in brick)
                d.rectangle([x + 1, row * bh + 1, x + bw - 2, row * bh + bh - 2], fill=c + (255,))
        save(im, f"patterns/bricks/{name}.png")

    # Floor tiles
    for name, (a, b) in {"white": ((235, 235, 238), (190, 192, 198)), "blue": ((110, 160, 220), (70, 110, 170)),
                         "terracotta": ((205, 120, 80), (150, 80, 55)), "black": ((50, 52, 58), (25, 26, 30))}.items():
        im, n = tile()
        d = ImageDraw.Draw(im)
        d.rectangle([0, 0, n, n], fill=b + (255,))
        for y in range(2):
            for x in range(2):
                d.rectangle([x * 64 + 2, y * 64 + 2, x * 64 + 61, y * 64 + 61], fill=a + (255,))
        save(im, f"patterns/tiles/{name}.png")

    # Wood planks
    for name, base in {"light": (190, 145, 95), "dark": (110, 70, 40), "red": (150, 75, 50)}.items():
        im, n = tile()
        px = im.load()
        for y in range(n):
            for x in range(n):
                plank = y // 32
                grain = math.sin((x * 0.09 + plank * 2.1) + math.sin(y * 0.3 + plank) * 0.8) * 10
                edge = -40 if y % 32 in (0, 31) else 0
                endcut = -40 if (x + plank * 47) % 128 in (0, 1) else 0
                px[x, y] = tuple(max(0, min(255, int(c + grain + edge + endcut))) for c in base) + (255,)
        save(im, f"patterns/wood/{name}.png")

    # Metal plate with rivets
    for name, base in {"steel": (150, 155, 165), "rust": (150, 90, 60), "gold": (210, 170, 70)}.items():
        im, n = tile()
        d = ImageDraw.Draw(im)
        d.rectangle([0, 0, n, n], fill=base + (255,))
        d.rectangle([1, 1, n - 2, n - 2], outline=darken(base, 0.35) + (255,), width=2)
        d.line([(2, 2), (n - 3, 2)], fill=lighten(base, 0.35) + (255,), width=2)
        for (x, y) in [(10, 10), (n - 11, 10), (10, n - 11), (n - 11, n - 11)]:
            d.ellipse([x - 4, y - 4, x + 4, y + 4], fill=darken(base, 0.25) + (255,))
            d.ellipse([x - 3, y - 3, x + 1, y + 1], fill=lighten(base, 0.3) + (255,))
        save(im, f"patterns/metal/{name}.png")

    # Waves, zigzag, chevrons, hexagons, diamonds, scales, circuit-ish lines
    for name, (a, b) in {"blue": ((70, 130, 210), (140, 190, 240)), "teal": ((30, 140, 130), (120, 210, 200)),
                         "warm": ((240, 160, 90), (200, 80, 60)), "mono": ((60, 60, 66), (200, 200, 205))}.items():
        im, n = tile()
        px = im.load()
        for y in range(n):
            for x in range(n):
                v = math.sin(x / n * 2 * math.pi * 2) * 8 + y
                px[x, y] = (a if int(v // 16) % 2 == 0 else b) + (255,)
        save(im, f"patterns/waves/{name}.png")
        im, n = tile()
        px = im.load()
        for y in range(n):
            for x in range(n):
                v = abs((x % 32) - 16) + y
                px[x, y] = (a if (v // 16) % 2 == 0 else b) + (255,)
        save(im, f"patterns/zigzag/{name}.png")
        im, n = tile()
        px = im.load()
        for y in range(n):
            for x in range(n):
                v = (abs((x % 32) - 16) + abs((y % 32) - 16))
                px[x, y] = (a if v < 12 else b) + (255,)
        save(im, f"patterns/diamonds/{name}.png")
        # scales (overlapping circles)
        im, n = tile()
        d = ImageDraw.Draw(im)
        d.rectangle([0, 0, n, n], fill=b + (255,))
        for row in range(-1, n // 16 + 2):
            for col in range(-1, n // 32 + 2):
                cx = col * 32 + (16 if row % 2 else 0)
                cy = row * 16
                d.ellipse([cx - 16, cy - 16, cx + 16, cy + 16], fill=a + (255,), outline=b + (255,), width=2)
        save(im, f"patterns/scales/{name}.png")
        # hexagon grid
        im = Image.new("RGBA", (128, 148), b + (255,))
        d = ImageDraw.Draw(im)
        hr = 18.5
        for row in range(-1, 6):
            for col in range(-1, 6):
                cx = col * 32 + (16 if row % 2 else 0)
                cy = row * 37
                pts = poly(cx, cy, hr, 6, -90)
                d.polygon(pts, fill=a + (255,), outline=b + (255,))
        save(im, f"patterns/hexagons/{name}.png")

    # Gradients
    for name, (a, b) in {"sunset": ((250, 120, 80), (90, 40, 120)), "sky": ((120, 180, 240), (230, 240, 250)),
                         "night": ((15, 20, 50), (70, 40, 110)), "fire": ((255, 220, 80), (200, 40, 20)),
                         "ocean": ((20, 120, 160), (5, 30, 70)), "mono": ((255, 255, 255), (0, 0, 0)),
                         "forest": ((140, 200, 110), (30, 80, 50)), "candy": ((255, 170, 220), (140, 200, 255))}.items():
        im = Image.new("RGBA", (8, 256))
        for y in range(256):
            t = y / 255
            c = tuple(int(a[i] + (b[i] - a[i]) * t) for i in range(3))
            for x in range(8):
                im.putpixel((x, y), c + (255,))
        save(im, f"patterns/gradients/{name}_vertical.png")
        save(im.rotate(90, expand=True), f"patterns/gradients/{name}_horizontal.png")
        n = 256
        im = Image.new("RGBA", (n, n))
        px = im.load()
        for y in range(n):
            for x in range(n):
                t = min(1.0, math.hypot(x - n / 2, y - n / 2) / (n / 2))
                px[x, y] = tuple(int(a[i] + (b[i] - a[i]) * t) for i in range(3)) + (255,)
        save(im, f"patterns/gradients/{name}_radial.png")


# ---------------------------------------------------------------- solid colors
def make_colors():
    more = {"white": (255, 255, 255), "light_gray": (200, 204, 210), "dark_gray": (70, 74, 82), "navy": (30, 45, 95),
            "maroon": (120, 30, 40), "olive": (120, 125, 40), "gold": (220, 175, 50), "sky": (140, 200, 245),
            "mint": (150, 230, 190), "peach": (250, 190, 150), "lavender": (190, 170, 240), "tan": (210, 180, 140),
            "forest": (35, 100, 55), "crimson": (190, 25, 55), "indigo": (75, 50, 160), "magenta": (220, 40, 200),
            "coral": (250, 120, 100), "beige": (240, 230, 200), "slate": (100, 115, 135)}
    for name, c in list(PALETTE.items()) + list(more.items()):
        save(Image.new("RGBA", (16, 16), c + (255,)), f"colors/{name}.png")


# ---------------------------------------------------------------- UI pieces (use with Panel/Button/Image "slice")
def make_ui():
    for name, (fill, edge) in {"light": ((235, 238, 242), (150, 158, 170)), "dark": ((40, 44, 54), (90, 98, 115)),
                               "blue": ((70, 120, 210), (40, 75, 150)), "green": ((70, 170, 95), (40, 115, 60)),
                               "red": ((215, 75, 70), (150, 45, 40)), "wood": ((170, 120, 75), (100, 65, 35)),
                               "glass": ((255, 255, 255, 60), (255, 255, 255, 150))}.items():
        n = 64
        im, d = canvas(n)
        S = n * SS
        f = fill if len(fill) == 4 else fill + (255,)
        e = edge if len(edge) == 4 else edge + (255,)
        d.rounded_rectangle([2 * SS, 2 * SS, S - 2 * SS, S - 2 * SS], radius=12 * SS, fill=f, outline=e, width=3 * SS)
        save(down(im, n), f"ui/panel_{name}.png")
        if name == "glass":
            continue
        # Buttons: normal / hover / pressed
        for state, k in (("normal", 0.0), ("hover", 0.15), ("pressed", -0.2)):
            im, d = canvas(n)
            base = lighten(fill, k) if k >= 0 else darken(fill, -k)
            d.rounded_rectangle([2 * SS, 6 * SS, S - 2 * SS, S - 2 * SS], radius=10 * SS, fill=darken(base, 0.35) + (255,))
            top = 2 * SS if state != "pressed" else 5 * SS
            d.rounded_rectangle([2 * SS, top, S - 2 * SS, S - 6 * SS + (top - 2 * SS)], radius=10 * SS, fill=base + (255,), outline=darken(base, 0.4) + (255,), width=2 * SS)
            # A soft shine along the top, blended over the face.
            shine = Image.new("RGBA", im.size, (0, 0, 0, 0))
            ImageDraw.Draw(shine).rounded_rectangle([6 * SS, top + 4 * SS, S - 6 * SS, top + 14 * SS], radius=6 * SS, fill=(255, 255, 255, 70))
            im = Image.alpha_composite(im, shine)
            save(down(im, n), f"ui/button_{name}_{state}.png")
    # Bars
    for name, col in {"red": PALETTE["red"], "green": PALETTE["green"], "blue": PALETTE["blue"], "yellow": PALETTE["yellow"], "purple": PALETTE["purple"]}.items():
        im = Image.new("RGBA", (64, 16))
        d = ImageDraw.Draw(im)
        d.rounded_rectangle([0, 0, 63, 15], radius=6, fill=col + (255,))
        d.rounded_rectangle([3, 2, 60, 6], radius=3, fill=lighten(col, 0.35) + (255,))
        save(im, f"ui/bar_fill_{name}.png")
    im = Image.new("RGBA", (64, 16))
    ImageDraw.Draw(im).rounded_rectangle([0, 0, 63, 15], radius=6, fill=(25, 27, 33, 255), outline=(80, 85, 95, 255))
    save(im, "ui/bar_back.png")
    # Checkbox, radio, slider knob, cursor, crosshairs
    n = 48
    for on in (False, True):
        im, d = canvas(n)
        S = n * SS
        d.rounded_rectangle([4 * SS, 4 * SS, S - 4 * SS, S - 4 * SS], radius=8 * SS, fill=(245, 245, 248, 255), outline=(110, 115, 125, 255), width=3 * SS)
        if on:
            d.line([(13 * SS, 25 * SS), (21 * SS, 33 * SS), (36 * SS, 15 * SS)], fill=(60, 160, 90, 255), width=5 * SS, joint="curve")
        save(down(im, n), f"ui/checkbox_{'on' if on else 'off'}.png")
        im, d = canvas(n)
        d.ellipse([4 * SS, 4 * SS, S - 4 * SS, S - 4 * SS], fill=(245, 245, 248, 255), outline=(110, 115, 125, 255), width=3 * SS)
        if on:
            d.ellipse([15 * SS, 15 * SS, S - 15 * SS, S - 15 * SS], fill=(60, 120, 230, 255))
        save(down(im, n), f"ui/radio_{'on' if on else 'off'}.png")
    im, d = canvas(n)
    S = n * SS
    d.ellipse([4 * SS, 4 * SS, S - 4 * SS, S - 4 * SS], fill=(250, 250, 252, 255), outline=(90, 95, 105, 255), width=3 * SS)
    save(down(im, n), "ui/slider_knob.png")
    for i, style in enumerate(["cross", "dot", "circle", "gap"]):
        n = 64
        im, d = canvas(n)
        S = n * SS
        c = S / 2
        wcol = (255, 255, 255, 255)
        if style == "cross":
            d.line([(c, 8 * SS), (c, S - 8 * SS)], fill=wcol, width=3 * SS)
            d.line([(8 * SS, c), (S - 8 * SS, c)], fill=wcol, width=3 * SS)
        elif style == "dot":
            d.ellipse([c - 4 * SS, c - 4 * SS, c + 4 * SS, c + 4 * SS], fill=wcol)
        elif style == "circle":
            d.ellipse([10 * SS, 10 * SS, S - 10 * SS, S - 10 * SS], outline=wcol, width=3 * SS)
            d.ellipse([c - 2 * SS, c - 2 * SS, c + 2 * SS, c + 2 * SS], fill=wcol)
        else:
            for a, b in [((c, 6 * SS), (c, c - 8 * SS)), ((c, c + 8 * SS), (c, S - 6 * SS)), ((6 * SS, c), (c - 8 * SS, c)), ((c + 8 * SS, c), (S - 6 * SS, c))]:
                d.line([a, b], fill=wcol, width=3 * SS)
        save(down(im, n), f"ui/crosshair_{style}.png")
    n = 32
    im, d = canvas(n)
    d.polygon([(2 * SS, 2 * SS), (2 * SS, 26 * SS), (9 * SS, 20 * SS), (14 * SS, 30 * SS), (18 * SS, 28 * SS), (13 * SS, 18 * SS), (22 * SS, 18 * SS)], fill=(255, 255, 255, 255), outline=(20, 20, 20, 255))
    save(down(im, n), "ui/cursor.png")


# ---------------------------------------------------------------- icons
ICON_COLORS = {"heart": PALETTE["red"], "star": PALETTE["yellow"], "coin": (240, 190, 50), "gem": PALETTE["cyan"],
               "key": (230, 185, 60), "skull": (235, 235, 230), "sword": (190, 200, 215), "shield": PALETTE["blue"],
               "bolt": PALETTE["yellow"], "flame": PALETTE["orange"], "drop": PALETTE["blue"], "gear": PALETTE["gray"],
               "home": PALETTE["brown"], "play": PALETTE["green"], "pause": PALETTE["gray"], "stop": PALETTE["red"],
               "plus": PALETTE["green"], "minus": PALETTE["red"], "check": PALETTE["green"], "close": PALETTE["red"],
               "arrow_up": PALETTE["gray"], "arrow_down": PALETTE["gray"], "arrow_left": PALETTE["gray"], "arrow_right": PALETTE["gray"],
               "music": PALETTE["purple"], "sound": PALETTE["gray"], "mute": PALETTE["gray"], "lock": PALETTE["gray"],
               "flag": PALETTE["red"], "clock": PALETTE["gray"], "info": PALETTE["blue"], "warning": PALETTE["yellow"],
               "potion": PALETTE["pink"], "bomb": PALETTE["black"], "trophy": (230, 180, 50), "eye": PALETTE["teal"]}


def icon(d, name, S, col):
    c = S / 2
    W = (255, 255, 255, 255)
    col = col + (255,)
    dark = darken(col, 0.4) + (255,)
    if name == "heart":
        d.polygon(heart_pts(c, c + S * 0.06, S / 40), fill=col)
    elif name == "star":
        d.polygon(star_pts(c, c + S * 0.04, S * 0.44, S * 0.2, 5), fill=col)
    elif name == "coin":
        d.ellipse([S * 0.1, S * 0.1, S * 0.9, S * 0.9], fill=darken(col, 0.25) + (255,))
        d.ellipse([S * 0.16, S * 0.12, S * 0.84, S * 0.84], fill=col)
        d.ellipse([S * 0.3, S * 0.26, S * 0.7, S * 0.7], outline=lighten(col, 0.4) + (255,), width=int(S * 0.04))
    elif name == "gem":
        d.polygon([(S * 0.3, S * 0.18), (S * 0.7, S * 0.18), (S * 0.9, S * 0.4), (c, S * 0.9), (S * 0.1, S * 0.4)], fill=col)
        d.polygon([(S * 0.3, S * 0.18), (S * 0.7, S * 0.18), (S * 0.6, S * 0.4), (S * 0.4, S * 0.4)], fill=lighten(col, 0.4) + (255,))
        d.polygon([(S * 0.4, S * 0.4), (S * 0.6, S * 0.4), (c, S * 0.9)], fill=lighten(col, 0.2) + (255,))
    elif name == "key":
        d.ellipse([S * 0.1, S * 0.3, S * 0.48, S * 0.68], fill=col)
        d.ellipse([S * 0.2, S * 0.4, S * 0.38, S * 0.58], fill=(0, 0, 0, 0))
        d.rectangle([S * 0.44, S * 0.44, S * 0.9, S * 0.54], fill=col)
        d.rectangle([S * 0.72, S * 0.54, S * 0.8, S * 0.68], fill=col)
        d.rectangle([S * 0.84, S * 0.54, S * 0.9, S * 0.64], fill=col)
    elif name == "skull":
        d.ellipse([S * 0.18, S * 0.1, S * 0.82, S * 0.7], fill=col)
        d.rectangle([S * 0.32, S * 0.6, S * 0.68, S * 0.86], fill=col)
        d.ellipse([S * 0.28, S * 0.36, S * 0.45, S * 0.53], fill=(30, 30, 30, 255))
        d.ellipse([S * 0.55, S * 0.36, S * 0.72, S * 0.53], fill=(30, 30, 30, 255))
        for x in (0.4, 0.5, 0.6):
            d.line([(S * x, S * 0.7), (S * x, S * 0.86)], fill=(30, 30, 30, 255), width=int(S * 0.03))
    elif name == "sword":
        d.polygon([(S * 0.82, S * 0.08), (S * 0.92, S * 0.18), (S * 0.4, S * 0.7), (S * 0.3, S * 0.6)], fill=col)
        d.line([(S * 0.22, S * 0.5), (S * 0.5, S * 0.78)], fill=(140, 90, 50, 255), width=int(S * 0.08))
        d.line([(S * 0.36, S * 0.64), (S * 0.12, S * 0.88)], fill=(110, 70, 40, 255), width=int(S * 0.08))
    elif name == "shield":
        d.polygon([(c, S * 0.08), (S * 0.86, S * 0.22), (S * 0.8, S * 0.6), (c, S * 0.92), (S * 0.2, S * 0.6), (S * 0.14, S * 0.22)], fill=col)
        d.polygon([(c, S * 0.2), (S * 0.74, S * 0.3), (S * 0.69, S * 0.58), (c, S * 0.8)], fill=lighten(col, 0.25) + (255,))
    elif name == "bolt":
        d.polygon([(S * 0.58, S * 0.06), (S * 0.2, S * 0.56), (S * 0.47, S * 0.56), (S * 0.38, S * 0.94), (S * 0.8, S * 0.4), (S * 0.53, S * 0.4)], fill=col)
    elif name == "flame":
        pts = []
        for i in range(120):
            t = i / 120 * 2 * math.pi
            x = math.sin(t) * (1 - math.cos(t)) * 0.5
            y = -math.cos(t)
            pts.append((c + x * S * 0.42, c + S * 0.06 - y * S * 0.4))
        pts = [(x, S - y + S * 0.04) for x, y in pts]
        d.polygon(pts, fill=col)
        d.polygon([(x * 0.55 + c * 0.45, (y - c) * 0.55 + c + S * 0.18) for x, y in pts], fill=PALETTE["yellow"] + (255,))
    elif name == "drop":
        shape(d, "drop", S // SS, col)
    elif name == "gear":
        d.polygon(star_pts(c, c, S * 0.44, S * 0.34, 8), fill=col)
        d.ellipse([S * 0.18, S * 0.18, S * 0.82, S * 0.82], fill=col)
        d.ellipse([S * 0.36, S * 0.36, S * 0.64, S * 0.64], fill=(0, 0, 0, 0))
    elif name == "home":
        d.polygon([(c, S * 0.1), (S * 0.9, S * 0.5), (S * 0.1, S * 0.5)], fill=col)
        d.rectangle([S * 0.2, S * 0.48, S * 0.8, S * 0.88], fill=col)
        d.rectangle([S * 0.42, S * 0.62, S * 0.58, S * 0.88], fill=dark)
    elif name == "play":
        d.polygon([(S * 0.25, S * 0.15), (S * 0.85, c), (S * 0.25, S * 0.85)], fill=col)
    elif name == "pause":
        d.rectangle([S * 0.22, S * 0.15, S * 0.42, S * 0.85], fill=col)
        d.rectangle([S * 0.58, S * 0.15, S * 0.78, S * 0.85], fill=col)
    elif name == "stop":
        d.rounded_rectangle([S * 0.18, S * 0.18, S * 0.82, S * 0.82], radius=S * 0.06, fill=col)
    elif name in ("plus", "minus", "close", "check"):
        t = int(S * 0.14)
        if name in ("plus", "minus"):
            d.line([(S * 0.15, c), (S * 0.85, c)], fill=col, width=t)
        if name == "plus":
            d.line([(c, S * 0.15), (c, S * 0.85)], fill=col, width=t)
        if name == "close":
            d.line([(S * 0.2, S * 0.2), (S * 0.8, S * 0.8)], fill=col, width=t)
            d.line([(S * 0.8, S * 0.2), (S * 0.2, S * 0.8)], fill=col, width=t)
        if name == "check":
            d.line([(S * 0.15, S * 0.52), (S * 0.4, S * 0.78), (S * 0.86, S * 0.24)], fill=col, width=t, joint="curve")
    elif name.startswith("arrow_"):
        pts = [(c, S * 0.1), (S * 0.88, c), (S * 0.64, c), (S * 0.64, S * 0.9), (S * 0.36, S * 0.9), (S * 0.36, c), (S * 0.12, c)]
        rot = {"arrow_up": 0, "arrow_right": 90, "arrow_down": 180, "arrow_left": 270}[name]
        a = math.radians(rot)
        d.polygon([(c + (x - c) * math.cos(a) - (y - c) * math.sin(a), c + (x - c) * math.sin(a) + (y - c) * math.cos(a)) for x, y in pts], fill=col)
    elif name == "music":
        d.ellipse([S * 0.12, S * 0.62, S * 0.4, S * 0.86], fill=col)
        d.ellipse([S * 0.56, S * 0.52, S * 0.84, S * 0.76], fill=col)
        d.rectangle([S * 0.32, S * 0.18, S * 0.4, S * 0.74], fill=col)
        d.rectangle([S * 0.76, S * 0.08, S * 0.84, S * 0.64], fill=col)
        d.polygon([(S * 0.32, S * 0.18), (S * 0.84, S * 0.08), (S * 0.84, S * 0.2), (S * 0.32, S * 0.3)], fill=col)
    elif name in ("sound", "mute"):
        d.polygon([(S * 0.1, S * 0.36), (S * 0.3, S * 0.36), (S * 0.52, S * 0.14), (S * 0.52, S * 0.86), (S * 0.3, S * 0.64), (S * 0.1, S * 0.64)], fill=col)
        if name == "sound":
            for r in (0.18, 0.3):
                d.arc([c + S * 0.04 - S * r, c - S * r, c + S * 0.04 + S * r, c + S * r], -50, 50, fill=col, width=int(S * 0.06))
        else:
            d.line([(S * 0.62, S * 0.34), (S * 0.9, S * 0.66)], fill=PALETTE["red"] + (255,), width=int(S * 0.08))
            d.line([(S * 0.9, S * 0.34), (S * 0.62, S * 0.66)], fill=PALETTE["red"] + (255,), width=int(S * 0.08))
    elif name == "lock":
        d.arc([S * 0.28, S * 0.1, S * 0.72, S * 0.6], 180, 360, fill=col, width=int(S * 0.09))
        d.line([(S * 0.32, S * 0.35), (S * 0.32, S * 0.45)], fill=col, width=int(S * 0.09))
        d.line([(S * 0.68, S * 0.35), (S * 0.68, S * 0.45)], fill=col, width=int(S * 0.09))
        d.rounded_rectangle([S * 0.18, S * 0.42, S * 0.82, S * 0.9], radius=S * 0.06, fill=col)
        d.ellipse([S * 0.45, S * 0.56, S * 0.55, S * 0.66], fill=dark)
    elif name == "flag":
        d.rectangle([S * 0.18, S * 0.08, S * 0.25, S * 0.92], fill=(120, 120, 125, 255))
        d.polygon([(S * 0.25, S * 0.1), (S * 0.85, S * 0.26), (S * 0.25, S * 0.46)], fill=col)
    elif name == "clock":
        d.ellipse([S * 0.08, S * 0.08, S * 0.92, S * 0.92], fill=W, outline=col, width=int(S * 0.07))
        d.line([(c, c), (c, S * 0.24)], fill=(40, 40, 40, 255), width=int(S * 0.06))
        d.line([(c, c), (S * 0.7, S * 0.6)], fill=(40, 40, 40, 255), width=int(S * 0.06))
    elif name in ("info", "warning"):
        if name == "info":
            d.ellipse([S * 0.08, S * 0.08, S * 0.92, S * 0.92], fill=col)
            d.ellipse([S * 0.44, S * 0.2, S * 0.56, S * 0.32], fill=W)
            d.rectangle([S * 0.44, S * 0.4, S * 0.56, S * 0.78], fill=W)
        else:
            d.polygon(poly(c, c + S * 0.08, S * 0.48, 3), fill=col)
            d.rectangle([S * 0.45, S * 0.34, S * 0.55, S * 0.64], fill=(40, 40, 40, 255))
            d.ellipse([S * 0.44, S * 0.7, S * 0.56, S * 0.82], fill=(40, 40, 40, 255))
    elif name == "potion":
        d.ellipse([S * 0.18, S * 0.36, S * 0.82, S * 0.92], fill=col)
        d.rectangle([S * 0.4, S * 0.12, S * 0.6, S * 0.42], fill=(210, 230, 240, 255))
        d.rectangle([S * 0.36, S * 0.06, S * 0.64, S * 0.14], fill=(140, 95, 60, 255))
        d.ellipse([S * 0.3, S * 0.48, S * 0.44, S * 0.6], fill=lighten(col, 0.5) + (255,))
    elif name == "bomb":
        d.ellipse([S * 0.12, S * 0.24, S * 0.84, S * 0.94], fill=col)
        d.rectangle([S * 0.5, S * 0.16, S * 0.64, S * 0.3], fill=(90, 90, 95, 255))
        d.line([(S * 0.58, S * 0.16), (S * 0.72, S * 0.04)], fill=(160, 120, 70, 255), width=int(S * 0.04))
        d.polygon(star_pts(S * 0.76, S * 0.05, S * 0.08, S * 0.03, 6), fill=PALETTE["orange"] + (255,))
    elif name == "trophy":
        d.polygon([(S * 0.22, S * 0.1), (S * 0.78, S * 0.1), (S * 0.7, S * 0.46), (c, S * 0.6), (S * 0.3, S * 0.46)], fill=col)
        d.arc([S * 0.06, S * 0.12, S * 0.34, S * 0.42], 90, 270, fill=col, width=int(S * 0.06))
        d.arc([S * 0.66, S * 0.12, S * 0.94, S * 0.42], -90, 90, fill=col, width=int(S * 0.06))
        d.rectangle([S * 0.45, S * 0.58, S * 0.55, S * 0.78], fill=col)
        d.rectangle([S * 0.28, S * 0.78, S * 0.72, S * 0.9], fill=darken(col, 0.2) + (255,))
    elif name == "eye":
        d.ellipse([S * 0.06, S * 0.26, S * 0.94, S * 0.74], fill=W, outline=col, width=int(S * 0.05))
        d.ellipse([S * 0.36, S * 0.3, S * 0.64, S * 0.7], fill=col)
        d.ellipse([S * 0.45, S * 0.42, S * 0.55, S * 0.58], fill=(20, 20, 20, 255))
    else:
        raise ValueError(name)


def make_icons():
    n = 64
    for name, col in ICON_COLORS.items():
        im, d = canvas(n)
        icon(d, name, n * SS, col)
        save(down(im, n), f"icons/{name}.png")
        im, d = canvas(n)
        icon(d, name, n * SS, (255, 255, 255))
        save(down(im, n), f"icons/white/{name}.png")


# ---------------------------------------------------------------- characters (simple stand-ins)
def face(d, S, cx, cy, s, mood="happy", eye=(25, 25, 30, 255)):
    d.ellipse([cx - s * 0.32 - s * 0.09, cy - s * 0.12, cx - s * 0.32 + s * 0.09, cy + s * 0.08], fill=eye)
    d.ellipse([cx + s * 0.32 - s * 0.09, cy - s * 0.12, cx + s * 0.32 + s * 0.09, cy + s * 0.08], fill=eye)
    if mood == "happy":
        d.arc([cx - s * 0.25, cy - s * 0.05, cx + s * 0.25, cy + s * 0.3], 20, 160, fill=eye, width=int(s * 0.07))
    elif mood == "angry":
        d.line([(cx - s * 0.45, cy - s * 0.25), (cx - s * 0.2, cy - s * 0.12)], fill=eye, width=int(s * 0.07))
        d.line([(cx + s * 0.45, cy - s * 0.25), (cx + s * 0.2, cy - s * 0.12)], fill=eye, width=int(s * 0.07))
        d.arc([cx - s * 0.22, cy + s * 0.15, cx + s * 0.22, cy + s * 0.4], 200, 340, fill=eye, width=int(s * 0.07))


def make_characters():
    n = 128
    S = n * SS
    c = S / 2
    specs = [("smiley", PALETTE["yellow"]), ("player", PALETTE["blue"]), ("enemy", PALETTE["red"]),
             ("slime", PALETTE["green"]), ("ghost", (235, 238, 245)), ("robot", PALETTE["gray"]),
             ("ship", (220, 230, 245)), ("ufo", PALETTE["purple"]), ("blob", PALETTE["pink"]),
             ("box_buddy", PALETTE["orange"]), ("tree", PALETTE["green"]), ("rock", PALETTE["gray"]),
             ("crate", (175, 125, 70)), ("chest", (160, 100, 50)), ("door", (140, 90, 55)), ("bush", (70, 150, 70))]
    for name, col in specs:
        im, d = canvas(n)
        k = col + (255,)
        dk = darken(col, 0.35) + (255,)
        if name == "smiley":
            d.ellipse([S * 0.06, S * 0.06, S * 0.94, S * 0.94], fill=k, outline=dk, width=SS * 3)
            face(d, S, c, c, S * 0.5)
        elif name == "player":
            d.rounded_rectangle([S * 0.18, S * 0.12, S * 0.82, S * 0.92], radius=S * 0.2, fill=k, outline=dk, width=SS * 3)
            face(d, S, c, S * 0.4, S * 0.4, eye=(255, 255, 255, 255))
        elif name == "enemy":
            d.polygon([(c, S * 0.08), (S * 0.92, S * 0.9), (S * 0.08, S * 0.9)], fill=k, outline=dk)
            face(d, S, c, S * 0.62, S * 0.36, mood="angry")
        elif name == "slime":
            d.pieslice([S * 0.06, S * 0.2, S * 0.94, S * 1.5], 180, 360, fill=k)
            d.rectangle([S * 0.06, S * 0.84, S * 0.94, S * 0.9], fill=k)
            d.ellipse([S * 0.24, S * 0.32, S * 0.38, S * 0.42], fill=lighten(col, 0.6) + (255,))
            face(d, S, c, S * 0.62, S * 0.42)
        elif name == "ghost":
            d.pieslice([S * 0.14, S * 0.06, S * 0.86, S * 0.8], 180, 360, fill=k)
            d.rectangle([S * 0.14, S * 0.42, S * 0.86, S * 0.8], fill=k)
            for i in range(4):
                x = S * 0.14 + i * S * 0.18
                d.ellipse([x, S * 0.72, x + S * 0.18, S * 0.92], fill=k)
            face(d, S, c, S * 0.42, S * 0.4)
        elif name == "robot":
            d.rounded_rectangle([S * 0.14, S * 0.22, S * 0.86, S * 0.86], radius=S * 0.08, fill=k, outline=dk, width=SS * 3)
            d.line([(c, S * 0.22), (c, S * 0.08)], fill=dk, width=SS * 3)
            d.ellipse([c - S * 0.05, S * 0.04, c + S * 0.05, S * 0.14], fill=PALETTE["red"] + (255,))
            d.rounded_rectangle([S * 0.24, S * 0.36, S * 0.76, S * 0.58], radius=S * 0.05, fill=(30, 35, 45, 255))
            d.ellipse([S * 0.32, S * 0.42, S * 0.42, S * 0.52], fill=PALETTE["cyan"] + (255,))
            d.ellipse([S * 0.58, S * 0.42, S * 0.68, S * 0.52], fill=PALETTE["cyan"] + (255,))
            for i in range(4):
                d.rectangle([S * (0.3 + i * 0.11), S * 0.68, S * (0.36 + i * 0.11), S * 0.76], fill=dk)
        elif name == "ship":
            d.polygon([(c, S * 0.06), (S * 0.86, S * 0.9), (c, S * 0.72), (S * 0.14, S * 0.9)], fill=k, outline=dk)
            d.ellipse([c - S * 0.08, S * 0.36, c + S * 0.08, S * 0.56], fill=PALETTE["cyan"] + (255,))
        elif name == "ufo":
            d.ellipse([S * 0.32, S * 0.2, S * 0.68, S * 0.56], fill=(170, 230, 250, 200))
            d.ellipse([S * 0.06, S * 0.42, S * 0.94, S * 0.7], fill=k, outline=dk, width=SS * 3)
            for x in (0.25, 0.42, 0.58, 0.75):
                d.ellipse([S * x - S * 0.04, S * 0.52, S * x + S * 0.04, S * 0.6], fill=PALETTE["yellow"] + (255,))
        elif name == "blob":
            pts = [(c + math.cos(t / 30 * 2 * math.pi) * S * (0.4 + 0.05 * math.sin(t * 0.9)), c + math.sin(t / 30 * 2 * math.pi) * S * (0.38 + 0.05 * math.cos(t * 1.3))) for t in range(30)]
            d.polygon(pts, fill=k, outline=dk)
            face(d, S, c, c, S * 0.42)
        elif name == "box_buddy":
            d.rectangle([S * 0.14, S * 0.14, S * 0.86, S * 0.86], fill=k, outline=dk, width=SS * 4)
            face(d, S, c, c, S * 0.5)
        elif name == "tree":
            d.rectangle([S * 0.44, S * 0.6, S * 0.56, S * 0.94], fill=(120, 80, 45, 255))
            d.polygon([(c, S * 0.04), (S * 0.86, S * 0.66), (S * 0.14, S * 0.66)], fill=k, outline=dk)
        elif name == "rock":
            d.polygon([(S * 0.2, S * 0.88), (S * 0.08, S * 0.6), (S * 0.26, S * 0.3), (S * 0.56, S * 0.22), (S * 0.86, S * 0.42), (S * 0.92, S * 0.88)], fill=k, outline=dk)
            d.polygon([(S * 0.3, S * 0.36), (S * 0.54, S * 0.3), (S * 0.46, S * 0.5)], fill=lighten(col, 0.3) + (255,))
        elif name == "crate":
            d.rectangle([S * 0.08, S * 0.08, S * 0.92, S * 0.92], fill=k, outline=dk, width=SS * 4)
            d.rectangle([S * 0.16, S * 0.16, S * 0.84, S * 0.84], outline=dk, width=SS * 3)
            d.line([(S * 0.16, S * 0.16), (S * 0.84, S * 0.84)], fill=dk, width=SS * 4)
        elif name == "chest":
            d.rounded_rectangle([S * 0.08, S * 0.26, S * 0.92, S * 0.88], radius=S * 0.06, fill=k, outline=dk, width=SS * 3)
            d.line([(S * 0.08, S * 0.5), (S * 0.92, S * 0.5)], fill=dk, width=SS * 3)
            d.rectangle([S * 0.44, S * 0.44, S * 0.56, S * 0.6], fill=(230, 185, 60, 255))
        elif name == "door":
            d.rounded_rectangle([S * 0.2, S * 0.06, S * 0.8, S * 0.94], radius=S * 0.08, fill=k, outline=dk, width=SS * 3)
            d.ellipse([S * 0.64, S * 0.5, S * 0.72, S * 0.58], fill=(230, 185, 60, 255))
        elif name == "bush":
            for (x, y, r) in [(0.3, 0.6, 0.24), (0.7, 0.6, 0.24), (0.5, 0.42, 0.28)]:
                d.ellipse([S * (x - r), S * (y - r), S * (x + r), S * (y + r)], fill=k)
            d.rectangle([S * 0.1, S * 0.6, S * 0.9, S * 0.84], fill=k)
        save(down(im, n), f"characters/{name}.png")


# ---------------------------------------------------------------- particles
def make_particles():
    n = 64
    def radial(fn, name):
        im = Image.new("RGBA", (n, n))
        px = im.load()
        for y in range(n):
            for x in range(n):
                d = math.hypot(x - (n - 1) / 2, y - (n - 1) / 2) / (n / 2)
                px[x, y] = (255, 255, 255, max(0, min(255, int(255 * fn(d, x, y)))))
        save(im, f"particles/{name}.png")
    radial(lambda d, x, y: max(0, 1 - d) ** 2, "glow")
    radial(lambda d, x, y: max(0, 1 - d) ** 0.6 if d < 1 else 0, "soft_circle")
    radial(lambda d, x, y: 1 if d < 0.9 else 0, "dot")
    radial(lambda d, x, y: max(0, 1 - abs(d - 0.75) * 8), "ring")
    rnd = random.Random(5)
    blobs = [(rnd.uniform(18, 46), rnd.uniform(18, 46), rnd.uniform(10, 18)) for _ in range(7)]
    radial(lambda d, x, y: min(1, sum(max(0, 1 - math.hypot(x - bx, y - by) / br) for bx, by, br in blobs)) * 0.85, "smoke")
    im, dr = canvas(n)
    dr.polygon(star_pts(n * SS / 2, n * SS / 2, n * SS * 0.48, n * SS * 0.08, 4), fill=(255, 255, 255, 255))
    save(down(im, n).filter(ImageFilter.GaussianBlur(0.6)), "particles/sparkle.png")
    im, dr = canvas(n)
    dr.polygon(star_pts(n * SS / 2, n * SS / 2, n * SS * 0.46, n * SS * 0.2, 5), fill=(255, 255, 255, 255))
    save(down(im, n), "particles/star.png")
    save(Image.new("RGBA", (8, 8), (255, 255, 255, 255)), "particles/square.png")
    im = Image.new("RGBA", (64, 16))
    px = im.load()
    for y in range(16):
        for x in range(64):
            a = max(0.0, 1 - abs(y - 7.5) / 8) * (x / 63)
            px[x, y] = (255, 255, 255, int(255 * a))
    save(im, "particles/streak.png")
    im, dr = canvas(n)
    dr.polygon([(n * SS * 0.5, 0), (n * SS * 0.65, n * SS * 0.5), (n * SS * 0.5, n * SS), (n * SS * 0.35, n * SS * 0.5)], fill=(255, 255, 255, 255))
    save(down(im, n), "particles/shard.png")


# ---------------------------------------------------------------- skies / backdrops
def make_skies():
    w, h = 512, 256
    for name, (top, mid, bot) in {"day": ((70, 130, 220), (150, 200, 245), (225, 240, 250)),
                                  "sunset": ((50, 40, 110), (230, 110, 90), (250, 200, 120)),
                                  "night": ((5, 8, 25), (20, 25, 60), (45, 50, 90)),
                                  "overcast": ((120, 128, 140), (165, 172, 182), (200, 205, 212)),
                                  "alien": ((40, 10, 60), (130, 40, 120), (240, 140, 120))}.items():
        im = Image.new("RGBA", (w, h))
        d = ImageDraw.Draw(im)
        for y in range(h):
            t = y / (h - 1)
            if t < 0.6:
                k = t / 0.6
                c = tuple(int(top[i] + (mid[i] - top[i]) * k) for i in range(3))
            else:
                k = (t - 0.6) / 0.4
                c = tuple(int(mid[i] + (bot[i] - mid[i]) * k) for i in range(3))
            d.line([(0, y), (w, y)], fill=c + (255,))
        rnd = random.Random(hash(name) & 0xffff)
        if name == "night":
            for _ in range(160):
                x, y = rnd.randrange(w), rnd.randrange(int(h * 0.75))
                b = rnd.randrange(150, 255)
                im.putpixel((x, y), (b, b, b, 255))
        if name in ("day", "overcast"):
            clouds = Image.new("RGBA", im.size, (255, 255, 255, 0))
            cd = ImageDraw.Draw(clouds)
            for _ in range(7):
                cx, cy = rnd.randrange(w), rnd.randrange(30, 150)
                for k in range(5):
                    r = rnd.randrange(14, 30)
                    ox = rnd.randrange(-40, 40)
                    cd.ellipse([cx + ox - r * 1.6, cy - r * 0.8, cx + ox + r * 1.6, cy + r * 0.8], fill=(255, 255, 255, 255))
            clouds = clouds.filter(ImageFilter.GaussianBlur(3))
            clouds.putalpha(clouds.getchannel("A").point(lambda a: int(a * 0.7)))
            im = Image.alpha_composite(im, clouds)
        save(im, f"skies/{name}.png")
    # Parallax hills strips (tileable horizontally)
    for name, col in {"green": (80, 160, 85), "blue": (90, 120, 170), "purple": (110, 80, 150), "sand": (210, 180, 120), "dark": (40, 45, 60)}.items():
        w, h = 512, 160
        im, d = Image.new("RGBA", (w * 2, h * 2), (0, 0, 0, 0)), None
        d = ImageDraw.Draw(im)
        pts = [(0, h * 2)]
        for x in range(0, w * 2 + 1, 4):
            y = h * 2 * (0.45 + 0.15 * math.sin(x / (w * 2) * 2 * math.pi * 2) + 0.08 * math.sin(x / (w * 2) * 2 * math.pi * 5 + 1))
            pts.append((x, y))
        pts.append((w * 2, h * 2))
        d.polygon(pts, fill=col + (255,))
        save(im.resize((w, h), Image.LANCZOS), f"skies/hills_{name}.png")


def main():
    if os.path.isdir(OUT):
        shutil.rmtree(OUT)
    os.makedirs(OUT)
    make_shapes()
    make_patterns()
    make_colors()
    make_ui()
    make_icons()
    make_characters()
    make_particles()
    make_skies()
    with open(os.path.join(OUT, "README.txt"), "w") as f:
        f.write("GEX built-in placeholder pack (gex://). Made by tools/make_placeholders.py.\n"
                "Free to use in any game, no credit needed.\n")
    print(f"{COUNT} files in {os.path.normpath(OUT)}")


if __name__ == "__main__":
    sys.exit(main())
