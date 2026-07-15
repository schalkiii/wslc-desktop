#!/usr/bin/env python3
"""Generate the wslc-desktop app icon.

Design: a rounded-square tile with a diagonal teal -> violet gradient (matching
the app's CPU/Mem plot accent colors), stamped with a clean white terminal
prompt mark ">_". Reads as a modern GUI-for-a-CLI tool and stays legible down
to 16x16.

Outputs (into this directory):
  icon.png    256x256 window icon consumed by eframe at runtime
  icon@512.png 512x512 for the README / release page
  icon.ico    multi-size Windows icon (256/128/64/48/32/16) embedded in the exe
"""
from __future__ import annotations

import os

from PIL import Image, ImageDraw

HERE = os.path.dirname(os.path.abspath(__file__))

# Accent colors lifted from the egui plots: CPU teal + Mem magenta.
TEAL = (0x2B, 0xC2, 0xD2)
VIOLET = (0xC0, 0x55, 0xC8)
SS = 8  # supersample factor for crisp anti-aliasing


def lerp(a: int, b: int, t: float) -> int:
    return round(a + (b - a) * t)


def diagonal_gradient(size: int) -> Image.Image:
    """Top-left TEAL -> bottom-right VIOLET diagonal gradient."""
    grad = Image.new("RGB", (size, size))
    px = grad.load()
    denom = 2 * (size - 1)
    for y in range(size):
        for x in range(size):
            t = (x + y) / denom
            px[x, y] = (
                lerp(TEAL[0], VIOLET[0], t),
                lerp(TEAL[1], VIOLET[1], t),
                lerp(TEAL[2], VIOLET[2], t),
            )
    return grad


def rounded_mask(size: int, radius: int) -> Image.Image:
    mask = Image.new("L", (size, size), 0)
    d = ImageDraw.Draw(mask)
    d.rounded_rectangle([0, 0, size - 1, size - 1], radius=radius, fill=255)
    return mask


def build_tile(px: int) -> Image.Image:
    """Render one square icon tile at `px` pixels."""
    size = px * SS
    tile = diagonal_gradient(size).convert("RGBA")

    # Soft top highlight for a little depth.
    hi = Image.new("L", (size, size), 0)
    ImageDraw.Draw(hi).ellipse(
        [-size * 0.35, -size * 0.75, size * 1.1, size * 0.55], fill=42
    )
    tile = Image.alpha_composite(
        tile, Image.merge("RGBA", (*[Image.new("L", (size, size), 255)] * 3, hi))
    )

    # Foreground mark: ">_" terminal prompt, drawn as thick rounded strokes.
    fg = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(fg)
    white = (255, 255, 255, 255)
    stroke = int(size * 0.085)

    # Chevron ">"  (apex on the right).
    cx, cy = size * 0.34, size * 0.46
    arm = size * 0.20
    apex = (cx + arm, cy)
    top = (cx - arm * 0.55, cy - arm)
    bot = (cx - arm * 0.55, cy + arm)
    d.line([top, apex], fill=white, width=stroke, joint="curve")
    d.line([apex, bot], fill=white, width=stroke, joint="curve")
    for pt in (top, apex, bot):
        r = stroke / 2
        d.ellipse([pt[0] - r, pt[1] - r, pt[0] + r, pt[1] + r], fill=white)

    # Underscore "_" cursor to the lower right.
    ux0, ux1 = size * 0.52, size * 0.74
    uy = size * 0.66
    d.rounded_rectangle(
        [ux0, uy - stroke / 2, ux1, uy + stroke / 2],
        radius=stroke / 2,
        fill=white,
    )

    tile = Image.alpha_composite(tile, fg)

    # Clip everything to the rounded-square silhouette.
    mask = rounded_mask(size, radius=int(size * 0.225))
    tile.putalpha(mask)

    return tile.resize((px, px), Image.LANCZOS)


def main() -> None:
    icon256 = build_tile(256)
    icon256.save(os.path.join(HERE, "icon.png"))
    build_tile(512).save(os.path.join(HERE, "icon@512.png"))

    # Raw 256x256 RGBA bytes for the eframe window icon (no decode dependency).
    with open(os.path.join(HERE, "icon_rgba.bin"), "wb") as fh:
        fh.write(icon256.convert("RGBA").tobytes())

    sizes = [256, 128, 64, 48, 32, 16]
    imgs = [build_tile(s) for s in sizes]
    imgs[0].save(
        os.path.join(HERE, "icon.ico"),
        format="ICO",
        sizes=[(s, s) for s in sizes],
        append_images=imgs[1:],
    )
    print("wrote icon.png, icon@512.png, icon.ico")


if __name__ == "__main__":
    main()
