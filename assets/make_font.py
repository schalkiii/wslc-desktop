#!/usr/bin/env python3
"""Subset Noto Sans SC to a compact bundle for wslc-desktop.

The full Noto Sans SC OTF is ~8.4 MB, which would bloat the self-contained
executable. This script keeps only the glyphs the UI actually needs:

  * ASCII + Latin-1 (English UI text, command lines, paths)
  * CJK / fullwidth punctuation and symbols
  * Every hanzi expressible in GB2312 (~6763 chars, >99.7% real-world coverage)

The result (assets/fonts/NotoSansSC-Subset.otf) is embedded via include_bytes!
in src/app.rs and registered with egui so Chinese renders instead of tofu (□).

Run once with a Python that has fonttools installed:
    python assets/make_font.py
"""
from __future__ import annotations

import os

from fontTools import subset

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = r"C:\Windows\Fonts\Noto Sans SC (TrueType).otf"
DST = os.path.join(HERE, "fonts", "NotoSansSC-Subset.otf")


def wanted_unicodes() -> set[int]:
    codes: set[int] = set()

    # Basic Latin + Latin-1 supplement (covers English UI and most symbols).
    codes.update(range(0x0020, 0x00FF + 1))

    # General punctuation, CJK symbols/punctuation, fullwidth forms.
    codes.update(range(0x2000, 0x206F + 1))  # general punctuation …—“”‘’
    codes.update(range(0x3000, 0x303F + 1))  # CJK punctuation 、。《》
    codes.update(range(0xFF00, 0xFFEF + 1))  # fullwidth ！？（）
    codes.update(range(0x2460, 0x24FF + 1))  # enclosed alphanumerics ①②

    # Every hanzi (and symbol) that GB2312 can encode: the practical set of
    # common simplified Chinese characters. We discover them by round-tripping
    # every GB2312 double-byte code point back to Unicode.
    for hi in range(0xA1, 0xFE + 1):
        for lo in range(0xA1, 0xFE + 1):
            try:
                ch = bytes([hi, lo]).decode("gb2312")
            except UnicodeDecodeError:
                continue
            codes.add(ord(ch))

    # Geometric-shape glyphs used by sort indicators and the start/run button.
    # These live in Noto Sans SC (so they render from the bundled font on any
    # host); the remaining action glyphs (stop/restart/restore/edit/etc.) are
    # real emoji and render via egui's bundled emoji font.
    codes.update(
        {
            0x25B2,  # ▲  sort ascending
            0x25BC,  # ▼  sort descending
            0x25B6,  # ▶  start / run
        }
    )

    return codes


def main() -> None:
    os.makedirs(os.path.dirname(DST), exist_ok=True)
    unicodes = wanted_unicodes()
    print(f"subsetting {len(unicodes)} code points from {SRC}")

    options = subset.Options()
    options.desubroutinize = True
    options.recalc_bounds = True
    options.name_IDs = ["*"]
    options.name_legacy = True
    options.name_languages = ["*"]
    # Keep it lean: drop layout/hinting tables the desktop UI doesn't need.
    options.layout_features = []
    options.glyph_names = False
    options.notdef_outline = True
    options.drop_tables = ["GPOS", "GSUB", "GDEF", "morx", "kern"]

    font = subset.load_font(SRC, options)
    subsetter = subset.Subsetter(options=options)
    subsetter.populate(unicodes=sorted(unicodes))
    subsetter.subset(font)
    subset.save_font(font, DST, options)

    size = os.path.getsize(DST)
    print(f"wrote {DST} ({size/1024/1024:.2f} MiB)")


if __name__ == "__main__":
    main()
