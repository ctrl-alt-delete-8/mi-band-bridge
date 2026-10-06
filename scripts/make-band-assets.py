#!/usr/bin/env python3
"""Draw the PNG assets both band apps use.

Kept as a script so the artwork can be regenerated, and so the sizes stay
in one place. Replace any output file by hand to use your own artwork.
"""
from pathlib import Path
from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
BLUE = (31, 111, 235, 255)
AMBER = (210, 153, 34, 255)
WHITE = (255, 255, 255, 255)
DIM = (255, 255, 255, 150)


# Icon size comes from the device profile, not from the general Zepp OS design
# spec. That spec covers round watches and gives 124, which overflows here.
# zmake's zepp_devices.json entry for mi_band7 gives:
#   screenWidth 192, screenHeight 490, iconSize 100
# The canvas is drawn at its natural size, so a wrong size sits in the corner
# or spills out of the slot.
ICON = 100
MARGIN = 2
GRAPHIC = 64


def disc(fill):
    """The circular icon background, opaque, with the safe margin left clear."""
    img = Image.new("RGBA", (ICON, ICON), (0, 0, 0, 0))
    ImageDraw.Draw(img).ellipse(
        [MARGIN, MARGIN, ICON - MARGIN - 1, ICON - MARGIN - 1], fill=fill
    )
    return img


def chevron(size, up, color):
    """A single chevron, pointing up or down."""
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    w = max(3, size // 8)
    mid, left, right = size / 2, size * 0.22, size * 0.78
    near, far = size * 0.34, size * 0.66
    tip, base = (near, far) if up else (far, near)
    d.line([(left, base), (mid, tip)], fill=color, width=w, joint="curve")
    d.line([(mid, tip), (right, base)], fill=color, width=w, joint="curve")
    return img


def button_icon():
    """A pill standing upright: the single big button."""
    img = disc(BLUE)
    d = ImageDraw.Draw(img)
    half = GRAPHIC / 2
    cx = cy = ICON / 2
    pill_w, pill_h = GRAPHIC * 0.46, GRAPHIC * 0.92
    d.rounded_rectangle(
        [cx - pill_w / 2, cy - pill_h / 2, cx + pill_w / 2, cy + pill_h / 2],
        radius=pill_w / 2,
        outline=WHITE,
        width=6,
    )
    return img


def arrows_icon():
    """Two chevrons, one above the other, kept clear of each other."""
    img = disc(AMBER)
    d = ImageDraw.Draw(img)
    cx = ICON / 2
    left, right = cx - GRAPHIC * 0.27, cx + GRAPHIC * 0.27
    top, bottom = ICON / 2 - GRAPHIC * 0.17, ICON / 2 + GRAPHIC * 0.17
    d.line([(left, top), (cx, top - GRAPHIC * 0.21), (right, top)],
           fill=WHITE, width=7, joint="curve")
    d.line([(left, bottom), (cx, bottom + GRAPHIC * 0.21), (right, bottom)],
           fill=WHITE, width=7, joint="curve")
    return img


# Pill artwork for the two-button app. The band draws normal_src as the button
# background, so the image must be the button's own size. The chevron sits in
# the upper half, because the button's text renders centered and would otherwise
# land on top of it.
PILL_W = 180
PILL_H = 202


def pill(up, fill, ink):
    img = Image.new("RGBA", (PILL_W, PILL_H), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle([0, 0, PILL_W - 1, PILL_H - 1], radius=PILL_W // 2, fill=fill)
    cx = PILL_W / 2
    left, right = cx - 30, cx + 30
    base, tip = (PILL_H * 0.40, PILL_H * 0.24) if up else (PILL_H * 0.24, PILL_H * 0.40)
    d.line([(left, base), (cx, tip), (right, base)], fill=ink, width=10, joint="curve")
    return img


def menu_bar_icon(size=44):
    """The menu bar mark: an outline, no fill.

    macOS treats this as a template image, so the shape comes from the alpha
    channel and the system colors it for the bar it sits in. Drawing it in one
    color is therefore enough.
    """
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    edge = max(2, round(size / 16))
    inset = edge
    d.rounded_rectangle(
        [inset, inset, size - inset - 1, size - inset - 1],
        radius=round(size * 0.24),
        outline=WHITE,
        width=edge,
    )
    pill_w, pill_h = size * 0.28, size * 0.52
    cx = cy = size / 2
    d.rounded_rectangle(
        [cx - pill_w / 2, cy - pill_h / 2, cx + pill_w / 2, cy + pill_h / 2],
        radius=pill_w / 2,
        outline=WHITE,
        width=edge,
    )
    return img


def main():
    one = ROOT / "band-app" / "assets"
    two = ROOT / "band-app-arrows" / "assets"
    one.mkdir(parents=True, exist_ok=True)
    two.mkdir(parents=True, exist_ok=True)

    button_icon().save(one / "icon.png")
    tray = ROOT / "gui" / "icons"
    tray.mkdir(parents=True, exist_ok=True)
    for scale, name in ((22, "tray.png"), (44, "tray@2x.png")):
        menu_bar_icon(scale).save(tray / name)
    arrows_icon().save(two / "icon.png")
    for up, stem in ((True, "up"), (False, "down")):
        pill(up, AMBER, WHITE).save(two / f"{stem}.png")
        pill(up, (255, 212, 121, 255), (90, 60, 0, 255)).save(two / f"{stem}_press.png")

    for path in sorted(list(one.iterdir()) + list(two.iterdir())):
        print(f"  {path.relative_to(ROOT)}  {path.stat().st_size} bytes")


if __name__ == "__main__":
    main()
