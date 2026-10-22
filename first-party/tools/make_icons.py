#!/usr/bin/env python3
"""Reproducible offline NovyDeck plugin icons (Pillow)."""
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[1]
FONT = ROOT / "novydeck-sdk/assets/DejaVuSans.ttf"
LABELS = {"clock": {"plugin": "ND", "clock": "24", "timer": "T", "stopwatch": "S"},
          "counter": {"plugin": "ND", "counter": "+1", "note": "N", "clipboard": "C"},
          "sysmonitor": {"plugin": "ND", "cpu": "CPU", "ram": "RAM", "gpu": "GPU", "temperature": "°C"},
          "nowplaying": {"plugin": "ND", "play": "▶", "next": ">|", "previous": "|<"},
          "oledsaver": {"plugin": "ND", "saver": "OLED"},
          "googleassistant": {"plugin": "ND", "command": "G"}}
for plugin, actions in LABELS.items():
    out = ROOT / "plugins" / f"com.novydeck.{plugin}.sdPlugin" / "images"
    out.mkdir(parents=True, exist_ok=True)
    for action, text in actions.items():
        for size, suffix in ((20, "-icon"), (40, "-icon@2x"), (72, ""), (144, "@2x")):
            image = Image.new("RGBA", (size, size), (18, 24, 36, 255))
            draw = ImageDraw.Draw(image)
            margin = max(1, size // 20)
            draw.rounded_rectangle((margin, margin, size-margin-1, size-margin-1), radius=size//6,
                                   outline=(68, 196, 165, 255), width=max(1, size//30))
            font = ImageFont.truetype(str(FONT), size//3)
            draw.text((size/2, size/2), text, font=font, anchor="mm", fill="white")
            image.save(out / f"{action}{suffix}.png", optimize=True)
