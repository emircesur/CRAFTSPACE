#!/usr/bin/env python3
"""Builds CraftSpace's own add-on packs into addons/packs/ (zip files, byte-for-byte reproducible).

    python3 addons/build.py            # writes the packs and prints their SHA-256
    python3 addons/build.py --check    # fails if the committed packs differ from a fresh build

Everything in them is original and generated here (CC0): colour palettes, 3D LUT looks and
LightCraft develop presets. A pack's file name carries its version; a pack is never changed
after it's published (the registry pins its SHA-256), a new version gets a new name.
"""

import colorsys
import hashlib
import io
import json
import math
import pathlib
import struct
import sys
import zipfile

HERE = pathlib.Path(__file__).resolve().parent
OUT = HERE / "packs"
STAMP = (2026, 10, 1, 0, 0, 0)
LICENSE = """These files were made for CraftSpace and are dedicated to the public domain (CC0 1.0):
https://creativecommons.org/publicdomain/zero/1.0/
Use them for anything, no credit needed.
"""


def zip_bytes(files):
    """A zip whose bytes depend only on `files` (name -> bytes)."""
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as z:
        for name in sorted(files):
            info = zipfile.ZipInfo(name, STAMP)
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o644 << 16
            z.writestr(info, files[name])
    return buf.getvalue()


# ---- palettes ------------------------------------------------------------------------------


def hsl(h, s, l):
    r, g, b = colorsys.hls_to_rgb((h % 360) / 360, l, s)
    return (round(r * 255), round(g * 255), round(b * 255))


def palettes():
    pals = {}
    hues = [(0, "Red"), (25, "Vermilion"), (40, "Orange"), (52, "Amber"), (60, "Yellow"), (90, "Lime"),
            (130, "Green"), (165, "Teal"), (195, "Cyan"), (215, "Blue"), (255, "Indigo"), (285, "Violet"),
            (320, "Magenta"), (345, "Rose")]
    spectrum = []
    for h, name in hues:
        for i, l in enumerate([0.88, 0.74, 0.58, 0.44, 0.30, 0.18]):
            spectrum.append((f"{name} {i + 1}", hsl(h, 0.78 if 0.2 < l < 0.8 else 0.6, l)))
    pals["CraftSpace Spectrum"] = spectrum
    greys = [(f"Grey {round(100 * i / 15)}%", (round(255 * i / 15),) * 3) for i in range(16)]
    warm = [(f"Warm grey {i + 1}", hsl(35, 0.10, 0.92 - i * 0.11)) for i in range(8)]
    cool = [(f"Cool grey {i + 1}", hsl(215, 0.10, 0.92 - i * 0.11)) for i in range(8)]
    pals["CraftSpace Neutrals"] = greys + warm + cool
    pals["CraftSpace Pastels"] = [(f"Pastel {name}", hsl(h, 0.62, 0.84)) for h, name in hues]
    earth = [("Clay", hsl(18, 0.45, 0.45)), ("Terracotta", hsl(14, 0.55, 0.52)), ("Ochre", hsl(40, 0.70, 0.48)),
             ("Sand", hsl(38, 0.45, 0.74)), ("Umber", hsl(28, 0.40, 0.28)), ("Sienna", hsl(20, 0.55, 0.36)),
             ("Olive", hsl(70, 0.35, 0.36)), ("Moss", hsl(85, 0.30, 0.42)), ("Sage", hsl(95, 0.18, 0.62)),
             ("Bark", hsl(25, 0.30, 0.20)), ("Wheat", hsl(42, 0.55, 0.80)), ("Slate", hsl(205, 0.12, 0.40))]
    pals["CraftSpace Earth"] = earth
    pals["CraftSpace Ocean"] = [(f"Ocean {i + 1}", hsl(185 + i * 4, 0.55 + 0.02 * (i % 3), 0.86 - i * 0.065)) for i in range(12)]
    skin = []
    for i, l in enumerate([0.88, 0.80, 0.72, 0.64, 0.56, 0.48, 0.40, 0.32, 0.25, 0.19]):
        for j, (h, s) in enumerate([(28, 0.55), (22, 0.45), (32, 0.40)]):
            skin.append((f"Skin {i + 1}{'abc'[j]}", hsl(h, s, l)))
    pals["CraftSpace Skin Tones"] = skin
    web = []
    for r in range(0, 256, 51):
        for g in range(0, 256, 51):
            for b in range(0, 256, 51):
                web.append((f"#{r:02X}{g:02X}{b:02X}", (r, g, b)))
    pals["Web Safe 216"] = web
    return pals


def gpl(name, colors):
    lines = ["GIMP Palette", f"Name: {name}", "Columns: 12", "#"]
    lines += [f"{r:3d} {g:3d} {b:3d}\t{n}" for n, (r, g, b) in colors]
    return ("\n".join(lines) + "\n").encode()


def ase(name, colors):
    """Adobe Swatch Exchange: one group of RGB swatches."""
    def ustr(s):
        s += "\0"
        return struct.pack(">H", len(s)) + s.encode("utf-16-be")

    blocks = [(0xC001, ustr(name))]
    for n, (r, g, b) in colors:
        body = ustr(n) + b"RGB " + struct.pack(">fff", r / 255, g / 255, b / 255) + struct.pack(">H", 2)
        blocks.append((0x0001, body))
    blocks.append((0xC002, b""))
    out = b"ASEF" + struct.pack(">HHI", 1, 0, len(blocks))
    for kind, body in blocks:
        out += struct.pack(">HI", kind, len(body)) + body
    return out


def aco(colors):
    """Photoshop swatches, version 1 then version 2 (with names)."""
    v1 = struct.pack(">HH", 1, len(colors))
    v2 = struct.pack(">HH", 2, len(colors))
    for n, (r, g, b) in colors:
        rgb = struct.pack(">HHHHH", 0, r * 257, g * 257, b * 257, 0)
        v1 += rgb
        name = n + "\0"
        v2 += rgb + struct.pack(">I", len(name)) + name.encode("utf-16-be")
    return v1 + v2


def palette_pack():
    files = {"LICENSE.txt": LICENSE.encode()}
    for name, colors in palettes().items():
        files[f"GIMP palettes/{name}.gpl"] = gpl(name, colors)
        files[f"Adobe swatch exchange/{name}.ase"] = ase(name, colors)
        files[f"Photoshop swatches/{name}.aco"] = aco(colors)
    files["README.txt"] = (
        "CraftSpace Palettes: seven palettes in three formats.\n\n"
        "- GIMP palettes (.gpl): VectorCraft lists them under Swatches > User Defined.\n"
        "- Adobe swatch exchange (.ase): PhotoCraft, DesignCraft and VectorCraft.\n"
        "- Photoshop swatches (.aco): PhotoCraft.\n"
    ).encode()
    return files


# ---- LUT looks -----------------------------------------------------------------------------


def clamp(x):
    return 0.0 if x < 0 else 1.0 if x > 1 else x


def luma(r, g, b):
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def mix(a, b, t):
    return a + (b - a) * t


def scurve(x, k):
    """Contrast around mid-grey; k > 0 adds contrast."""
    return clamp(0.5 + (x - 0.5) * (1 + k) - k * 4 * (x - 0.5) ** 3)


def sat(rgb, s):
    y = luma(*rgb)
    return tuple(clamp(y + (c - y) * s) for c in rgb)


def tint(rgb, shadow, highlight, amount):
    y = luma(*rgb)
    return tuple(clamp(c + amount * ((1 - y) * sh + y * hi)) for c, sh, hi in zip(rgb, shadow, highlight))


def looks():
    def warm(rgb):
        r, g, b = rgb
        return sat((clamp(r * 1.06 + 0.02), clamp(g * 1.01 + 0.01), clamp(b * 0.90)), 1.05)

    def cool(rgb):
        r, g, b = rgb
        return sat((clamp(r * 0.92), clamp(g * 0.99 + 0.01), clamp(b * 1.06 + 0.02)), 0.95)

    def teal_orange(rgb):
        rgb = tint(rgb, (-0.10, 0.03, 0.10), (0.10, 0.03, -0.08), 0.6)
        return tuple(scurve(c, 0.15) for c in sat(rgb, 1.1))

    def faded(rgb):
        rgb = tuple(0.08 + c * 0.86 for c in rgb)
        return sat(tint(rgb, (0.02, 0.0, 0.04), (0.04, 0.02, -0.02), 0.5), 0.8)

    def bleach(rgb):
        y = luma(*rgb)
        mixed = tuple(mix(c, y, 0.55) for c in rgb)
        return tuple(scurve(c, 0.45) for c in mixed)

    def sepia(rgb):
        y = scurve(luma(*rgb), 0.1)
        return (clamp(y * 1.07 + 0.03), clamp(y * 0.96 + 0.01), clamp(y * 0.78))

    def cross(rgb):
        r, g, b = rgb
        return (clamp(scurve(r, 0.35) * 1.02), clamp(scurve(g, 0.25) * 1.03), clamp(0.12 + b * 0.75))

    def moonlight(rgb):
        r, g, b = sat(rgb, 0.55)
        return tuple(clamp(c * 0.85) for c in (r * 0.88, g * 0.97, b * 1.12 + 0.03))

    def golden(rgb):
        rgb = tint(rgb, (0.04, 0.01, -0.05), (0.10, 0.05, -0.08), 0.7)
        return tuple(scurve(c, 0.08) for c in rgb)

    def vivid(rgb):
        return tuple(scurve(c, 0.2) for c in sat(rgb, 1.35))

    def matte_mono(rgb):
        y = luma(*rgb)
        y = 0.07 + scurve(y, 0.15) * 0.87
        return (y, y, y)

    def high_mono(rgb):
        r, g, b = rgb
        y = clamp(0.35 * r + 0.55 * g + 0.10 * b)
        y = scurve(y, 0.6)
        return (y, y, y)

    return {
        "Warm Sun": warm,
        "Cool Shade": cool,
        "Teal and Orange": teal_orange,
        "Faded Film": faded,
        "Bleach Bypass": bleach,
        "Sepia": sepia,
        "Cross Process": cross,
        "Moonlight": moonlight,
        "Golden Hour": golden,
        "Vivid": vivid,
        "Matte Mono": matte_mono,
        "High Contrast Mono": high_mono,
    }


def cube(name, fn, size=33):
    lines = [f'TITLE "{name}"', "# Made for CraftSpace, CC0 1.0", f"LUT_3D_SIZE {size}", "DOMAIN_MIN 0.0 0.0 0.0", "DOMAIN_MAX 1.0 1.0 1.0"]
    n = size - 1
    # Red changes fastest.
    for b in range(size):
        for g in range(size):
            for r in range(size):
                out = fn((r / n, g / n, b / n))
                lines.append(" ".join(f"{clamp(c):.5f}" for c in out))
    return ("\n".join(lines) + "\n").encode()


def looks_pack():
    files = {"LICENSE.txt": LICENSE.encode()}
    for name, fn in looks().items():
        files[f"CraftSpace Looks/{name}.cube"] = cube(name, fn)
    files["README.txt"] = (
        "CraftSpace Looks: twelve 33-point 3D LUTs (.cube).\n\n"
        "- LightCraft: File > Import Profiles & Presets..., choose the CraftSpace Looks folder.\n"
        "  They appear as creative profiles.\n"
        "- FilmCraft: Lumetri Color > Creative > Look > Browse...\n"
        "- EffectCraft: Effect > Utility > Apply Color LUT.\n"
        "- PhotoCraft: Image > Adjustments > Color Lookup..., choose a 3D LUT file.\n"
    ).encode()
    return files


# ---- LightCraft presets ----------------------------------------------------------------------


def lightcraft_presets():
    def p(group, slug, name, settings):
        return {"id": f"craftspace.{slug}", "name": name, "group": f"CraftSpace {group}", "settings": settings}

    presets = [
        p("Portrait", "soft-skin", "Soft Skin", {"light": {"exposure": 0.15, "contrast": -8.0, "shadows": 18.0}, "effects": {"texture": -20.0, "clarity": -6.0}, "mixer": {"orange": {"sat": -6.0, "lum": 8.0}}}),
        p("Portrait", "warm-portrait", "Warm Portrait", {"wb": {"mode": "custom", "temp": 6200.0, "tint": 4.0}, "light": {"contrast": 6.0, "highlights": -18.0, "shadows": 12.0}, "effects": {"texture": -8.0}, "vignette": {"amount": -12.0}}),
        p("Portrait", "editorial", "Editorial", {"light": {"contrast": 18.0, "highlights": -25.0, "whites": 8.0, "blacks": -12.0}, "color": {"saturation": -12.0}, "effects": {"clarity": 10.0}}),
        p("Portrait", "window-light", "Window Light", {"light": {"exposure": 0.2, "highlights": -35.0, "shadows": 25.0}, "grading": {"highlights": {"hue": 45.0, "sat": 10.0, "lum": 0.0}}, "color": {"vibrance": 6.0}}),
        p("Landscape", "clear-day", "Clear Day", {"light": {"contrast": 12.0, "highlights": -30.0, "shadows": 20.0}, "effects": {"dehaze": 12.0, "clarity": 12.0}, "color": {"vibrance": 18.0}, "mixer": {"blue": {"sat": 10.0, "lum": -12.0}}}),
        p("Landscape", "misty-morning", "Misty Morning", {"light": {"contrast": -18.0, "highlights": -10.0, "blacks": 15.0}, "effects": {"dehaze": -15.0, "clarity": -10.0}, "grading": {"shadows": {"hue": 210.0, "sat": 10.0, "lum": 0.0}}}),
        p("Landscape", "deep-green", "Deep Green", {"mixer": {"green": {"hue": 10.0, "sat": 15.0, "lum": -10.0}, "yellow": {"hue": 8.0, "sat": 8.0}}, "light": {"contrast": 10.0}, "effects": {"clarity": 8.0}}),
        p("Landscape", "dusk", "Dusk", {"wb": {"mode": "custom", "temp": 7000.0, "tint": 12.0}, "light": {"exposure": -0.2, "highlights": -25.0, "contrast": 14.0}, "grading": {"shadows": {"hue": 250.0, "sat": 18.0, "lum": 0.0}, "highlights": {"hue": 30.0, "sat": 20.0, "lum": 0.0}}}),
        p("Street", "concrete", "Concrete", {"color": {"saturation": -30.0, "vibrance": 5.0}, "light": {"contrast": 22.0, "blacks": -10.0}, "effects": {"clarity": 18.0, "texture": 10.0}}),
        p("Street", "night-neon", "Night Neon", {"light": {"exposure": 0.1, "highlights": -40.0, "shadows": 15.0, "blacks": -10.0}, "color": {"vibrance": 25.0}, "mixer": {"purple": {"sat": 20.0}, "aqua": {"sat": 15.0}}, "grading": {"shadows": {"hue": 230.0, "sat": 15.0, "lum": 0.0}}}),
        p("Street", "faded-print", "Faded Print", {"curve": {"shadows": 30.0, "highlights": -10.0}, "color": {"saturation": -15.0}, "grain": {"amount": 20.0, "size": 25.0, "roughness": 50.0}}),
        p("Street", "warm-tungsten", "Warm Tungsten", {"wb": {"mode": "custom", "temp": 6800.0, "tint": 6.0}, "light": {"contrast": 10.0}, "grading": {"midtones": {"hue": 35.0, "sat": 8.0, "lum": 0.0}}, "vignette": {"amount": -18.0}}),
        p("B&W", "silver", "Silver", {"treatment": "bw", "light": {"contrast": 25.0, "highlights": -15.0, "whites": 12.0, "blacks": -15.0}, "effects": {"clarity": 12.0}}),
        p("B&W", "charcoal", "Charcoal", {"treatment": "bw", "light": {"exposure": -0.2, "contrast": 35.0, "blacks": -25.0}, "grain": {"amount": 25.0, "size": 30.0, "roughness": 60.0}, "vignette": {"amount": -20.0}}),
        p("B&W", "paper-white", "Paper White", {"treatment": "bw", "light": {"exposure": 0.3, "contrast": -10.0, "shadows": 30.0, "blacks": 15.0}}),
        p("B&W", "warm-tone", "Warm Tone", {"treatment": "bw", "light": {"contrast": 15.0}, "grading": {"highlights": {"hue": 40.0, "sat": 12.0, "lum": 0.0}, "shadows": {"hue": 30.0, "sat": 8.0, "lum": 0.0}}}),
    ]
    doc = {"format": "lightcraft.preset", "version": 1, "presets": presets}
    files = {
        "LICENSE.txt": LICENSE.encode(),
        "CraftSpace Presets.lcpreset": (json.dumps(doc, indent=2) + "\n").encode(),
        "README.txt": b"Sixteen LightCraft develop presets in four groups (Portrait, Landscape, Street, B&W).\n\n"
        b"In LightCraft: File > Import Profiles & Presets..., then choose CraftSpace Presets.lcpreset.\n",
    }
    return files


PACKS = {
    "craftspace-palettes-1.zip": palette_pack,
    "craftspace-looks-1.zip": looks_pack,
    "craftspace-lightcraft-presets-1.zip": lightcraft_presets,
}


def main():
    check = "--check" in sys.argv
    OUT.mkdir(exist_ok=True)
    bad = False
    for name, make in PACKS.items():
        data = zip_bytes(make())
        path = OUT / name
        sha = hashlib.sha256(data).hexdigest()
        if check:
            if not path.exists() or path.read_bytes() != data:
                print(f"{name}: differs from a fresh build", file=sys.stderr)
                bad = True
            continue
        if path.exists() and path.read_bytes() != data:
            print(f"{name} exists with other contents; published packs never change, bump the version", file=sys.stderr)
            bad = True
            continue
        path.write_bytes(data)
        print(f"{name}  {len(data):>8}  {sha}")
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
