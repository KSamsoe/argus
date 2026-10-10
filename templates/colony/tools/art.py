"""The colony's art, drawn as pixel grids and by rule, and written as PNGs.

Everything the colony draws comes from here: the meadow's grass, the four
colonists, the berry bushes, the trees, the pond, the stockpile (its yard,
its store, its woodpile and baskets), the goods carried home, and the
panel the HUD and the pause screen sit on. Run it once and the files land
in assets/, where the engine picks them up (a running game takes them in
within a second):

    python tools/art.py

The camera (scenes/main/camera.toml) shows 20 world units top to bottom,
27 screen pixels a unit at 960 x 540. The art is drawn at 9 pixels a unit
(PPU), so every art pixel is three screen pixels.

What it writes, and who reads it:

- sprites/ground.png: the meadow and the long grass round it, one picture
  (scenes/main/ground.toml).
- sprites/colonists.png: every pose of every colonist, a row each, in the
  order of PEOPLE (systems/looks.luau picks the frame).
- sprites/meadow.png: the bushes, trees, goods, store, woodpile, baskets,
  pond, and yard, one sheet; its sidecar lists the frames in the order
  systems/looks.luau counts them.
- ui/panel.png: a wooden panel, nine-sliced (scenes/main/hud.toml and
  scenes/main/paused.toml).

To change first: the palette below (every colour has a name) and the
colonists' clothes and hair (PEOPLE). Standard library only, and the same
files every run: the noise comes from fixed seeds (SEED).
"""

import math
import os
import random
import struct
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
ASSETS = os.path.join(HERE, "..", "assets")

PPU = 9  # art pixels a world unit
SEED = 7  # change it for other grass, ripples, and leaves
MEADOW = (32, 18)  # the walkable meadow, in units (scenes/main/ground.toml's NavGrid)
GROUND = (40, 24)  # the picture under it: the meadow and a border of long grass


def rgb(s, a=255):
    return (int(s[0:2], 16), int(s[2:4], 16), int(s[4:6], 16), a)


# The palette ----------------------------------------------------------------
# Each list runs dark to light.

CLEAR = (0, 0, 0, 0)
INK = rgb("2A1E18")  # every outline
EYE = rgb("1E1410")
SHADOW = (16, 26, 12, 80)  # the shadow under everything standing
GRASS = [rgb("2C4724"), rgb("34532B"), rgb("3C5E31"), rgb("456A37"), rgb("51783E")]
BRIGHT_GRASS = rgb("78A256")  # the brightest tufts
FLOWERS = [rgb("EEE8D2"), rgb("F2D25A"), rgb("E79AB4"), rgb("A6BEEA")]
STONE = [rgb("5E5C54"), rgb("85837A"), rgb("ABA99E")]
LEAF = [rgb("223C1D"), rgb("30522A"), rgb("426E34"), rgb("578A42"), rgb("74A852")]
PINE = [rgb("1A3424"), rgb("23472F"), rgb("2F5C3C"), rgb("3E744C"), rgb("528E5C")]
BARK = [rgb("3A281C"), rgb("553B28"), rgb("704E34"), rgb("8E6A48")]
CUT = [rgb("A88454"), rgb("CFAE78"), rgb("E6CC98")]  # a cut log's face
BERRY = [rgb("6E1428"), rgb("C2405A"), rgb("F49AAC")]  # dark, berry, shine
UNRIPE = [rgb("80A040"), rgb("BCD46C")]
BLOSSOM = [rgb("F6F2E4"), rgb("EDCB5A")]
WATER = [rgb("1C3C62"), rgb("265086"), rgb("336AA2"), rgb("4E88C0"), rgb("98C8E6")]
BANK = [rgb("4E4030"), rgb("6E5C40"), rgb("8E7A56"), rgb("AE9A70")]
REED = [rgb("3E5E28"), rgb("628A3C"), rgb("6A4428")]
DIRT = [rgb("5E4A34"), rgb("7A6346"), rgb("907858"), rgb("A88E6C")]
WOOD = [rgb("3A2616"), rgb("5A3C24"), rgb("7A5432"), rgb("9C7246"), rgb("BC9060")]
ROOF = [rgb("4A2416"), rgb("6E321E"), rgb("8E4428"), rgb("AE5C36"), rgb("C8784A")]
GLASS = rgb("27323E")  # a window by day
LAMPLIGHT = [rgb("F2A040"), rgb("FFD27A"), rgb("FFF2BE")]  # the window and lantern at night
BASKET = [rgb("5E3E1E"), rgb("8A6234"), rgb("B68E56")]
STEEL = [rgb("6E747C"), rgb("B4BCC4")]

# The colonists, a row of colonists.png each: systems/looks.luau finds a
# colonist's row by Colonist.name (another name gets a row from its length).
# hair: short, long, or beard.
PEOPLE = [
    {"name": "Ada", "hair": ("A85432", "74321E"), "skin": ("F0C8A0", "D09A74"),
     "shirt": ("5E8E3E", "44682C"), "legs": ("6A4A30", "4E3622"), "style": "long"},
    {"name": "Bram", "hair": ("3E2C22", "261A12"), "skin": ("D8A070", "B07A50"),
     "shirt": ("B23E30", "842C22"), "legs": ("3E4C6C", "2C3852"), "style": "beard"},
    {"name": "Cleo", "hair": ("E8C462", "B8923C"), "skin": ("F4D2B4", "D8AA88"),
     "shirt": ("4A7CBC", "34598E"), "legs": ("6A5A8C", "4E4068"), "style": "long"},
    {"name": "Dov", "hair": ("6E4C30", "4A3220"), "skin": ("A86C4A", "845238"),
     "shirt": ("C88C32", "9A6822"), "legs": ("5A6C3A", "44522A"), "style": "short"},
]


# PNG out ----------------------------------------------------------------------


def _paeth(a, b, c):
    p = a + b - c
    pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
    if pa <= pb and pa <= pc:
        return a
    return b if pb <= pc else c


def pow2(n):
    p = 1
    while p < n:
        p *= 2
    return p


def write_png(path, img):
    """An Img as an RGBA PNG, padded at the right and bottom to a power of
    two each way (the size a texture wants; the frames do not move). Each
    row takes the filter that leaves the smallest sum (the usual
    heuristic), so the grass packs small."""
    if (pow2(img.w), pow2(img.h)) != (img.w, img.h):
        padded = Img(pow2(img.w), pow2(img.h))
        padded.blit(img, 0, 0)
        img = padded
    w, h = img.w, img.h
    prev = bytes(w * 4)
    out = bytearray()
    for row in img.px:
        cur = bytes(v for px in row for v in px)
        best = None
        for kind in range(5):
            f = bytearray(len(cur))
            for i, x in enumerate(cur):
                a = cur[i - 4] if i >= 4 else 0
                b = prev[i]
                c = prev[i - 4] if i >= 4 else 0
                p = (0, a, b, (a + b) // 2, _paeth(a, b, c))[kind]
                f[i] = (x - p) & 0xFF
            score = sum(v if v < 128 else 256 - v for v in f)
            if best is None or score < best[0]:
                best = (score, kind, f)
        out.append(best[1])
        out += best[2]
        prev = cur

    def chunk(kind, data):
        c = struct.pack(">I", len(data)) + kind + data
        return c + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    data = b"\x89PNG\r\n\x1a\n"
    data += chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
    data += chunk(b"IDAT", zlib.compress(bytes(out), 9))
    data += chunk(b"IEND", b"")
    full = os.path.join(ASSETS, path)
    os.makedirs(os.path.dirname(full), exist_ok=True)
    with open(full, "wb") as f:
        f.write(data)


def write_text(path, body):
    full = os.path.join(ASSETS, path)
    os.makedirs(os.path.dirname(full), exist_ok=True)
    with open(full, "w", newline="\n") as f:
        f.write(body)


# Drawing ------------------------------------------------------------------------


class Img:
    """A w x h picture of (r, g, b, a) pixels, row 0 at the top."""

    def __init__(self, w, h):
        self.w, self.h = w, h
        self.px = [[CLEAR] * w for _ in range(h)]

    def inside(self, x, y):
        return 0 <= x < self.w and 0 <= y < self.h

    def get(self, x, y):
        return self.px[y][x] if self.inside(x, y) else CLEAR

    def set(self, x, y, c):
        if self.inside(x, y):
            self.px[y][x] = c

    def under(self, x, y, c):
        """Set only where nothing is drawn yet (a shadow under a thing)."""
        if self.inside(x, y) and self.px[y][x][3] == 0:
            self.px[y][x] = c

    def blit(self, other, ox, oy):
        for y in range(other.h):
            for x in range(other.w):
                c = other.px[y][x]
                if c[3]:
                    self.set(ox + x, oy + y, c)

    def outline(self, c=INK):
        """A one-pixel line round everything drawn (four neighbours)."""
        edge = []
        for y in range(self.h):
            for x in range(self.w):
                if self.px[y][x][3] == 0 and any(
                    self.get(x + dx, y + dy)[3] == 255 for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1))
                ):
                    edge.append((x, y))
        for x, y in edge:
            self.px[y][x] = c
        return self

    def shadow(self, cx, cy, rx, ry):
        """A soft oval on the ground, under what is drawn."""
        for y in range(self.h):
            for x in range(self.w):
                if ((x + 0.5 - cx) / rx) ** 2 + ((y + 0.5 - cy) / ry) ** 2 <= 1:
                    self.under(x, y, SHADOW)
        return self


def grid(picture, palette):
    """A picture as strings, one character a pixel, looked up in palette
    ('.' is clear)."""
    img = Img(len(picture[0]), len(picture))
    for y, line in enumerate(picture):
        for x, ch in enumerate(line):
            if ch != ".":
                img.px[y][x] = palette[ch]
    return img


def ease(t):
    return t * t * (3 - 2 * t)


class Noise:
    """Smooth value noise over [0, 1] x [0, 1]: random values on a lattice
    of cx x cy cells, eased between."""

    def __init__(self, rng, cx, cy):
        self.cx, self.cy = cx, cy
        self.v = [[rng.random() for _ in range(cx + 1)] for _ in range(cy + 1)]

    def at(self, u, v):
        x, y = u * self.cx, v * self.cy
        i, j = min(int(x), self.cx - 1), min(int(y), self.cy - 1)
        fx, fy = ease(x - i), ease(y - j)
        a = self.v[j][i] + (self.v[j][i + 1] - self.v[j][i]) * fx
        b = self.v[j + 1][i] + (self.v[j + 1][i + 1] - self.v[j + 1][i]) * fx
        return a + (b - a) * fy


# An ordered dither: a pixel's threshold by where it is, so shades meet in
# a fine checker instead of a hard edge.
BAYER = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]]


def dither(x, y):
    return BAYER[y % 4][x % 4] / 16.0 - 0.47


def foliage(img, circles, pal, rng, dx=0, light=(-0.55, -0.8)):
    """Leaves: the union of circles (cx, cy, r), lit from the top left, in
    pal's shades with a darker rim; dx shifts them (a tree shaken by the axe)."""
    lx, ly = light
    cells = {}
    for y in range(img.h):
        for x in range(img.w):
            best = None
            for cx, cy, r in circles:
                d = math.hypot(x + 0.5 - cx - dx, y + 0.5 - cy)
                if d <= r:
                    k = (r - d) / r
                    if best is None or k > best[0]:
                        best = (k, (x + 0.5 - cx - dx) / r, (y + 0.5 - cy) / r)
            if best is not None:
                cells[(x, y)] = best
    clumps = {}
    for (x, y), (k, nx, ny) in cells.items():
        key = ((x + 1) // 3, (y + 2) // 3)
        if key not in clumps:
            clumps[key] = rng.uniform(-0.55, 0.55)
        lit = -(nx * lx + ny * ly)  # 1 facing the light, -1 away
        v = 1.9 + 1.25 * lit + clumps[key] + 0.6 * dither(x, y)
        if ((x + 1) % 3 == 0 and (y + 2) % 3 == 0) and lit > -0.2:
            v += 0.8  # a leaf catching the light
        level = max(1, min(len(pal) - 1, int(round(v))))
        img.set(x, y, pal[level])
    for (x, y) in cells:
        if any((x + ox, y + oy) not in cells for ox, oy in ((1, 0), (-1, 0), (0, 1), (0, -1))):
            img.set(x, y, pal[0])
    return cells


# The colonists ------------------------------------------------------------------
# Drawn facing right (systems/looks.luau mirrors them to face left), 9 x 13
# pixels inside a one-pixel outline: an 11 x 16 frame, feet at the bottom
# and a row under them for a shadow. Letters: H hair, h its shade, S skin,
# s its shade, e eye, T shirt, t its shade (the arms), L belt, P legs,
# B boots, A and K an axe head, a its handle, W a wooden bowl, R a berry.

HEADS = {
    "short": [
        "..HHHHH..",
        ".HHHHHHH.",
        ".HhSSSSS.",
        ".HSSeSSe.",
        ".hSSSSSs.",
        "..sSSSs..",
    ],
    "long": [
        "..HHHHH..",
        ".HHHHHHH.",
        "HHhSSSSS.",
        "HHSSeSSe.",
        "HhSSSSSs.",
        "hh.sSSs..",
    ],
    "beard": [
        "..HHHHH..",
        ".HHHHHHH.",
        ".HhSSSSS.",
        ".HSSeSSe.",
        ".hSHHHHH.",
        "..hHHHh..",
    ],
}

# The back of each head, for walking away up the screen; long hair falls
# down the back (BACK_HAIR).
HEADS_AWAY = {
    "short": [
        "..HHHHH..",
        ".HHHHHHH.",
        ".HHHHHHH.",
        ".HHHHHHH.",
        ".hHHHHHh.",
        "..sSSSs..",
    ],
    "long": [
        "..HHHHH..",
        ".HHHHHHH.",
        "HHHHHHHHH",
        "HHHHHHHHH",
        "hHHHHHHHh",
        "hhHHHHHhh",
    ],
}
HEADS_AWAY["beard"] = HEADS_AWAY["short"]
BACK_HAIR = {(3, 6): "h", (4, 6): "H", (5, 6): "H", (6, 6): "H", (7, 6): "h", (4, 7): "h", (5, 7): "h", (6, 7): "h"}

BODIES = {
    "down": [".tTTTTTt.", ".tTTTTTt.", ".tTTTTTt.", ".SLLLLLS."],
    "up": ["..TTTTT..", "..TTTTT..", "..TTTTT..", "..LLLLL.."],
    "reach": [".tTTTTTtt", ".tTTTTT.S", ".tTTTTT..", ".SLLLLL.."],
    "reach2": [".tTTTTTt.", ".tTTTTTt.", ".tTTTTTtS", ".SLLLLL.."],
    "bowl": [".tTTTTTt.", ".tTTTTTt.", ".SWWWWWS.", "..LLLLL.."],
    "bite": [".tTTTTTt.", ".tTTTTTt.", ".SWWWWW..", "..LLLLL.."],
}

LEGS = {
    "stand": ["..PPPPP..", "..PP.PP..", "..BB.BB.."],
    "step_a": ["..PPPPP..", "..PP.BB..", "..BB....."],
    "step_b": ["..PPPPP..", "..BB.PP..", ".....BB.."],
    "wide": ["..PPPPP..", ".PP...PP.", ".BB...BB."],
}

# Extra pixels over a pose, at (column, row) of the frame (the outline's
# column is 0, the head's top row 0). Arms raised to hold a load over the
# head (the load is the goods' own sprite, drawn above): hands at the top,
# sleeves down to the shoulders.
ARMS_UP = {(1, 0): "S", (9, 0): "S", (1, 1): "t", (9, 1): "t", (1, 2): "t", (9, 2): "t",
           (1, 3): "t", (9, 3): "t", (1, 4): "t", (9, 4): "t", (2, 5): "t", (8, 5): "t",
           (2, 6): "t", (8, 6): "t"}
# Chopping: the axe swung back behind, then into the tree in front.
SWING = {(0, 6): "A", (0, 7): "A", (0, 8): "A", (1, 7): "K", (2, 7): "a", (3, 7): "a",
         (4, 7): "S", (5, 7): "S"}
STRIKE = {(10, 6): "A", (10, 7): "A", (10, 8): "A", (9, 7): "K", (8, 7): "a", (7, 7): "a",
          (5, 7): "S", (6, 7): "S"}
# Eating: the hand comes up to the mouth with a berry.
BITE = {(8, 5): "R", (8, 6): "t", (8, 7): "S"}

# The poses, in the order of a row: (body, legs, extra pixels, seen from
# behind).
POSES = [
    ("down", "stand", None),  # 0 standing
    ("down", "step_a", None),  # 1, 2 walking
    ("down", "step_b", None),
    ("up", "stand", ARMS_UP),  # 3 carrying, standing
    ("up", "step_a", ARMS_UP),  # 4, 5 carrying, walking
    ("up", "step_b", ARMS_UP),
    ("reach", "stand", None),  # 6, 7 picking (a bush, or goods off the ground)
    ("reach2", "stand", None),
    ("up", "wide", SWING),  # 8, 9 chopping: the swing and the strike
    ("up", "wide", STRIKE),
    ("bowl", "stand", None),  # 10, 11 eating
    ("bite", "stand", BITE),
    ("down", "step_a", None, True),  # 12, 13 walking away
    ("down", "step_b", None, True),
    ("up", "step_a", ARMS_UP, True),  # 14, 15 carrying away
    ("up", "step_b", ARMS_UP, True),
]
PERSON_W, PERSON_H = 11, 16


def person_palette(p):
    hair, skin, shirt, legs = p["hair"], p["skin"], p["shirt"], p["legs"]
    return {
        "H": rgb(hair[0]), "h": rgb(hair[1]), "S": rgb(skin[0]), "s": rgb(skin[1]),
        "e": EYE, "T": rgb(shirt[0]), "t": rgb(shirt[1]), "L": WOOD[1],
        "P": rgb(legs[0]), "B": WOOD[0], "A": STEEL[1], "K": STEEL[0], "a": WOOD[3],
        "W": BASKET[1], "R": BERRY[1],
    }


def person_frame(p, pose):
    body, legs, extra = POSES[pose][:3]
    away = len(POSES[pose]) > 3
    head = (HEADS_AWAY if away else HEADS)[p["style"]]
    rows = [list("." + r + ".") for r in head + BODIES[body] + LEGS[legs]]
    if away and p["style"] == "long":
        extra = {**BACK_HAIR, **(extra or {})}
    for (x, y), ch in (extra or {}).items():
        rows[y][x] = ch
    img = Img(PERSON_W, PERSON_H)
    img.blit(grid(["".join(r) for r in rows], person_palette(p)), 0, 1)
    img.outline()
    return img.shadow(5.5, 14.8, 4.4, 1.3)


CELL = 16  # a frame of colonists.png: 16 x 16, the colonist in its middle


def colonists():
    sheet = Img(CELL * len(POSES), CELL * len(PEOPLE))
    for row, p in enumerate(PEOPLE):
        for pose in range(len(POSES)):
            sheet.blit(person_frame(p, pose), pose * CELL + (CELL - PERSON_W + 1) // 2, row * CELL)
    return sheet


# The meadow's things ------------------------------------------------------------

BUSH_W, BUSH_H = 13, 12
BUSH_LEAVES = [(6.5, 5.8, 5.0), (3.4, 7.6, 3.2), (9.6, 7.6, 3.2)]
BUSH_SPOTS = [(3, 6), (6, 3), (9, 5), (5, 8), (8, 8), (10, 8), (2, 9)]


def bush(stage):
    """0 just picked, 1 in flower, 2 green berries, 3 ripe."""
    rng = random.Random(SEED + 10)
    img = Img(BUSH_W, BUSH_H)
    pal = LEAF if stage else [LEAF[0], LEAF[1], LEAF[1], LEAF[2], LEAF[3]]
    foliage(img, BUSH_LEAVES, pal, rng)
    for i, (x, y) in enumerate(BUSH_SPOTS):
        if stage == 0 and i % 3 == 0:
            img.set(x, y, BARK[2])  # a bare twig
        elif stage == 1:
            img.set(x, y, BLOSSOM[0])
            if i % 2:
                img.set(x + 1, y, BLOSSOM[1])
        elif stage == 2:
            img.set(x, y, UNRIPE[1])
            img.set(x, y + 1, UNRIPE[0])
        elif stage == 3:
            img.set(x, y, BERRY[2])
            img.set(x + 1, y, BERRY[1])
            img.set(x, y + 1, BERRY[1])
            img.set(x + 1, y + 1, BERRY[0])
    img.outline()
    return img.shadow(6.5, 10.9, 5.6, 1.4)


TREE_W, TREE_H = 19, 27
FOOT = 24  # the row the trunk stands on


def trunk(img, x0, x1, top, bottom=FOOT):
    for y in range(top, bottom + 1):
        for x in range(x0, x1 + 1):
            c = BARK[3] if x == x0 else BARK[1] if x == x1 else BARK[2]
            if (y * 3 + x) % 5 == 0:
                c = BARK[1]
            img.set(x, y, c)
    if x1 - x0 >= 2:  # roots
        img.set(x0 - 1, bottom, BARK[1])
        img.set(x1 + 1, bottom, BARK[1])


def stump(img):
    trunk(img, 7, 11, 21)
    for x in range(7, 12):
        img.set(x, 21, CUT[1])
    img.set(9, 21, CUT[2])
    img.set(8, 21, CUT[0])
    img.set(10, 21, CUT[0])
    for x, y in ((5, 24), (13, 23), (14, 24)):
        img.set(x, y, CUT[1])  # chips


def oak_leaves(scale, drop):
    c = [(9.5, 8.5, 7.2), (5.0, 11.5, 4.6), (14.0, 11.5, 4.6), (9.5, 13.0, 5.0), (6.6, 5.4, 4.0), (12.4, 5.4, 4.0)]
    return [(9.5 + (x - 9.5) * scale, 17 + (y - 17) * scale + drop, r * scale) for x, y, r in c]


def pine_leaves(img, scale, drop, dx, rng):
    """Three tiers of a fir, each a triangle, lit from the left."""
    cells = set()
    for i in range(3):
        apex = 1 + i * 5.2
        base = apex + 8.5
        half = 4.0 + i * 1.9
        apex, base, half = 19 + (apex - 19) * scale + drop, 19 + (base - 19) * scale + drop, half * scale
        for y in range(TREE_H):
            if apex <= y + 0.5 <= base:
                w = half * (y + 0.5 - apex) / (base - apex)
                for x in range(TREE_W):
                    if abs(x + 0.5 - 9.5 - dx) <= w:
                        cells.add((x, y))
    for (x, y) in cells:
        side = (x + 0.5 - 9.5 - dx) / 6.0
        v = 2.4 - 1.6 * side + 0.7 * dither(x, y) + rng.uniform(-0.3, 0.3)
        if (y % 3 == 0) and side < 0.3:
            v += 0.7
        img.set(x, y, PINE[max(1, min(4, int(round(v))))])
    for (x, y) in cells:
        if any((x + ox, y + oy) not in cells for ox, oy in ((1, 0), (-1, 0), (0, 1), (0, -1))):
            img.set(x, y, PINE[0])


def tree(kind, stage):
    """kind 0 an oak, 1 a fir. stage: 0 grown, 1 shaken by the axe, 2 young,
    3 a sapling, 4 a shoot from the stump, 5 the stump."""
    rng = random.Random(SEED + 20 + kind)
    img = Img(TREE_W, TREE_H)
    scale = {0: 1.0, 1: 1.0, 2: 0.74, 3: 0.44}.get(stage)
    if stage <= 3:
        top = 15 if stage < 2 else 17 if stage == 2 else 19
        if stage == 3:
            trunk(img, 9, 9, top)
        else:
            trunk(img, 8, 10, top)
        dx = 1 if stage == 1 else 0
        drop = 0 if stage < 2 else (2 if stage == 2 else 4)
        if kind == 0:
            foliage(img, oak_leaves(scale, drop), LEAF, rng, dx=dx)
        else:
            pine_leaves(img, scale, drop, dx, rng)
        if stage == 1:  # leaves shaken loose
            for x, y in ((2, 14), (16, 9), (15, 17)):
                img.set(x, y, LEAF[3] if kind == 0 else PINE[3])
    else:
        stump(img)
        if stage == 4:
            pal = LEAF if kind == 0 else PINE
            for x, y, c in ((9, 20, 2), (9, 19, 2), (8, 18, 3), (10, 18, 3), (8, 19, 1), (10, 19, 1)):
                img.set(x, y, pal[c])
    img.outline()
    return img.shadow(9.5, 24.6, 6.4 if stage <= 2 else 4.0, 1.8)


# Goods: a basket of berries and a pair of logs, on the ground or carried.
# Both are 8 x 19 frames with their pivot at the bottom: on the ground the
# goods are in the bottom rows; carried, in the top rows, so they ride over
# the carrier's head, in their raised hands (the job system keeps carried
# goods where the carrier is).
GOODS_W, GOODS_H = 8, 19
BERRIES = [".RrRRr.", "RRRrRRR", "bBBBBBb", "bBbBbBb", ".bBBBb."]
LOGS = ["CccccD.", "cccccDd", "CccccD.", ".CccccD", ".cccccD"]
GOODS_PAL = {"R": BERRY[1], "r": BERRY[2], "b": BASKET[0], "B": BASKET[2],
             "C": BARK[3], "c": BARK[2], "D": CUT[1], "d": CUT[0]}


def goods(picture, carried):
    img = Img(GOODS_W, GOODS_H)
    top = 1 if carried else GOODS_H - 1 - len(picture)
    img.blit(grid(picture, GOODS_PAL), 0, top)
    img.outline()
    if not carried:
        img.shadow(4, GOODS_H - 0.6, 3.6, 0.9)
    return img


# The stockpile: a yard on the ground where goods come in, and behind it a
# store with a lantern by its door, its woodpile and baskets showing what
# is in store.
STORE_W, STORE_H = 36, 34
STORE_BASE = 31  # the row the store stands on


def store(lit):
    rng = random.Random(SEED + 30)
    img = Img(STORE_W, STORE_H)
    # The wall: logs laid flat, their ends at the corners.
    for y in range(15, STORE_BASE):
        for x in range(2, 34):
            band = (y - 15) % 3
            img.set(x, y, (WOOD[3], WOOD[2], WOOD[1])[band])
            if band == 1 and rng.random() < 0.08:
                img.set(x, y, WOOD[1])
        for x in (2, 33):
            img.set(x, y, CUT[1] if (y - 15) % 3 == 1 else CUT[0])
    # The roof: shingles in rows, overhanging the wall, its ridge at the top.
    for y in range(1, 14):
        inset = max(0, 3 - y // 4)
        for x in range(inset, STORE_W - inset):
            row = (y - 1) // 3
            c = ROOF[2]
            if (y - 1) % 3 == 2:
                c = ROOF[1]
            elif (x + row * 2) % 5 == 0:
                c = ROOF[1]
            elif (y - 1) % 3 == 0 and x < STORE_W * 0.55:
                c = ROOF[3]
            img.set(x, y, c)
    for x in range(3, STORE_W - 3):
        img.set(x, 1, ROOF[4] if x % 2 else ROOF[3])
    for x in range(0, STORE_W):
        img.set(x, 13, WOOD[1])  # the eave's board
        img.set(x, 14, WOOD[0])  # its shadow on the wall
    # The door: planks, a frame, a latch, a step.
    for y in range(19, STORE_BASE):
        for x in range(15, 22):
            edge = x in (15, 21) or y == 19
            img.set(x, y, WOOD[0] if edge else (WOOD[1] if x % 2 else WOOD[2]))
    img.set(20, 25, STEEL[1])
    if lit:
        for y in range(20, STORE_BASE):
            img.set(18, y, LAMPLIGHT[0] if y % 3 else WOOD[1])
    for x in range(14, 23):
        img.set(x, STORE_BASE, STONE[1])
    # The window: shutters open, glass dark by day and warm by night.
    for y in range(18, 24):
        for x in range(25, 31):
            frame = x in (25, 30) or y in (18, 23)
            if frame:
                c = WOOD[0]
            elif lit:
                c = LAMPLIGHT[2] if (x in (27, 28) and y in (20, 21)) else LAMPLIGHT[1]
            else:
                c = GLASS if (x + y) % 5 else STONE[1]
            img.set(x, y, c)
    for y in range(18, 24):
        img.set(24, y, WOOD[2])
        img.set(31, y, WOOD[2])
    # The lantern by the door, on a hook.
    img.set(12, 17, INK)
    img.set(13, 17, INK)
    for y in range(18, 22):
        for x in (11, 12, 13):
            if x == 12 and y in (19, 20):
                img.set(x, y, LAMPLIGHT[2] if lit else STEEL[1])
            elif x == 12 and y == 21 and lit:
                img.set(x, y, LAMPLIGHT[1])
            else:
                img.set(x, y, INK if y in (18, 21) or x != 12 else STEEL[0])
    # The foundation stones.
    for x in range(1, STORE_W - 1):
        img.set(x, STORE_BASE, STONE[1] if (x // 3) % 2 else STONE[0])
    img.outline()
    return img.shadow(18, STORE_BASE + 1.2, 17.5, 2.2)


PILE_W, PILE_H = 16, 14
LOG_END = [".bb.", "bDdb", "bdDb", ".bb."]


def woodpile(level):
    """0 a chopping block with its axe; 1 to 3 more and more logs, ends out."""
    img = Img(PILE_W, PILE_H)
    pal = {"b": BARK[1], "D": CUT[2], "d": CUT[0]}
    if level == 0:
        trunk(img, 6, 9, 8, 12)
        for x in range(6, 10):
            img.set(x, 8, CUT[1])
        for i in range(5):  # the axe's handle, its head in the block
            img.set(9 + i // 2 + (i % 2), 7 - i, WOOD[3])
        img.set(7, 7, STEEL[1])
        img.set(8, 7, STEEL[0])
        img.set(8, 6, STEEL[1])
    else:
        rows = {1: [3], 2: [3, 2], 3: [4, 3, 2, 1]}[level]
        log = grid(LOG_END, pal)
        for tier, n in enumerate(rows):
            x0 = (PILE_W - n * 4 + 1) // 2 - (1 if tier % 2 else 0) + tier
            for k in range(n):
                img.blit(log, x0 + k * 4 - tier, PILE_H - 5 - tier * 3)
    img.outline()
    return img.shadow(8, PILE_H - 0.8, 7.2, 1.2)


BASKET_PIC = [".RrRRr.", "RRRrRRR", "bBBBBBb", "bBbBbBb", "bBBBBBb", ".bbbbb."]
EMPTY_PIC = [".......", ".......", "bBBBBBb", "bkkkkkb", "bBbBbBb", ".bbbbb."]


def baskets(level):
    """0 an empty basket; 1 to 3 baskets of berries."""
    img = Img(PILE_W, PILE_H)
    pal = dict(GOODS_PAL, B=BASKET[2], b=BASKET[0], k=BASKET[0])
    if level == 0:
        img.blit(grid(EMPTY_PIC, pal), 4, PILE_H - 7)
    else:
        spots = [(4, PILE_H - 7), (0, PILE_H - 7), (8, PILE_H - 7)][:level]
        if level == 3:
            spots = [(0, PILE_H - 7), (8, PILE_H - 7), (4, PILE_H - 11)]
        for x, y in spots:
            img.blit(grid(BASKET_PIC, pal), x + 1, y)
        if level >= 2:
            img.set(13, PILE_H - 2, BERRY[1])
            img.set(2, PILE_H - 1, BERRY[0])
    img.outline()
    return img.shadow(8, PILE_H - 0.8, 7.4, 1.2)


POND_W, POND_H = 54, 45  # a unit and a half wider and taller than the water's 5 x 4


def pond(frame):
    """The water within its bank: deeper toward the middle, the far bank's
    shadow on it, lily pads, reeds at two corners; frame 1 moves the
    ripples (systems/looks.luau swaps the two)."""
    rng = random.Random(SEED + 40)
    img = Img(POND_W, POND_H)
    wob = Noise(rng, 6, 5)
    cx, cy, a, b = (POND_W - 1) / 2, (POND_H - 1) / 2, 26.0, 21.6
    bed = Noise(rng, 4, 3)  # the pond's floor: deeper in some places than others
    depth = {}
    for y in range(POND_H):
        for x in range(POND_W):
            u, v = (x - cx) / a, (y - cy) / b
            r = (abs(u) ** 2.4 + abs(v) ** 2.4) ** (1 / 2.4) + (wob.at(x / POND_W, y / POND_H) - 0.5) * 0.1
            d = (1 - r) * 20.0
            if d >= 0:
                depth[(x, y)] = d
    for (x, y), d in depth.items():
        t = dither(x, y)
        if d < 2.6:  # the bank, darker toward the water
            c = BANK[3] if d < 1.0 else BANK[2] if d + t < 1.9 else BANK[1]
        elif d < 3.4:
            c = BANK[0] if y < cy else WATER[3]  # the far bank's lip, the near bank's shallows
        else:
            k = (d - 3.4) * (0.6 + 0.9 * bed.at(x / POND_W, y / POND_H)) + t * 1.2
            c = WATER[3] if k < 1.2 else WATER[2] if k < 4.5 else WATER[1] if k < 9.0 else WATER[0]
            if y < cy - 8 and k < 3.0:
                c = WATER[1]  # the far bank shades the water under it
        img.set(x, y, c)
    # Ripples: short light lines that move a little between the frames.
    rr = random.Random(SEED + 41)
    for _ in range(9):
        while True:
            x, y = rr.randint(10, POND_W - 14), rr.randint(10, POND_H - 10)
            if depth.get((x, y), 0) > 6:
                break
        x += 2 * frame if rr.random() < 0.5 else -frame
        n = rr.randint(3, 5)
        for i in range(n):
            img.set(x + i, y, WATER[4] if 0 < i < n - 1 else WATER[3])
    # Lily pads, one in flower.
    for px, py, flower in ((15, 27, True), (19, 31, False), (37, 15, False)):
        for y in range(py - 2, py + 3):
            for x in range(px - 3, px + 3):
                if math.hypot(x + 0.5 - px, (y + 0.5 - py) * 1.3) <= 2.6 and not (x == px and y < py):
                    img.set(x, y, LEAF[3] if x < px else LEAF[2])
        if flower:
            img.set(px - 1, py - 1, FLOWERS[2])
            img.set(px, py - 1, FLOWERS[0])
    # Reeds at the near left and the far right, cattails on some.
    for bx, by, n in ((6, 36, 6), (44, 9, 5)):
        for i in range(n):
            x = bx + i + (i // 2)
            h = 3 + (i * 7) % 4
            base = by + (i % 2)
            for k in range(h):
                img.set(x, base - k, REED[1] if k < h - 1 else REED[0])
            if i % 2 == 0:
                img.set(x, base - h, REED[2])
                img.set(x, base - h - 1, REED[2])
    return img


YARD_W, YARD_H = 40, 48
YARD_MIDDLE = 32  # the row of the yard's middle, where the stockpile stands


def yard():
    """The stockpile's ground: trodden earth, ragged at the edge, a path worn
    from it up to the store's door, tufts the feet have missed, a few
    stones."""
    rng = random.Random(SEED + 50)
    img = Img(YARD_W, YARD_H)
    wob = Noise(rng, 6, 6)
    tone = Noise(rng, 4, 4)
    cx, cy = (YARD_W - 1) / 2, YARD_MIDDLE
    inside = set()
    for y in range(YARD_H):
        for x in range(YARD_W):
            r = math.hypot((x - cx) / 18.5, (y - cy) / 13.5) + (wob.at(x / YARD_W, y / YARD_H) - 0.5) * 0.3
            path = y < cy and abs(x - cx + 0.8 * math.sin(y / 5.0)) < 2.6 + (cx - 2.6) * (y / YARD_MIDDLE) ** 3 * 0.25 + (wob.at(x / YARD_W, y / YARD_H) - 0.5) * 2
            if r <= 1 or path:
                inside.add((x, y))
                v = 1.6 + (tone.at(x / YARD_W, y / YARD_H) - 0.5) * 1.6 + dither(x, y) * 0.3
                img.set(x, y, DIRT[max(1, min(3, int(v)))])
    for (x, y) in inside:  # a darker rim where the grass begins
        if any((x + ox, y + oy) not in inside for ox, oy in ((1, 0), (-1, 0), (0, 1), (0, -1))):
            img.set(x, y, DIRT[0])
    for _ in range(12):  # tufts the feet have missed
        x, y = rng.choice(sorted(inside))
        if math.hypot((x - cx) / 18.5, (y - cy) / 13.5) > 0.7:
            for dx, dy in ((0, 0), (2, 0), (1, 1)):
                img.set(x + dx, y + dy, GRASS[3])
    for _ in range(7):
        x, y = rng.randint(8, YARD_W - 9), rng.randint(YARD_MIDDLE - 8, YARD_H - 6)
        img.set(x, y, STONE[2])
        img.set(x + 1, y, STONE[1])
        img.set(x, y + 1, STONE[0])
    return img


def ui_panel():
    """The HUD's and the pause screen's panel: dark wood inside a lighter
    frame, a nail in each corner. 16 x 16, nine-sliced at 4."""
    img = Img(16, 16)
    fill = rgb("2A1C14", 236)
    for y in range(16):
        for x in range(16):
            ring = min(x, y, 15 - x, 15 - y)
            if ring == 0:
                c = rgb("1A120C") if (x, y) not in ((0, 0), (15, 0), (0, 15), (15, 15)) else CLEAR
            elif ring == 1:
                c = WOOD[3] if (x == 1 or y == 1) else WOOD[2]
            elif ring == 2:
                c = WOOD[1]
            elif ring == 3:
                c = rgb("1A120C", 236)
            else:
                c = fill
            img.set(x, y, c)
    for x, y in ((1, 1), (14, 1), (1, 14), (14, 14)):
        img.set(x, y, rgb("E2D2A8"))
    return img


# The ground ---------------------------------------------------------------------

# A tuft's pixels: a v, a w, a lone blade, a peak.
TUFTS = [[(0, 0), (2, 0), (1, 1)], [(0, 0), (1, 1), (2, 0), (3, 1), (4, 0)], [(1, 0), (1, 1)], [(1, 0), (0, 1), (2, 1)]]


def ground():
    """The meadow: grass in light and dark patches, blades, flowers, and a few
    stones; round it, outside where the colonists walk, longer and darker
    grass, so the meadow reads as a clearing."""
    rng = random.Random(SEED)
    W, H = GROUND[0] * PPU, GROUND[1] * PPU
    img = Img(W, H)
    big = Noise(rng, 7, 4)
    small = Noise(rng, 22, 13)
    mx, my = (GROUND[0] - MEADOW[0]) / 2 * PPU, (GROUND[1] - MEADOW[1]) / 2 * PPU
    level = [[0] * W for _ in range(H)]

    def outside(x, y):
        ox = max(mx - x, x + 1 - (W - mx), 0)
        oy = max(my - y, y + 1 - (H - my), 0)
        return math.hypot(ox, oy) / PPU  # units beyond the meadow's edge

    for y in range(H):
        for x in range(W):
            n = 0.7 * big.at(x / W, y / H) + 0.3 * small.at(x / W, y / H)
            v = 0.5 + n * 3.4 - min(outside(x, y), 2.5) * 0.75 + dither(x, y) * 0.45
            level[y][x] = max(0, min(4, int(v)))
            img.set(x, y, GRASS[level[y][x]])
    # Tufts of grass, a shade lighter than the ground under them (a few a
    # shade darker), each with a dark pixel under it; beyond the meadow,
    # long blades instead.
    for _ in range(W * H // 40):
        x, y = rng.randrange(W), rng.randrange(1, H - 3)
        L = level[y][x]
        if outside(x, y) > 0.3:
            for k in range(rng.randint(3, 5)):
                img.set(x, y + k, GRASS[min(4, L + (2 if k == 0 else 1))])
            continue
        light = rng.random() < 0.8
        c = GRASS[min(4, L + 1)] if light else GRASS[max(0, L - 1)]
        if light and L >= 2 and rng.random() < 0.25:
            c = BRIGHT_GRASS
        for dx, dy in rng.choice(TUFTS):
            img.set(x + dx, y + dy, c)
        if light:
            img.set(x + 1, y + 2, GRASS[max(0, L - 1)])
    # Flowers, only on the meadow: a bright pixel on a dark one, in little
    # clusters, and a few patches thick with one colour.
    for k in range(46):
        cx, cy = rng.randrange(int(mx) + 6, int(W - mx) - 6), rng.randrange(int(my) + 6, int(H - my) - 6)
        c = FLOWERS[rng.choice((0, 0, 1, 1, 2, 3))]
        patch = k < 6
        for _ in range(rng.randint(14, 22) if patch else rng.randint(3, 6)):
            x, y = cx + rng.randint(-9, 9) if patch else cx + rng.randint(-5, 5), cy + rng.randint(-5, 5) if patch else cy + rng.randint(-3, 3)
            img.set(x, y + 1, GRASS[0])
            img.set(x, y, c)
    # Pebbles, and a few rocks with a shadow.
    for _ in range(16):
        x, y = rng.randrange(8, W - 8), rng.randrange(8, H - 8)
        img.set(x, y, STONE[2])
        img.set(x + 1, y, STONE[1])
        img.set(x, y + 1, STONE[1])
        img.set(x + 1, y + 1, STONE[0])
    rock = grid([".hHh.", "hHHMm", "MMMmm", ".mmm."], {"H": STONE[2], "h": STONE[1], "M": STONE[1], "m": STONE[0]})
    rock.outline().shadow(2.5, 4.6, 3.2, 1.0)
    for _ in range(7):
        img.blit(rock, rng.randrange(int(mx), int(W - mx) - 6), rng.randrange(int(my), int(H - my) - 6))
    return img


# The sheet and its sidecars -----------------------------------------------------


def meadow_frames():
    """Every frame of meadow.png in index order (systems/looks.luau counts in
    it), each (name, picture, pivot)."""
    frames = []
    for s, name in enumerate(("picked", "in flower", "green berries", "ripe")):
        frames.append(("bush " + name, bush(s), (0.5, 1.6 / BUSH_H)))
    stages = ("grown", "shaken", "young", "sapling", "shoot", "stump")
    for k, kind in enumerate(("oak", "fir")):
        for s, name in enumerate(stages):
            frames.append(("%s %s" % (kind, name), tree(k, s), (0.5, (TREE_H - FOOT - 0.5) / TREE_H)))
    for name, pic, carried in (("berries", BERRIES, False), ("logs", LOGS, False),
                               ("berries carried", BERRIES, True), ("logs carried", LOGS, True)):
        frames.append((name, goods(pic, carried), (0.5, 0.0)))
    for lit in (False, True):
        frames.append(("store" + (" at night" if lit else ""), store(lit), (0.5, (STORE_H - STORE_BASE - 0.5) / STORE_H)))
    for n in range(4):
        frames.append(("woodpile %d" % n, woodpile(n), (0.5, 1.5 / PILE_H)))
    for n in range(4):
        frames.append(("baskets %d" % n, baskets(n), (0.5, 1.5 / PILE_H)))
    for f in range(2):
        frames.append(("pond %d" % f, pond(f), (0.5, 0.5)))
    frames.append(("yard", yard(), (0.5, (YARD_H - YARD_MIDDLE - 0.5) / YARD_H)))
    return frames


def pack(frames, width=256):
    """Shelf-packs the frames into one sheet; returns it and each frame's rect."""
    x = y = shelf = 0
    rects = []
    for _, img, _ in frames:
        if x + img.w > width:
            x, y, shelf = 0, y + shelf + 1, 0
        rects.append((x, y, img.w, img.h))
        x += img.w + 1
        shelf = max(shelf, img.h)
    height = y + shelf
    sheet = Img(width, height)
    for (_, img, _), (rx, ry, _, _) in zip(frames, rects):
        sheet.blit(img, rx, ry)
    return sheet, rects


def meadow_sidecar(frames, rects):
    lines = [
        "# The meadow's things, one sheet: written by tools/art.py with the",
        "# picture, so change and run that rather than editing this. Frames are",
        "# counted from 0 in the order below, which is the order",
        "# systems/looks.luau counts them in; the pivot is where the entity",
        "# stands (a tree's trunk foot, a bush's base), as fractions of the",
        "# frame, y up from the bottom. 9 pixels a world unit.",
        "pixels_per_unit = %d" % PPU,
        "",
    ]
    for i, ((name, _, pivot), r) in enumerate(zip(frames, rects)):
        lines += ["# %d: %s" % (i, name), "[[frames]]", "rect = [%d, %d, %d, %d]" % r,
                  "pivot = [%.4f, %.4f]" % pivot, ""]
    return "\n".join(lines)


COLONISTS_SIDECAR = """# The colonists: a row each, in tools/art.py's PEOPLE order (Ada, Bram,
# Cleo, Dov), and in each row a frame of 16 x 16 pixels for each of the 16
# poses, drawn facing right: 0 standing; 1-2 walking; 3 carrying, standing;
# 4-5 carrying, walking; 6-7 picking; 8-9 chopping; 10-11 eating; 12-13
# walking away (up the screen) and 14-15 carrying away, seen from behind.
# systems/looks.luau picks row * 16 + pose and mirrors it to face left. The
# pivot is at the feet.
# Written by tools/art.py with the picture; run that rather than editing this.
grid = [16, 4]
pivot = [0.5, 0.125]
pixels_per_unit = 9
"""

GROUND_SIDECAR = """# The meadow's grass, one picture of 40 x 24 world units (360 x 216 pixels,
# 9 a unit, on a sheet padded to 512 x 256): the 32 x 18 the colonists walk
# on and a border of long grass. Drawn by tools/art.py (SEED picks other
# grass); scenes/main/ground.toml shows it.
pixels_per_unit = 9

[[frames]]
rect = [0, 0, %d, %d]
""" % (GROUND[0] * PPU, GROUND[1] * PPU)

PANEL_SIDECAR = """# The wooden panel the HUD (scenes/main/hud.toml) and the pause screen
# (scenes/main/paused.toml) sit on: 16 x 16 pixels, nine-sliced at 4 (their
# UiPanel's slice), drawn at twice its pixels (scale). Drawn by tools/art.py.
grid = [1, 1]
"""


if __name__ == "__main__":
    write_png("sprites/ground.png", ground())
    write_text("sprites/ground.toml", GROUND_SIDECAR)
    write_png("sprites/colonists.png", colonists())
    write_text("sprites/colonists.toml", COLONISTS_SIDECAR)
    frames = meadow_frames()
    sheet, rects = pack(frames)
    write_png("sprites/meadow.png", sheet)
    write_text("sprites/meadow.toml", meadow_sidecar(frames, rects))
    write_png("ui/panel.png", ui_panel())
    write_text("ui/panel.toml", PANEL_SIDECAR)
    print("wrote sprites/ground.png, sprites/colonists.png, sprites/meadow.png, ui/panel.png and their sidecars")
