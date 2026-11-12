#!/usr/bin/env python3
"""Generate the Haste mod's clock artwork and its effect program.

Run from the repository root:

    python3 tools/haste_clock.py
    resonance-import texture --input local/haste-clock.png \
        --output mods/haste/battle/spells/haste/textures/clock.ktx2

The atlas is original artwork drawn by this script. The effect program is a
schedule of ordinary particle births: the second hand and the counter are
re-spawned at each tick, so their timing is entirely in `TICKS` below.
"""
import json
import math
from pathlib import Path

from PIL import Image, ImageDraw

REPOSITORY = Path(__file__).resolve().parent.parent
ROOT = REPOSITORY / "mods/haste"
ATLAS = 256
FACE = (0, 0, 128, 128)  # x, y, width, height in atlas pixels
HAND = (128, 0, 128, 128)
DIGIT = (24, 40)  # one counter wheel; digit d starts at x = 25 * d
DIGITS_Y = 132
PLATE = (0, 180, 68, 52)

BRASS = (236, 196, 92, 255)
IVORY = (250, 244, 226, 255)
INK = (40, 30, 24, 255)
STEEL = (70, 78, 92, 255)
RED = (222, 62, 46, 255)

# Gameplay updates between successive ticks. The clock starts at one tick
# every quarter second and winds up to one every thirtieth.
TICKS = [15, 13, 11, 10, 9, 8, 7, 6, 5, 5, 4, 4, 3, 3, 3, 2, 2, 2, 2, 2, 2, 2]
HOLD = 14  # the final reading stays this long before fading
DEGREES_PER_TICK = 30

# Seven-segment wheels: which of segments a-g are lit for each digit.
SEGMENTS = [
    "abcdef", "bc", "abdeg", "abcdg", "bcfg",
    "acdfg", "acdefg", "abc", "abcdefg", "abcdfg",
]


def draw_face(draw):
    x, y, w, h = FACE
    cx, cy, r = x + w / 2, y + h / 2, w / 2 - 4
    draw.ellipse((cx - r, cy - r, cx + r, cy + r), fill=BRASS)
    draw.ellipse((cx - r + 7, cy - r + 7, cx + r - 7, cy + r - 7), fill=IVORY)
    for hour in range(12):
        angle = math.radians(hour * 30)
        outer, inner = r - 10, r - (24 if hour % 3 == 0 else 17)
        sx, sy = math.sin(angle), -math.cos(angle)
        draw.line(
            (cx + sx * inner, cy + sy * inner, cx + sx * outer, cy + sy * outer),
            fill=INK,
            width=5 if hour % 3 == 0 else 3,
        )


def draw_hand(draw):
    # The pivot is the cell's center, so a billboard roll turns the hand in place.
    x, y, w, h = HAND
    cx, cy = x + w / 2, y + h / 2
    draw.polygon(
        ((cx - 4, cy + 14), (cx + 4, cy + 14), (cx + 1.5, cy - 44), (cx - 1.5, cy - 44)),
        fill=RED,
    )
    draw.ellipse((cx - 8, cy - 8, cx + 8, cy + 8), fill=INK)
    draw.ellipse((cx - 3, cy - 3, cx + 3, cy + 3), fill=BRASS)


def draw_digit(draw, digit):
    w, h = DIGIT
    x, y = 25 * digit, DIGITS_Y
    draw.rounded_rectangle((x, y, x + w - 1, y + h - 1), radius=3, fill=INK)
    left, right, top, middle, bottom, t = x + 5, x + w - 6, y + 5, y + h / 2, y + h - 6, 3
    bars = {
        "a": (left, top, right, top + t),
        "g": (left, middle - t / 2, right, middle + t / 2),
        "d": (left, bottom - t, right, bottom),
        "f": (left, top, left + t, middle),
        "b": (right - t, top, right, middle),
        "e": (left, middle, left + t, bottom),
        "c": (right - t, middle, right, bottom),
    }
    for segment in SEGMENTS[digit]:
        draw.rectangle(bars[segment], fill=IVORY)


def draw_plate(draw):
    x, y, w, h = PLATE
    draw.rounded_rectangle((x, y, x + w - 1, y + h - 1), radius=6, fill=BRASS)
    draw.rounded_rectangle((x + 4, y + 4, x + w - 5, y + h - 5), radius=4, fill=STEEL)


def atlas():
    image = Image.new("RGBA", (ATLAS, ATLAS), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)
    draw_face(draw)
    draw_hand(draw)
    for digit in range(10):
        draw_digit(draw, digit)
    draw_plate(draw)
    return image


def particle(uv, size, offset, lifetime=None, fade_from=None):
    """An alpha-blended billboard that follows the recipient."""
    template = {
        "follow_origin": True,
        "state": {
            "offset": offset,
            "colors": [[255, 255, 255, 255], [255, 255, 255, 255]],
            # Height, then width, in world units.
            "geometry": {"Size": {"value": [size[1], size[0], 0], "velocity": [0, 0, 0]}},
            "uv": list(uv),
        },
    }
    if lifetime is not None:
        template["lifetime"] = lifetime
    if fade_from is not None:
        template["fade"] = [0, 0, 0, 32]
        template["fade_from"] = fade_from
    return {
        "kind": "particle",
        "template": template,
        "visual": {
            "blend": "alpha",
            "depth_test": False,
            "geometry": {"kind": "quad"},
            "orientation": "billboard",
            "texture": {"kind": "atlas", "slot": "bank-0"},
        },
    }


def effects():
    total = sum(TICKS) + HOLD
    height = 95  # above the recipient's center
    templates = {
        "clock-face": particle(FACE, (56, 56), [0, height, 0], total, total - 8),
        "clock-hand": particle(HAND, (56, 56), [0, height, 0.5]),
        "counter-plate": particle(PLATE, (34, 26), [0, height - 44, 0], total, total - 8),
        "counter-digit": particle((0, DIGITS_Y, *DIGIT), (12, 20), [0, height - 44, 0.5]),
    }
    program = [
        {"kind": "spawn", "particle": "clock-face"},
        {"kind": "spawn", "particle": "counter-plate"},
    ]
    at = 0
    for tick, interval in enumerate(TICKS + [HOLD]):
        # Births in the program's first update wait one update before they
        # age, so the first reading is told to leave one update sooner.
        shown = interval - 1 if tick == 0 else interval
        hand = {"lifetime": shown, "angles": [None, None, -DEGREES_PER_TICK * tick]}
        program.append({"at": at, "kind": "spawn", "particle": "clock-hand", "birth": hand})
        for place, digit in ((-7, tick // 10), (7, tick % 10)):
            birth = {
                "lifetime": shown,
                "offset": [place, None, None],
                "uv": [25 * digit, None, None, None],
            }
            program.append(
                {"at": at, "kind": "spawn", "particle": "counter-digit", "birth": birth}
            )
        # The menu's navigation tick, the nearest original sound to clockwork.
        program.append({"at": at, "kind": "sound", "id": 1})
        at += interval
    return {
        "templates": templates,
        "programs": [program],
        "artwork": {
            "textures": {
                "bank-0": [{
                    "images": [{
                        "image": "battle/spells/haste/textures/clock.ktx2",
                        "size": [ATLAS, ATLAS],
                    }],
                    "sampler": {"wrap": ["clamp", "clamp"]},
                }]
            }
        },
    }


if __name__ == "__main__":
    (REPOSITORY / "local").mkdir(exist_ok=True)
    atlas().save(REPOSITORY / "local/haste-clock.png")
    path = ROOT / "battle/spells/haste/effects.json"
    path.write_text(json.dumps(effects(), indent=2) + "\n")
    print(f"clock runs {sum(TICKS) + HOLD} updates over {len(TICKS) + 1} readings")
