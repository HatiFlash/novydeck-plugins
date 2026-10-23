#!/usr/bin/env python3
"""Reproducible offline NovyDeck icons; vector-like shapes rendered at 4x."""
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[1]
FONT = ROOT / "novydeck-sdk/assets/DejaVuSans.ttf"
LABELS = {
    "clock": {"plugin": "", "clock": "24", "timer": "T", "stopwatch": "S"},
    "counter": {"plugin": "", "counter": "+1", "note": "N", "clipboard": "C"},
    "sysmonitor": {"plugin": "", "cpu": "CPU", "ram": "RAM", "gpu": "GPU", "temperature": "°C"},
    "nowplaying": {"plugin": "", "play": "▶", "next": ">|", "previous": "|<"},
    "oledsaver": {"plugin": "", "saver": "OLED"},
    "homeassistant": {"plugin": "", "toggle": "HA", "service": "API", "state": "°C"},
    "googleassistant": {"plugin": "", "command": "G"},
}
# Dedicated hues, shared geometry and lighting. No third-party brand marks.
COLORS = {
    "clock": ((39, 116, 220), (24, 55, 139)),
    "counter": ((41, 174, 128), (17, 98, 103)),
    "sysmonitor": ((241, 153, 62), (167, 69, 63)),
    "nowplaying": ((177, 95, 226), (89, 52, 167)),
    "oledsaver": ((70, 103, 153), (27, 36, 85)),
    "homeassistant": ((48, 173, 203), (30, 86, 154)),
    "googleassistant": ((223, 99, 137), (129, 50, 115)),
}
SCALE = 4


def logo(name, size):
    n = size * SCALE
    def p(v):
        return round(v * n / 144)
    top, bottom = COLORS[name]
    image = Image.new("RGBA", (n, n))
    pixels = image.load()
    assert pixels is not None
    for y in range(n):
        t = y / max(n - 1, 1)
        color = tuple(round(top[i] * (1-t) + bottom[i] * t) for i in range(3)) + (255,)
        for x in range(n):
            pixels[x, y] = color
    mask = Image.new("L", (n, n))
    ImageDraw.Draw(mask).rounded_rectangle((p(4), p(4), n-p(4), n-p(4)), radius=p(28), fill=255)
    image.putalpha(mask)
    d = ImageDraw.Draw(image)
    white = (255, 255, 255, 255)
    stroke = p(8)
    if name == "clock":
        d.ellipse((p(32), p(32), p(112), p(112)), outline=white, width=stroke)
        d.line((p(72), p(47), p(72), p(73), p(89), p(85)), fill=white, width=stroke, joint="curve")
        d.ellipse((p(68), p(68), p(76), p(76)), fill=white)
    elif name == "counter":
        for y in (47, 71, 95):
            d.line((p(35), p(y), p(77), p(y)), fill=white, width=stroke)
        d.line((p(95), p(67), p(95), p(99)), fill=white, width=stroke)
        d.line((p(79), p(83), p(111), p(83)), fill=white, width=stroke)
    elif name == "sysmonitor":
        d.arc((p(31), p(30), p(113), p(112)), 185, 355, fill=white, width=stroke)
        d.line((p(72), p(78), p(96), p(51)), fill=white, width=stroke)
        d.ellipse((p(65), p(71), p(79), p(85)), fill=white)
        d.line((p(40), p(100), p(104), p(100)), fill=white, width=p(6))
    elif name == "nowplaying":
        d.ellipse((p(36), p(81), p(61), p(101)), fill=white)
        d.line((p(59), p(90), p(59), p(39), p(83), p(34)), fill=white, width=stroke, joint="curve")
        d.polygon([(p(84), p(61)), (p(84), p(99)), (p(114), p(80))], fill=white)
    elif name == "oledsaver":
        d.rounded_rectangle((p(27), p(37), p(117), p(100)), radius=p(9), outline=white, width=p(7))
        d.arc((p(62), p(47), p(95), p(82)), 50, 260, fill=white, width=p(6))
        d.line((p(58), p(110), p(86), p(110)), fill=white, width=p(6))
    elif name == "homeassistant":
        d.polygon([(p(25), p(68)), (p(72), p(30)), (p(119), p(68)), (p(108), p(79)), (p(72), p(48)), (p(36), p(79))], fill=white)
        d.rounded_rectangle((p(39), p(73), p(105), p(111)), radius=p(4), fill=white)
        d.rounded_rectangle((p(65), p(83), p(82), p(111)), radius=p(3), fill=COLORS[name][1])
    else:
        d.rounded_rectangle((p(51), p(32), p(93), p(83)), radius=p(20), fill=white)
        d.arc((p(38), p(44), p(106), p(100)), 0, 180, fill=white, width=p(7))
        d.line((p(72), p(100), p(72), p(112)), fill=white, width=p(7))
        d.line((p(55), p(113), p(89), p(113)), fill=white, width=p(7))
    return image.resize((size, size), Image.Resampling.LANCZOS)


def action_icon(name, text, size):
    # Action names stay readable and use the same palette and rounded tile.
    n = size * SCALE
    image = logo(name, size).resize((n, n), Image.Resampling.NEAREST)
    shade = Image.new("RGBA", (n, n), (12, 18, 32, 155))
    alpha_mask = Image.new("L", (n, n))
    alpha_mask.paste(155, (0, 0, n, n), image.getchannel("A"))
    shade.putalpha(alpha_mask)
    image = Image.alpha_composite(image, shade)
    d = ImageDraw.Draw(image)
    font = ImageFont.truetype(str(FONT), max(8, n // (4 if len(text) > 2 else 3)))
    d.text((n / 2, n / 2), text, font=font, anchor="mm", fill="white", stroke_width=max(1, n // 125), stroke_fill=(20, 28, 42))
    return image.resize((size, size), Image.Resampling.LANCZOS)


def main():
    board = Image.new("RGB", (7 * 180, 230), "#1b1e26")
    draw = ImageDraw.Draw(board)
    font = ImageFont.truetype(str(FONT), 15)
    for col, (name, actions) in enumerate(LABELS.items()):
        out = ROOT / "plugins" / f"com.novydeck.{name}.sdPlugin" / "images"
        out.mkdir(parents=True, exist_ok=True)
        for action, text in actions.items():
            for size, suffix in ((20, "-icon"), (40, "-icon@2x"), (72, ""), (144, "@2x")):
                (logo(name, size) if action == "plugin" else action_icon(name, text, size)).save(out / f"{action}{suffix}.png", optimize=True)
        board.paste(logo(name, 144), (col * 180 + 18, 8), logo(name, 144))
        board.paste(logo(name, 28), (col * 180 + 76, 160), logo(name, 28))
        draw.text((col * 180 + 12, 200), name, font=font, fill="#e8eaf0")
    board.save(ROOT / "screenshots/logos.png", optimize=True)


if __name__ == "__main__":
    main()
