"""The platformer's art: every picture the game draws, made by this file.

Run it once and the PNGs (and their sidecars, which say how each picture is
cut into frames) land in assets/, where the engine picks them up:

    python tools/art.py

It needs only Python's standard library, and it writes the same files
every run (its noise is seeded). The PNGs are committed beside it, so you
need Python only to change them.

What it writes, and who uses it:

- assets/sprites.png (+ .toml): everything in the level, one atlas. The
  hero (scenes/level/player.toml, played by assets/anim/hero.toml), the
  coin (prefabs/coin.toml, assets/anim/coin.toml), the flag
  (scenes/level/goal*.toml, assets/anim/flag.toml), the spikes
  (prefabs/spikes.toml), the ground's tiles, which systems/looks.luau
  lays over every solid block, and the sparkle assets/fx/sparkle.toml
  bursts into.
- assets/backdrop.png (+ .toml): the sky (scenes/level/sky.toml) and the
  layers behind the level that scroll slower than it (scenes/level/
  clouds.toml, hills.far.toml, hills.near.toml).
- assets/ui/panel.png (+ .toml): the HUD's and the pause panel's frame.

What to change first: the colours below. Every colour has a name; change
one, run this file, and everything drawn with it changes. Sizes are in
pixels, PPU (15) to a world unit: the camera shows 12 units top to
bottom, so at 960 x 540 each pixel of the art is 3 on the screen.
"""

import math
import os
import random
import struct
import zlib

ASSETS = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets")

PPU = 15  # pixels a world unit

# The palette ----------------------------------------------------------------

OUTLINE = "2A1E30"  # the dark line round the hero, the coin, the flag

# The hero: a cap, hair, a tunic, a belt, trousers, boots.
CAP = "D9483B"
CAP_SHADE = "A3303A"
CAP_SHINE = "F27B5C"
HAIR = "5A3A2A"
SKIN = "F6C9A0"
SKIN_SHADE = "D99A78"
TUNIC = "F2A541"  # the hero's colour before there was art
TUNIC_SHADE = "C77A2A"
BELT = "6B4226"
TROUSERS = "3E5C9A"
TROUSERS_SHADE = "2C4274"
BOOTS = "4A2E26"

# Coins.
GOLD = "FFD84A"
GOLD_SHADE = "D49A22"
GOLD_SHINE = "FFF7C2"
GOLD_EDGE = "8A5414"

# The flag and its pole.
CLOTH = "4ADE80"
CLOTH_SHADE = "23A85A"
CLOTH_SHINE = "A8F5C4"
POLE = "F2F2F2"
POLE_SHADE = "A9AFBF"

# Spikes: bright steel with red tips, on an iron plate.
STEEL = "D4DCE6"
STEEL_SHINE = "FFFFFF"
STEEL_SHADE = "7C8798"
SPIKE_LINE = "262A36"
SPIKE_TIP = "E04848"
SPIKE_TIP_SHADE = "9E2A3A"
IRON = "4A4F63"
IRON_SHADE = "33374A"

# The ground: grass over earth with stones in it.
GRASS_SHINE = "A6E86A"
GRASS = "5CC33D"
GRASS_SHADE = "2F8A3A"
FLOWERS = [[], ["FFFFFF"], ["F48FB1", "FFE066"]]  # on each grass tile
EARTH = "94633F"
EARTH_SPECK = "AD7B52"
EARTH_SHADE = "734A30"
EARTH_EDGE = "3E2618"  # a block's sides and underside
STONE_SHINE = "D2C6B8"
STONE = "B3A496"
STONE_SHADE = "7D7068"
WOOD = "C08A52"
WOOD_SHADE = "8A5A32"

# The sky, top to bottom, and the hills behind the level.
SKY = [(0.0, "2E64B8"), (0.35, "4F8FD8"), (0.62, "8CC4EC"), (0.72, "C6E8F2"), (1.0, "E6F4E4")]
CLOUD = "FFFFFF"
CLOUD_SHADE = "D4E6F6"
FAR_HILL = "A9CFDC"
FAR_HILL_SHINE = "C4E1EA"
FAR_HILL_SHADE = "8DB9CC"
NEAR_HILL = "79BE8C"
NEAR_HILL_SHINE = "9CD7A4"
NEAR_HILL_SHADE = "4F8F72"
TREE = "4E9E6A"
TREE_SHADE = "3A7D58"

# The UI's panels: a deep blue, edged in the coins' cream.
PANEL = "1D2440"
PANEL_EDGE = "F6E7C1"
PANEL_LINE = "0E1222"


# Writing PNGs ---------------------------------------------------------------

CLEAR = (0, 0, 0, 0)


def rgba(hexcode, a=255):
    return (int(hexcode[0:2], 16), int(hexcode[2:4], 16), int(hexcode[4:6], 16), a)


def mix(a, b, t):
    return tuple(int(round(a[i] + (b[i] - a[i]) * t)) for i in range(4))


def png(name, rows):
    """Writes assets/<name>.png from rows of (r, g, b, a) tuples. Each row is
    stored as its differences from the pixel to its left (PNG's "sub"
    filter), which packs flat colour small."""
    h, w = len(rows), len(rows[0])
    out = bytearray()
    for row in rows:
        out.append(1)
        left = (0, 0, 0, 0)
        for px in row:
            out += bytes((px[i] - left[i]) & 0xFF for i in range(4))
            left = px

    def chunk(kind, data):
        c = struct.pack(">I", len(data)) + kind + data
        return c + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    data = b"\x89PNG\r\n\x1a\n"
    data += chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
    data += chunk(b"IDAT", zlib.compress(bytes(out), 9))
    data += chunk(b"IEND", b"")
    write(name + ".png", data)


def write(path, data):
    full = os.path.join(ASSETS, path)
    os.makedirs(os.path.dirname(full), exist_ok=True)
    if isinstance(data, str):
        data = data.encode()
    with open(full, "wb") as f:
        f.write(data)


class Pic:
    """A w x h picture, clear to start, that the drawing below writes into."""

    def __init__(self, w, h):
        self.w, self.h = w, h
        self.px = [[CLEAR] * w for _ in range(h)]

    def get(self, x, y):
        if 0 <= x < self.w and 0 <= y < self.h:
            return self.px[y][x]
        return CLEAR

    def set(self, x, y, c):
        if 0 <= x < self.w and 0 <= y < self.h:
            self.px[y][x] = rgba(c) if isinstance(c, str) else c

    def outline(self, c):
        """A line round everything drawn: each clear pixel beside a drawn one."""
        line = [(x, y) for y in range(self.h) for x in range(self.w)
                if self.px[y][x][3] == 0 and any(self.get(x + dx, y + dy)[3] > 0
                                                  for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)))]
        for x, y in line:
            self.set(x, y, c)


def grid(picture, palette):
    """A picture as strings, one character a pixel, looked up in palette."""
    p = Pic(len(picture[0]), len(picture))
    for y, line in enumerate(picture):
        for x, ch in enumerate(line):
            if palette[ch] is not None:
                p.set(x, y, palette[ch])
    return p


# The hero: 16 x 18, facing right --------------------------------------------
#
# Its box (the Collider) is 12 x 18 pixels, the middle of the frame; arms and
# legs swing a little past it. Each frame is the head, a body (the arms),
# and legs, put together by hero_frame(). systems/looks.luau mirrors it to
# face left.

HERO_PAL = {
    ".": None, "o": OUTLINE, "C": CAP, "c": CAP_SHADE, "w": CAP_SHINE, "H": HAIR,
    "S": SKIN, "s": SKIN_SHADE, "E": OUTLINE, "T": TUNIC, "t": TUNIC_SHADE,
    "B": BELT, "P": TROUSERS, "p": TROUSERS_SHADE, "K": BOOTS,
}

HEAD = [
    "................",
    ".....ooooo......",
    "....oCCwwCo.....",
    "...oCCCCCwCo....",
    "...oCCCCCCCCooo.",
    "...occccccccccco",
    "...oHHSSSSESSo..",
    "...oHSSSSSESSo..",
    "....osSSSSSSo...",
    ".....oossso.....",
]

BODIES = {
    # Arms down at the sides.
    "rest": [
        "....oTTTTTTo....",
        "...oTTTTTTTto...",
        "...oStTTTTtSo...",
        "....oBBBBBBo....",
    ],
    # Swinging: the front arm forward, the back one behind.
    "swing": [
        "....oTTTTTTo....",
        "...oTTTTTTTtoo..",
        "..oStTTTTTtSSo..",
        "...ooBBBBBBoo...",
    ],
    "swing2": [
        "....oTTTTTTo....",
        "..ooTTTTTTTto...",
        "..oSStTTTTttSo..",
        "...ooBBBBBBo....",
    ],
    # Both arms up, for the jump.
    "up": [
        "..oSoTTTTTToSo..",
        "...otTTTTTTto...",
        "....oTTTTTTo....",
        "....oBBBBBBo....",
    ],
    # Arms out to the sides, for the fall.
    "out": [
        "....oTTTTTTo....",
        "..ooTTTTTTTtoo..",
        ".oSStTTTTTTtSSo.",
        "..oooBBBBBBooo..",
    ],
}

LEGS = {
    "stand": [
        "....oPPpoPPo....",
        "....oPPpoPPo....",
        "...oKKKooKKKo...",
        "...oooo..oooo...",
    ],
    "run1": [
        "...oPPpooPPo....",
        "..oPPpo..oPPo...",
        ".oKKKo....oKKo..",
        ".oooo.....ooo...",
    ],
    "run2": [
        "....oPPpPPo.....",
        "....oPPoPPo.....",
        "...oKKKoKKo.....",
        "...ooooooo......",
    ],
    "run3": [
        "....oPPooPPpo...",
        "...oPPo..opPPo..",
        "..oKKo....oKKKo.",
        "..ooo.....oooo..",
    ],
    "tuck": [
        "...oPPpppPPo....",
        "...oKKKooKKKo...",
        "...oooo..oooo...",
        "................",
    ],
    "dangle": [
        "....oPPpoPPo....",
        "...oPPpo.oPPo...",
        "...oKKo..oKKo...",
        "...ooo....ooo...",
    ],
}


def hero_frame(body, legs, crouch=0):
    """The head, a body, and legs stacked; crouch drops the head and body by
    that many pixels (a landing's squash), the legs staying put."""
    rows = HEAD + BODIES[body] + LEGS[legs]
    assert len(rows) == 18 and all(len(r) == 16 for r in rows), (body, legs)
    if crouch:
        top = HEAD + BODIES[body]
        rows = ["." * 16] * crouch + top[:len(top) - crouch] + LEGS[legs]
    return grid(rows, HERO_PAL)


def hero_frames():
    breathe = hero_frame("rest", "stand", crouch=1)
    return [
        hero_frame("rest", "stand"),  # idle
        breathe,  # idle, breathing out
        hero_frame("swing", "run1"),  # run
        hero_frame("rest", "run2", crouch=1),
        hero_frame("swing2", "run3"),
        hero_frame("rest", "run2", crouch=1),
        hero_frame("up", "tuck"),  # jump
        hero_frame("out", "dangle"),  # fall
        hero_frame("rest", "stand", crouch=2),  # land
    ]


# A coin: 9 x 9, eight frames of a turn -------------------------------------

COIN = 9
COIN_FRAMES = 8


def coin_frame(k):
    """The coin turned k eighths of a half turn: an ellipse as wide as the
    turn shows, gold, its rim darker, a shine at its top left."""
    p = Pic(COIN, COIN)
    w = max(1.0, COIN * abs(math.cos(math.pi * k / COIN_FRAMES)))
    rx, ry = w / 2.0, COIN / 2.0
    cx = cy = COIN / 2.0
    inside = lambda x, y, s=1.0: ((x + 0.5 - cx) / (rx * s)) ** 2 + ((y + 0.5 - cy) / (ry * s)) ** 2 <= 1.0
    for y in range(COIN):
        for x in range(COIN):
            if not inside(x, y):
                continue
            edge = not all(inside(x + dx, y + dy) for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)))
            if edge:
                p.set(x, y, GOLD_EDGE)
            elif w > 4 and not inside(x, y, 0.62):
                p.set(x, y, GOLD_SHADE if x + 0.5 > cx else GOLD)
            else:
                p.set(x, y, GOLD)
    if w >= 3:
        # The shine, and the face's mark (a darker bar down the middle).
        sx = int(cx - rx * 0.45)
        p.set(sx, 2, GOLD_SHINE)
        p.set(sx, 3, GOLD_SHINE)
        if w > 5:
            for y in (3, 4, 5):
                p.set(int(cx), y, GOLD_SHADE)
    else:
        for y in range(1, COIN - 1):
            p.set(int(cx), y, GOLD_SHADE if y > 4 else GOLD)
    return p


# The flag: cloth 17 x 11 in four frames of a wave, and its pole -------------

CLOTH_W, CLOTH_H = 17, 11
FLAG_FRAMES = 4


def cloth_frame(k):
    """The cloth waving: each column lifted by a sine that moves along it,
    more the farther from the pole; lit where the wave rises."""
    p = Pic(CLOTH_W, CLOTH_H)
    for x in range(CLOTH_W - 1):
        along = x / (CLOTH_W - 2)
        phase = 2 * math.pi * (x / 10.0) - k * math.pi / 2
        dy = int(round(1.4 * math.sin(phase) * along))
        slope = math.cos(phase)
        top, bottom = 2 + dy, 8 + dy
        for y in range(top, bottom + 1):
            if slope > 0.45:
                c = CLOTH_SHINE if y == top else CLOTH
            elif slope < -0.45:
                c = CLOTH_SHADE
            else:
                c = CLOTH
            p.set(x, y, c)
    p.outline(OUTLINE)
    return p


POLE_W, POLE_H = 6, 45  # 3 units tall, as the flag's Collider


def pole():
    p = Pic(POLE_W, POLE_H)
    # A gold knob on top.
    knob = [".oooo.", "oGgGGo", "oGGGGo", ".oooo."]
    for y, line in enumerate(knob):
        for x, ch in enumerate(line):
            if ch != ".":
                p.set(x, y, {"o": GOLD_EDGE, "G": GOLD, "g": GOLD_SHINE}[ch])
    for y in range(4, POLE_H - 3):
        p.set(1, y, OUTLINE)
        p.set(2, y, POLE)
        p.set(3, y, POLE_SHADE)
        p.set(4, y, OUTLINE)
    # A stone foot, standing on the ledge.
    for y in range(POLE_H - 3, POLE_H):
        for x in range(POLE_W):
            edge = x in (0, POLE_W - 1) or y in (POLE_H - 3, POLE_H - 1)
            p.set(x, y, OUTLINE if edge else (STONE if x < 3 else STONE_SHADE))
    return p


def sparkle():
    """A spark of light, 5 x 5: what a taken coin bursts into (white, so the
    effect's colours, assets/fx/sparkle.toml, tint it)."""
    return grid(["..w..", "..W..", "wWWWw", "..W..", "..w.."],
                {".": None, "W": (255, 255, 255, 255), "w": (255, 255, 255, 140)})


# The spikes: 28 x 10, four steel teeth with red tips on an iron plate -------

TOOTH = 7  # pixels across a tooth's foot
TOOTH_H = 7  # and from its foot to its point
SPIKES_W, SPIKES_H = 4 * TOOTH, TOOTH_H + 3


def spikes():
    p = Pic(SPIKES_W, SPIKES_H)
    for i in range(SPIKES_W // TOOTH):
        c = TOOTH * i + TOOTH / 2.0
        # A pixel wide at the point, the whole foot at the plate.
        for r in range(TOOTH_H):
            hw = 0.5 + r * (TOOTH / 2.0 - 0.5) / (TOOTH_H - 1)
            for x in range(TOOTH * i, TOOTH * (i + 1)):
                d = x + 0.5 - c
                if abs(d) > hw:
                    continue
                if r < 3:
                    col = SPIKE_TIP if d <= 0 else SPIKE_TIP_SHADE
                elif d < -hw + 1.0:
                    col = STEEL_SHINE
                elif d <= 0:
                    col = STEEL
                else:
                    col = STEEL_SHADE
                p.set(x, r + 1, col)
    p.outline(SPIKE_LINE)
    # The plate they stand on, with a rivet under each tooth.
    for x in range(SPIKES_W):
        p.set(x, SPIKES_H - 2, IRON)
        p.set(x, SPIKES_H - 1, IRON_SHADE)
    for x in range(TOOTH // 2, SPIKES_W, TOOTH):
        p.set(x, SPIKES_H - 2, STEEL_SHADE)
    return p


# The ground's tiles: a unit (15 x 15) each ----------------------------------
#
# systems/looks.luau lays them over every solid block: a grass tile along
# the top, earth below, a dark edge down each side and along the underside
# that no other block covers, and grass curling over the top corners. The
# edges are the same all along, so they stretch to any length.

T = PPU
BLADES = 3  # pixels of grass blades above a grass tile's top


def earth(p, rng, oy=0, stones=None):
    """Earth into p from row oy down: a few specks and darker clumps, and
    now and then a pebble with its shadow under it."""
    for y in range(oy, p.h):
        for x in range(T):
            r = rng.random()
            p.set(x, y, EARTH_SPECK if r < 0.07 else EARTH_SHADE if r < 0.15 else EARTH)
    for _ in range(rng.randint(3, 5)):
        x, y = rng.randrange(T - 1), rng.randrange(oy, p.h - 1)
        for dx, dy in ((0, 0), (1, 0), (0, 1), (1, 1)):
            p.set(x + dx, y + dy, EARTH_SHADE)
        p.set(x, y, EARTH_SPECK)
    if stones:
        # A pebble: lit on top, round at its top corners, a shadow under it.
        sw, sh = stones
        sx, sy = rng.randint(1, T - sw - 1), rng.randint(oy + 1, p.h - sh - 2)
        for y in range(sy, sy + sh):
            for x in range(sx, sx + sw):
                if y == sy and sw > 2 and x in (sx, sx + sw - 1):
                    continue
                p.set(x, y, STONE_SHINE if y == sy else STONE_SHADE if y == sy + sh - 1 else STONE)
        for x in range(sx, sx + sw):
            p.set(x, sy + sh, EARTH_SHADE)


def grass_tile(n):
    """Grass along a block's top: blades above it (the top BLADES rows of
    the frame), a lit edge, the turf, its ragged foot over the earth."""
    rng = random.Random(100 + n)
    p = Pic(T, T + BLADES)
    earth(p, rng, BLADES + 4)
    foot = [rng.choice((5, 6, 6, 7)) for _ in range(T)]
    for x in range(T):
        for y in range(BLADES, BLADES + foot[x]):
            p.set(x, y, GRASS)
        p.set(x, BLADES, GRASS_SHINE)
        p.set(x, BLADES + foot[x] - 1, GRASS_SHADE)
        # The turf's shadow on the earth under it.
        p.set(x, BLADES + foot[x], EARTH_SHADE)
        if rng.random() < 0.25:
            p.set(x, BLADES + 2 + rng.randint(0, 2), GRASS_SHADE)
    # Blades: tufts of one to three pixels standing up.
    x = rng.randint(0, 3)
    while x < T:
        height = rng.randint(1, BLADES)
        for y in range(BLADES - height, BLADES):
            p.set(x, y, GRASS if y > BLADES - height else GRASS_SHINE)
        x += rng.randint(2, 4)
    # A flower on a stem, on some of the tiles.
    for colour in FLOWERS[n]:
        x = rng.randint(2, T - 3)
        p.set(x, 1, GRASS_SHADE)
        p.set(x, 2, GRASS_SHADE)
        p.set(x, 0, colour)
    return p


# The earth tiles' pebbles (width, height), one kind a tile; None: plain.
PEBBLES = [None, None, (5, 3), None, (3, 2)]


def earth_tile(n):
    rng = random.Random(200 + n)
    p = Pic(T, T)
    earth(p, rng, stones=PEBBLES[n])
    return p


EDGE_W = 3


def side_edge():
    """A block's left side, the same all the way down so it stretches to
    any height (mirrored for the right side)."""
    p = Pic(EDGE_W, T)
    for y in range(T):
        p.set(0, y, EARTH_EDGE)
        p.set(1, y, mix(rgba(EARTH_EDGE), rgba(EARTH_SHADE), 0.5))
        p.set(2, y, EARTH_SHADE)
    return p


def underside():
    """A block's underside, the same all the way along (it stretches)."""
    p = Pic(T, EDGE_W)
    for x in range(T):
        p.set(x, 0, EARTH_SHADE)
        p.set(x, 1, mix(rgba(EARTH_EDGE), rgba(EARTH_SHADE), 0.5))
        p.set(x, 2, EARTH_EDGE)
    return p


CORNER_W, CORNER_H = 5, BLADES + 9


def grass_corner():
    """Grass curling over a block's top left corner and a little way down
    its side (mirrored for the right corner)."""
    rows = [
        ".....",
        ".g...",
        ".Gg..",
        ".gggg",
        "gGGGG",
        "dGGGG",
        "dGGGG",
        "dGGGG",
        "dGGGd",
        "dGGd.",
        ".dd..",
        ".d...",
    ]
    return grid(rows, {".": None, "g": GRASS_SHINE, "G": GRASS, "d": GRASS_SHADE})


def plank():
    """A one-way platform's plank, 15 x 6: what the hero jumps up through."""
    p = Pic(T, 6)
    for x in range(T):
        p.set(x, 0, OUTLINE)
        p.set(x, 1, WOOD)
        for y in (2, 3):
            p.set(x, y, WOOD_SHADE if x in (0, 7) else WOOD)
        p.set(x, 4, WOOD_SHADE)
        p.set(x, 5, OUTLINE)
    p.set(3, 2, OUTLINE)
    p.set(11, 2, OUTLINE)
    return p


GRASS_TILES, EARTH_TILES = len(FLOWERS), len(PEBBLES)


# The atlas ------------------------------------------------------------------


def pack(frames, width):
    """Frames left to right, a new row when one is full, a pixel apart, on a
    sheet whose sides are powers of two (as a texture's are); returns the
    sheet and each frame's rect, in order."""
    rects, x, y, row_h = [], 0, 0, 0
    for f in frames:
        if x + f.w > width:
            x, y, row_h = 0, y + row_h + 1, 0
        rects.append((x, y, f.w, f.h))
        x += f.w + 1
        row_h = max(row_h, f.h)
    pow2 = lambda n: 1 << (n - 1).bit_length()
    sheet = Pic(pow2(width), pow2(y + row_h))
    for f, (fx, fy, _, _) in zip(frames, rects):
        for j in range(f.h):
            for i in range(f.w):
                if f.px[j][i][3]:
                    sheet.px[fy + j][fx + i] = f.px[j][i]
    return sheet, rects


def sidecar(rects, header, clips, pivots=None):
    """The sidecar beside an atlas: each frame's rect (and its pivot, the
    point the entity's position is, when not the middle), then the clips."""
    lines = header + [""]
    for i, r in enumerate(rects):
        lines += ["[[frames]]", "rect = [%d, %d, %d, %d]" % r]
        if pivots and i in pivots:
            lines.append("pivot = [%g, %g]" % pivots[i])
        lines.append("")
    for name, keys in clips:
        lines.append("[clips.%s]" % name)
        lines += keys
        lines.append("")
    return "\n".join(lines)


def sprites():
    groups = [
        ("hero", hero_frames()),
        ("coin", [coin_frame(k) for k in range(COIN_FRAMES)]),
        ("cloth", [cloth_frame(k) for k in range(FLAG_FRAMES)]),
        ("pole", [pole()]),
        ("spikes", [spikes()]),
        ("grass", [grass_tile(n) for n in range(GRASS_TILES)]),
        ("earth", [earth_tile(n) for n in range(EARTH_TILES)]),
        ("edge", [side_edge()]),
        ("corner", [grass_corner()]),
        ("underside", [underside()]),
        ("plank", [plank()]),
        ("sparkle", [sparkle()]),
    ]
    frames, first = [], {}
    for name, fs in groups:
        first[name] = len(frames)
        frames += fs
    sheet, rects = pack(frames, 128)
    png("sprites", sheet.px)
    f = first
    header = [
        "# Everything in the level, one atlas: written by tools/art.py with",
        "# sprites.png, so change that and run it rather than editing this.",
        "# %d pixels a world unit. The frames, from 0:" % PPU,
        "#   %d-%d the hero: idle (2), run (4), jump, fall, land" % (f["hero"], f["coin"] - 1),
        "#   %d-%d the coin turning" % (f["coin"], f["cloth"] - 1),
        "#   %d-%d the flag's cloth waving" % (f["cloth"], f["pole"] - 1),
        "#   %d the flag's pole, %d the spikes" % (f["pole"], f["spikes"]),
        "#   %d-%d grass tiles, %d-%d earth tiles, %d a side, %d a grass corner,"
        % (f["grass"], f["earth"] - 1, f["earth"], f["edge"] - 1, f["edge"], f["corner"]),
        "#   %d an underside, %d a plank (systems/looks.luau lays these)" % (f["underside"], f["plank"]),
        "#   %d a sparkle (assets/fx/sparkle.toml)" % f["sparkle"],
        "# The spikes' pivot is the middle of their foot, so prefabs/spikes.toml",
        "# stands them on the ground where its Transform is.",
        "pixels_per_unit = %d" % PPU,
    ]
    h = f["hero"]
    clips = [
        ("idle", ["frames = [%d, %d]" % (h, h + 1), "fps = 2"]),
        ("run", ["from = %d" % (h + 2), "to = %d" % (h + 5), "fps = 12"]),
        ("jump", ["frames = [%d]" % (h + 6)]),
        ("fall", ["frames = [%d]" % (h + 7)]),
        ("land", ["frames = [%d, %d]" % (h + 8, h), "fps = 12", "loop = false"]),
        ("coin", ["from = %d" % f["coin"], "to = %d" % (f["cloth"] - 1), "fps = 12"]),
        ("flag", ["from = %d" % f["cloth"], "to = %d" % (f["pole"] - 1), "fps = 8"]),
    ]
    write("sprites.toml", sidecar(rects, header, clips, {f["spikes"]: (0.5, 0)}))
    return first


# The backdrop -----------------------------------------------------------------
#
# Behind the level: the sky's colours top to bottom (one column, which
# scenes/level/sky.toml stretches across the level), then three strips 24
# units wide that repeat end to end (clouds, far hills, near hills).
# Each strip's hills and clouds are made from waves that fit the width a
# whole number of times, so its right end meets its left.

STRIP_W = 24 * PPU
SKY_H = 16 * PPU  # the scene's bounds: 16 units top to bottom
CLOUDS_H, FAR_H, NEAR_H = 4 * PPU, 5 * PPU, 8 * PPU


def sky_column():
    p = Pic(1, SKY_H)
    for y in range(SKY_H):
        t = y / (SKY_H - 1)
        for (t0, c0), (t1, c1) in zip(SKY, SKY[1:]):
            if t0 <= t <= t1:
                p.set(0, y, mix(rgba(c0), rgba(c1), (t - t0) / (t1 - t0)))
                break
    return p


def waves(x, parts):
    """A height from waves (cycles across the strip, amplitude, phase)."""
    return sum(a * math.sin(2 * math.pi * n * x / STRIP_W + ph) for n, a, ph in parts)


# The clouds, each (its middle across, its middle down, its size), in
# pixels of the strip.
CLOUDS = [(40, 34, 1.0), (170, 22, 0.75), (275, 40, 1.15)]


def clouds():
    """Each cloud is five puffs (circles) cut flat along the bottom, white
    with a pale blue shade underneath."""
    p = Pic(STRIP_W, CLOUDS_H)
    for cx, cy, size in CLOUDS:
        puffs = [(cx + dx * size, cy - dy * size, r * size) for dx, dy, r in
                 ((-16, 0, 8), (-6, 5, 11), (7, 3, 10), (17, 0, 7), (0, -1, 9))]
        bottom = cy + 6 * size
        for y in range(int(bottom) + 1):
            for xx in range(int(cx - 40), int(cx + 40)):
                d = min(math.hypot(xx - px, y - py) - r for px, py, r in puffs)
                if d <= 0:
                    under = y > bottom - 3 or (d > -1.2 and y > cy)
                    p.set(xx % STRIP_W, y, CLOUD_SHADE if under else CLOUD)
    return p


def hills(h, base, parts, fill, shine, shade=None, trees=0, seed=0):
    p = Pic(STRIP_W, h)
    rng = random.Random(seed)
    tops = [int(round(base + waves(x, parts))) for x in range(STRIP_W)]
    for x in range(STRIP_W):
        for y in range(max(tops[x], 0), h):
            c = rgba(fill)
            if shade:
                c = mix(c, rgba(shade), min(1.0, max(0.0, (y - tops[x] - 8) / (h - 20))))
            p.set(x, y, c)
        p.set(x, tops[x], shine)
        p.set(x, tops[x] + 1, shine if tops[x] < tops[(x + 1) % STRIP_W] else fill)
    # Trees: round crowns on short trunks along the ridge, in clumps, lit
    # from the top left.
    for _ in range(trees):
        tx = rng.randrange(STRIP_W)
        for k in range(rng.randint(1, 3)):
            x0 = tx + k * rng.randint(6, 9)
            r = rng.uniform(3.4, 5.2)
            ground = tops[int(x0) % STRIP_W]
            cy = ground - r - 1
            for y in range(int(cy), ground + 2):
                p.set(int(x0) % STRIP_W, y, TREE_SHADE)
            for y in range(int(cy - r - 1), int(cy + r + 2)):
                for xx in range(int(x0 - r - 1), int(x0 + r + 2)):
                    if math.hypot(xx - x0, y - cy) > r:
                        continue
                    if math.hypot(xx - (x0 - r * 0.35), y - (cy - r * 0.35)) < r * 0.55:
                        c = NEAR_HILL_SHINE
                    elif math.hypot(xx - (x0 - r * 0.2), y - (cy - r * 0.2)) < r * 0.85:
                        c = TREE
                    else:
                        c = TREE_SHADE
                    p.set(xx % STRIP_W, y, c)
    return p


def backdrop():
    frames = [
        sky_column(),
        clouds(),
        hills(FAR_H, 26, ((1, 7, 0.3), (3, 5, 1.7), (5, 3, 4.0), (8, 1.5, 2.2)), FAR_HILL, FAR_HILL_SHINE,
              shade=FAR_HILL_SHADE),
        hills(NEAR_H, 30, ((2, 9, 1.1), (3, 6, 0.2), (7, 3, 2.9)), NEAR_HILL, NEAR_HILL_SHINE,
              shade=NEAR_HILL_SHADE, trees=14, seed=3),
    ]
    sheet, rects = pack(frames, STRIP_W + 2)
    png("backdrop", sheet.px)
    header = [
        "# Behind the level: written by tools/art.py with backdrop.png, so",
        "# change that and run it rather than editing this. %d pixels a unit." % PPU,
        "# Frame 0 the sky's colours, top to bottom (scenes/level/sky.toml);",
        "# 1 clouds, 2 far hills, 3 near hills: strips 24 units wide whose ends",
        "# meet, so scenes/level/clouds.toml and hills.*.toml repeat them.",
        "pixels_per_unit = %d" % PPU,
    ]
    write("backdrop.toml", sidecar(rects, header, []))


# The UI's panel ---------------------------------------------------------------


PANEL_SIZE = 16


def panel():
    """16 x 16, nine-sliced at 4 (UiPanel slice): a deep blue fill, a cream
    edge inside a dark line, round corners."""
    n = PANEL_SIZE
    p = Pic(n, n)
    for y in range(n):
        for x in range(n):
            if x in (0, n - 1) and y in (0, n - 1):
                continue  # the corners are round
            if x in (0, n - 1) or y in (0, n - 1) or (x in (1, n - 2) and y in (1, n - 2)):
                p.set(x, y, PANEL_LINE)
            elif x in (1, n - 2) or y in (1, n - 2):
                p.set(x, y, PANEL_EDGE)
            else:
                p.set(x, y, rgba(PANEL, 236))
    return p


if __name__ == "__main__":
    sprites()
    backdrop()
    png("ui/panel", panel().px)
    write("ui/panel.toml", "\n".join([
        "# The HUD's and the pause panel's frame, 16 x 16 pixels: written by",
        "# tools/art.py with panel.png. scenes/level/hud.toml nine-slices it at",
        "# 4 pixels (UiPanel slice), drawn 3 canvas pixels to each of its own.",
        "",
    ]))
    print("wrote assets/sprites.png, assets/backdrop.png, assets/ui/panel.png and their sidecars")
