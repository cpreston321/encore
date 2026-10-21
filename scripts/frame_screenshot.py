#!/usr/bin/env python3
"""Frames a tour snapshot as a Mac window for the README: rounded corners, a hairline edge,
the window buttons, and a soft shadow, on a transparent background that suits GitHub's
light and dark themes alike. Needs Pillow.

  scripts/frame_screenshot.py docs/screenshots/home.png
  scripts/frame_screenshot.py --points 300 --lights 10,10 docs/screenshots/mini-player.png

Tour snapshots are rendered offscreen, so they have no window buttons or corners of their
own; `--lights` is where the window puts its buttons, in points (TitlebarOptions'
traffic_light_position), and `--points` is the window's width in points.
"""

import argparse

from PIL import Image, ImageDraw, ImageFilter

SUPERSAMPLE = 4
# Tahoe's window corner radius, and its window buttons' size and spacing, in points.
CORNER = 26
BUTTON = 14
BUTTON_STEP = 22
BUTTONS = [((255, 95, 87), (224, 68, 62)), ((254, 188, 46), (222, 161, 35)), ((40, 200, 64), (29, 173, 43))]


def rounded_mask(size, radius):
    w, h = size
    big = Image.new("L", (w * SUPERSAMPLE, h * SUPERSAMPLE), 0)
    ImageDraw.Draw(big).rounded_rectangle(
        (0, 0, w * SUPERSAMPLE - 1, h * SUPERSAMPLE - 1), radius * SUPERSAMPLE, fill=255
    )
    return big.resize(size, Image.LANCZOS)


def frame(path, points, lights):
    shot = Image.open(path).convert("RGBA")
    w, h = shot.size
    scale = w / points
    radius = round(CORNER * scale)

    if lights:
        big = shot.resize((w * SUPERSAMPLE, h * SUPERSAMPLE), Image.LANCZOS)
        draw = ImageDraw.Draw(big)
        s = scale * SUPERSAMPLE
        for i, (fill, edge) in enumerate(BUTTONS):
            cx = (lights[0] + BUTTON / 2 + i * BUTTON_STEP) * s
            cy = (lights[1] + BUTTON / 2) * s
            r = BUTTON / 2 * s
            draw.ellipse((cx - r, cy - r, cx + r, cy + r), fill=edge)
            inset = 0.5 * s
            draw.ellipse((cx - r + inset, cy - r + inset, cx + r - inset, cy + r - inset), fill=fill)
        shot = big.resize((w, h), Image.LANCZOS)

    mask = rounded_mask((w, h), radius)
    window = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    window.paste(shot, (0, 0), mask)

    # A hairline just inside the edge, which reads on light and dark pages.
    edge = Image.new("RGBA", (w * SUPERSAMPLE, h * SUPERSAMPLE), (0, 0, 0, 0))
    ImageDraw.Draw(edge).rounded_rectangle(
        (0, 0, w * SUPERSAMPLE - 1, h * SUPERSAMPLE - 1),
        radius * SUPERSAMPLE,
        outline=(128, 128, 128, 110),
        width=max(1, round(scale * SUPERSAMPLE)),
    )
    window = Image.alpha_composite(window, edge.resize((w, h), Image.LANCZOS))

    pad = round(w * 0.05)
    blur = round(w * 0.022)
    drop = round(w * 0.009)
    canvas = Image.new("RGBA", (w + pad * 2, h + pad * 2), (0, 0, 0, 0))
    shadow_alpha = Image.new("L", canvas.size, 0)
    shadow_alpha.paste(mask.point(lambda a: a * 0.34), (pad, pad + drop))
    shadow = Image.new("RGBA", canvas.size, (0, 0, 0, 255))
    shadow.putalpha(shadow_alpha.filter(ImageFilter.GaussianBlur(blur)))
    canvas = Image.alpha_composite(canvas, shadow)
    canvas.alpha_composite(window, (pad, pad))
    canvas.save(path, optimize=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("paths", nargs="+")
    parser.add_argument("--points", type=float, default=1320, help="window width in points (default 1320)")
    parser.add_argument("--lights", default="22,22", help="window buttons' position in points, or 'none'")
    args = parser.parse_args()
    lights = None if args.lights == "none" else tuple(float(v) for v in args.lights.split(","))
    for path in args.paths:
        frame(path, args.points, lights)
        print(f"framed {path}")


if __name__ == "__main__":
    main()
