"""Hollow's placeholder art, drawn as pixel grids and written as PNGs.

The engine's described shapes (kind, size, color) are one frame of one
colour; a tileset with a one-way plank and a hero with run frames need
sheets. This is the "text-to-sprite pipeline" of design section 14 at its
smallest: run it and the files land in assets/, where the importer picks
them up. Standard library only.

    python tools/art.py
"""

import os
import struct
import zlib

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets")


def png(path, w, h, rows):
    """rows: h lists of w (r, g, b, a) tuples."""
    raw = b"".join(b"\x00" + b"".join(bytes(px) for px in row) for row in rows)

    def chunk(kind, data):
        c = struct.pack(">I", len(data)) + kind + data
        return c + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    data = b"\x89PNG\r\n\x1a\n"
    data += chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
    data += chunk(b"IDAT", zlib.compress(raw, 9))
    data += chunk(b"IEND", b"")
    full = os.path.join(ROOT, path)
    os.makedirs(os.path.dirname(full), exist_ok=True)
    with open(full, "wb") as f:
        f.write(data)


def hexc(s, a=255):
    return (int(s[0:2], 16), int(s[2:4], 16), int(s[4:6], 16), a)


CLEAR = (0, 0, 0, 0)


def grid(picture, palette):
    """A picture as strings, one character a pixel, looked up in palette."""
    return [[palette[ch] for ch in line] for line in picture]


# Tiles are 8 x 8: one world unit.
ROCK = grid(
    [
        "AAABAAAA",
        "ABAAAACA",
        "AAAACAAA",
        "CAAAAABA",
        "AABAAAAA",
        "AAAACAAB",
        "BAAAAAAA",
        "AACAABAA",
    ],
    {"A": hexc("4A4458"), "B": hexc("3A3548"), "C": hexc("5C566C")},
)

PLANK = grid(
    [
        "WWWWWWWW",
        "wwwwwwww",
        "DDDDDDDD",
        "..D..D..",
        "........",
        "........",
        "........",
        "........",
    ],
    {"W": hexc("B8834E"), "w": hexc("9C6B3C"), "D": hexc("6B4526"), ".": CLEAR},
)

BACK = grid(
    [
        "bbbbbbbb",
        "bbbbbbbb",
        "bbbabbbb",
        "bbbbbbbb",
        "bbbbbbbb",
        "babbbbbb",
        "bbbbbbbb",
        "bbbbbbab",
    ],
    {"b": hexc("1E1B26"), "a": hexc("24202E")},
)

# The hero: 8 frames of 8 x 13. Idle, run 1 to 4, jump, fall, land.
H = hexc("3B2A20")  # hair
S = hexc("F2D1B3")  # skin
T = hexc("D0603F")  # tunic
L = hexc("3B3350")  # legs
PAL = {"h": H, "s": S, "t": T, "l": L, ".": CLEAR}
TOP = [
    "..hhhh..",
    ".hhhhhh.",
    "..ssss..",
    "..ssss..",
    ".tttttt.",
    "tttttttt",
    ".tttttt.",
    ".tttttt.",
]
LEGS = {
    "idle": ["..l..l..", "..l..l..", "..l..l..", "..l..l..", ".ll..ll."],
    "run1": [".l....l.", ".l....l.", "l......l", "l.......", "........"],
    "run2": ["..l..l..", "..l.l...", "...ll...", "...l.l..", "..ll...."],
    "run3": ["..l..l..", ".l...l..", ".l....l.", "l.....l.", "......ll"],
    "run4": ["..l..l..", "...l.l..", "...ll...", "..l.l...", "....ll.."],
    "jump": ["..llll..", ".l....l.", ".l....l.", "........", "........"],
    "fall": [".l....l.", "l......l", "l......l", "........", "........"],
}


def frame(legs, crouch=False):
    top = TOP
    if crouch:
        top = ["........", "........"] + TOP[:2] + TOP[3:4] + TOP[5:8]
        legs = [".l....l.", "ll....ll", "........", "........", "........"][:5]
        return grid((top + legs)[:13], PAL)
    return grid(top + LEGS[legs], PAL)


# A signpost, 8 x 18: a board above the hero's head (13 tall) on a post,
# so a sign reads as a sign and its words (Describe.shown) clear the hero.
SIGN = grid(
    [
        "DDDDDDDD",
        "DLLLLLLD",
        "DLkkkLLD",
        "DLLLkkLD",
        "DLkkLkLD",
        "DDDDDDDD",
    ]
    + ["...PQ..."] * 12,
    {
        ".": CLEAR,
        "D": hexc("5a4028"),
        "L": hexc("b08850"),
        "k": hexc("3a2a18"),
        "P": hexc("8a6a40"),
        "Q": hexc("6a5030"),
    },
)


# The way out, 12 x 20: a stone arch in the rock's colours around daylight,
# so the end of the hall reads as somewhere to go (the phase 2 playtest's
# mark: "The level needs a goal to reach that isn't death").
EXIT = grid(
    [
        "...AAAAAA...",
        "..AACCCCAA..",
        ".AACyyyyCAA.",
        ".ACyyooyyCA.",
        "AACyoWWoyCAA",
        "ACyoWWWWoyCA",
    ]
    + ["ACyoWWWWoyCA"] * 12
    + ["CCCCCCCCCCCC", "ABAAABAAABAA"],
    {
        ".": CLEAR,
        "A": hexc("4A4458"),
        "B": hexc("3A3548"),
        "C": hexc("6E6882"),
        "y": hexc("F0B050"),
        "o": hexc("FFD880"),
        "W": hexc("FFF4D0"),
    },
)


# Something to find, 8 x 8: a small crystal that catches the light (the
# playtest's mark at the top of the shaft: "put something up here like a
# collectible so its not pointless").
GEM = grid(
    [
        "...cc...",
        "..cCCc..",
        ".cCWCCc.",
        "cCWCCCCc",
        ".cCCCCc.",
        "..cCCc..",
        "...cc...",
        "........",
    ],
    {".": CLEAR, "c": hexc("1F7A78"), "C": hexc("5FE0D0"), "W": hexc("F0FFFC")},
)


def hero():
    frames = [
        frame("idle"),
        frame("run1"),
        frame("run2"),
        frame("run3"),
        frame("run4"),
        frame("jump"),
        frame("fall"),
        frame("idle", crouch=True),
    ]
    rows = []
    for y in range(13):
        row = []
        for f in frames:
            row.extend(f[y])
        rows.append(row)
    return rows


if __name__ == "__main__":
    png("tiles/rock.png", 8, 8, ROCK)
    png("tiles/plank.png", 8, 8, PLANK)
    png("tiles/back.png", 8, 8, BACK)
    png("hero.png", 64, 13, hero())
    png("sign.png", 8, 18, SIGN)
    png("exit.png", 12, 20, EXIT)
    png("gem.png", 8, 8, GEM)
    print("wrote tiles/rock.png, tiles/plank.png, tiles/back.png, hero.png, sign.png, exit.png, gem.png")
