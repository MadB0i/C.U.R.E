#!/usr/bin/env python3
"""Draw C.U.R.E's application icon.

The mark is original: a graphite rescue tile carrying a violet open-bracket
crosshair — the same shape the wordmark uses — with one amber eye at the
centre. The eye is the companion, reduced to a single mark.

Everything is drawn from primitives at a fixed 1024×1024 master size and
then resampled, so the small sizes stay legible: the crosshair is three
strokes wide at 16 pixels, and the eye is a solid dot rather than a
gradient.

    python tools/make-icons.py

writes:

    gui/src-tauri/icons/32x32.png
    gui/src-tauri/icons/128x128.png
    gui/src-tauri/icons/128x128@2x.png
    gui/src-tauri/icons/icon.ico
    gui/src-tauri/icons/icon.png
"""

from __future__ import annotations

import math
from pathlib import Path

from PIL import Image, ImageDraw

REPO = Path(__file__).resolve().parents[1]
ICONS = REPO / "gui" / "src-tauri" / "icons"

MASTER = 1024
TILE = "#141021"
TILE_EDGE = "#241a3a"
VIOLET = "#8b6ff0"
VIOLET_DEEP = "#5b45b8"
AMBER = "#ffc264"


def _rounded_square(size: int) -> Image.Image:
    image = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)
    radius = int(size * 0.22)
    draw.rounded_rectangle(
        (0, 0, size - 1, size - 1), radius=radius, fill=TILE, outline=TILE_EDGE,
        width=max(1, int(size * 0.012)),
    )
    return image


def _bracket(draw: ImageDraw.ImageDraw, x0: float, y0: float, x1: float, y1: float,
             width: float) -> None:
    """One open corner of the crosshair."""
    arm = (x1 - x0) * 0.42
    draw.line((x0 + arm, y0, x0, y0, x0, y0 + arm), fill=VIOLET, width=int(width),
              joint="curve")
    draw.line((x1 - arm, y0, x1, y0, x1, y0 + arm), fill=VIOLET, width=int(width),
              joint="curve")
    draw.line((x0 + arm, y1, x0, y1, x0, y1 - arm), fill=VIOLET, width=int(width),
              joint="curve")
    draw.line((x1 - arm, y1, x1, y1, x1, y1 - arm), fill=VIOLET, width=int(width),
              joint="curve")


def draw_icon(size: int) -> Image.Image:
    image = _rounded_square(size)
    draw = ImageDraw.Draw(image)
    s = size

    # A soft violet bloom behind the mark, so the tile is not flat.
    bloom = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    bloom_draw = ImageDraw.Draw(bloom)
    bloom_draw.ellipse(
        (s * 0.12, s * 0.12, s * 0.88, s * 0.88), fill=(139, 111, 240, 46)
    )
    image = Image.alpha_composite(image, bloom)
    draw = ImageDraw.Draw(image)

    margin = s * 0.235
    width = max(2, int(s * 0.062))
    _bracket(draw, margin, margin, s - margin, s - margin, width)

    # The centre bar and the eye.
    draw.line((s * 0.5, s * 0.5 - s * 0.17, s * 0.5, s * 0.5 + s * 0.17),
              fill=VIOLET_DEEP, width=int(width * 0.85))
    eye = s * 0.072
    draw.ellipse((s * 0.5 - eye, s * 0.5 - eye, s * 0.5 + eye, s * 0.5 + eye),
                 fill=AMBER)
    return image


def main() -> None:
    ICONS.mkdir(parents=True, exist_ok=True)
    master = draw_icon(MASTER)

    master.resize((32, 32), Image.LANCZOS).save(ICONS / "32x32.png")
    master.resize((128, 128), Image.LANCZOS).save(ICONS / "128x128.png")
    master.resize((256, 256), Image.LANCZOS).save(ICONS / "128x128@2x.png")
    master.resize((512, 512), Image.LANCZOS).save(ICONS / "icon.png")
    master.save(
        ICONS / "icon.ico",
        format="ICO",
        sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
    )
    for name in sorted(p.name for p in ICONS.iterdir()):
        print(f"  {name}")


if __name__ == "__main__":
    main()
