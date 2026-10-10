"""Swarm's art, drawn as pixel grids and by rule, and written as PNGs.

The camera shows 60 units top to bottom, so on a 960 x 540 screen a unit
is 9 pixels: an enemy is 8 pixels across, the hero 11. The actors are drawn
pixel by pixel at that size (as Hollow's are), so the picture is crisp at
540 and at twice that; the floor and the glows are drawn by rule. Run it
once and the files land in assets/, where the importer picks them up:

    python tools/art.py

What it writes, and who reads it:

- sprites/swarm.png and its sidecar: every actor, bullet, and spark, one
  atlas. The enemies and dashers are grey, so the colour sim.waves and
  sim.dash_look give their Sprite tints them (the dasher's phases stay
  yellow, red, and dim); the hero is in his own colours.
- sprites/floor.png, floor.normal.png, and the sidecar: the deck, eight
  tiles of 12 units (3 x 3 plates), with a normal map so the hero's light
  catches the plates' edges. systems/looks.luau lays them under the camera.
- ui/frame.png and ui/menu.png: the HUD's panel and the pause menu's,
  nine-sliced.

Standard library only; the same files every run (the noise is seeded).
"""

import math
import os
import random
import struct
import zlib

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets")
CLEAR = (0, 0, 0, 0)


# PNG out ------------------------------------------------------------------


def _paeth(a, b, c):
    p = a + b - c
    pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
    if pa <= pb and pa <= pc:
        return a
    return b if pb <= pc else c


def png(path, rows):
    """rows: lists of (r, g, b, a) tuples, all the same length. Each row
    takes the filter that leaves the smallest sum (the usual heuristic), so
    the floor packs small."""
    h, w = len(rows), len(rows[0])
    prev = bytes(w * 4)
    out = bytearray()
    for row in rows:
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
    full = os.path.join(ROOT, path)
    os.makedirs(os.path.dirname(full), exist_ok=True)
    with open(full, "wb") as f:
        f.write(data)
    return full


def text(path, body):
    full = os.path.join(ROOT, path)
    os.makedirs(os.path.dirname(full), exist_ok=True)
    with open(full, "w", newline="\n") as f:
        f.write(body)


def hexc(s, a=255):
    return (int(s[0:2], 16), int(s[2:4], 16), int(s[4:6], 16), a)


def grey(v, a=255):
    return (v, v, v, a)


def grid(picture, palette):
    """A picture as strings, one character a pixel, looked up in palette."""
    return [[palette[ch] for ch in line] for line in picture]


def clamp8(v):
    return max(0, min(255, int(round(v))))


# The actors, 9 pixels a unit ----------------------------------------------

# The hero, 11 x 11: a turret drone, a cyan ring round a white core, its
# four prongs the way the fan of bullets leaves it. Navy outline so it
# stands out of its own yellow bullets.
HERO = [
    "....NLN....",
    "..NNCLCNN..",
    ".NCCKKKCCN.",
    ".NCKLLLKCN.",
    "NCKLWWWLKCN",
    "LLKLWWWLKLL",
    "NCKLWWWLKCN",
    ".NCKLLLKCN.",
    ".NCCKKKCCN.",
    "..NNCLCNN..",
    "....NLN....",
]
HERO_PAL = {
    ".": CLEAR,
    "N": hexc("0B1628"),
    "K": hexc("12708F"),
    "C": hexc("2FC6EA"),
    "L": hexc("A6F2FF"),
    "W": hexc("FFFFFF"),
}
# The ring's cells clockwise from the top: two sparks run round it, so the
# idle hero looks alive.
RING = [
    (6, 1), (7, 2), (8, 2), (8, 3), (9, 4), (9, 6), (8, 7), (8, 8), (7, 8), (6, 9),
    (4, 9), (3, 8), (2, 8), (2, 7), (1, 6), (1, 4), (2, 3), (2, 2), (3, 2), (4, 1),
]
HERO_IDLE_FRAMES = 10


def hero_idle(k):
    g = [list(r) for r in HERO]
    for cell in (RING[(2 * k) % 20], RING[(2 * k + 10) % 20]):
        x, y = cell
        g[y][x] = "W"
    return grid(["".join(r) for r in g], HERO_PAL)


def hero_hurt(white):
    """Hit: the whole hero white, then red, flickering through the iframes."""
    out = []
    for line in HERO:
        row = []
        for ch in line:
            if ch == ".":
                row.append(CLEAR)
            elif white:
                row.append(hexc("FFFFFF") if ch != "N" else hexc("FF4D6D"))
            else:
                row.append({"N": hexc("3A0A18"), "K": hexc("A01838"), "C": hexc("FF4D6D"),
                            "L": hexc("FFB0BE"), "W": hexc("FFFFFF")}[ch])
        out.append(row)
    return out


# Dead: the core out, the ring cracked, the prongs gone.
HERO_DEAD = grid(
    [
        "...........",
        "..NNN.CNN..",
        ".NCCKKK.CN.",
        ".NCKkkkKCN.",
        "NCKkdddkKCN",
        ".CKkdddk.C.",
        "NCKkdddkKCN",
        ".N.KkkkKCN.",
        ".NCCK.KCCN.",
        "..NNCC.NN..",
        "...........",
    ],
    {
        ".": CLEAR,
        "N": hexc("0B1628"),
        "C": hexc("8494AC"),
        "K": hexc("4E5C74"),
        "k": hexc("2E3848"),
        "d": hexc("171D29"),
    },
)

# The enemies are grey: their Sprite's colour tints them (sim.waves: the
# swarm orange, the dashers violet). O outline, D shade, B body, H light,
# E eyes.
TINT = {".": CLEAR, "O": grey(46), "D": grey(168), "B": grey(232), "H": grey(255), "E": grey(18)}

# A mite, 8 x 8: three of them, a round body with eyes, so a crowd of
# hundreds is a crowd of creatures and not a blur.
MITES = [
    [
        "..OOOO..",
        ".OHHBBO.",
        "OHBBBBBO",
        "OBEBBEBO",
        "OBBBBBDO",
        "OBBBBDDO",
        ".ODDDDO.",
        "..OOOO..",
    ],
    [
        ".O....O.",
        "..OOOO..",
        ".OHBBBO.",
        "OHBEBEBO",
        "OBBBBBDO",
        "OBBBBDDO",
        ".ODDDDO.",
        "..OOOO..",
    ],
    [
        "..OOOO..",
        "OOHBBBOO",
        ".OHEBEO.",
        "OBBBBBDO",
        "OBBBBBDO",
        ".OBBBDO.",
        "OODDDDOO",
        "..O..O..",
    ],
]

# The dasher, 9 x 9: a diamond, so its outline is not a mite's; C its core.
# Walking; winding up (flared, the core lit); spent (hollow).
DART_PAL = dict(TINT, C=grey(255))
DARTS = [
    [
        "....O....",
        "...OHO...",
        "..OHBBO..",
        ".OHBBBDO.",
        "OHBBCBBDO",
        ".OBBBBDO.",
        "..OBBDO..",
        "...ODO...",
        "....O....",
    ],
    [
        "O...O...O",
        ".O.OHO.O.",
        "..OHCBO..",
        ".OHCCCDO.",
        "OHCCCCCDO",
        ".OBCCCDO.",
        "..OBCDO..",
        ".O.ODO.O.",
        "O...O...O",
    ],
    [
        "....O....",
        "...ODO...",
        "..ODDDO..",
        ".ODDODDO.",
        "ODDOEODDO",
        ".ODDODDO.",
        "..ODDDO..",
        "...ODO...",
        "....O....",
    ],
]


def glow(size, power, peak=255):
    """A round glow, white, its alpha falling from the middle: a bullet, a
    spark, the hero's halo (drawn added, tinted by the sprite's colour)."""
    c = (size - 1) / 2
    rows = []
    for y in range(size):
        row = []
        for x in range(size):
            d = math.hypot(x - c, y - c) / (size / 2)
            a = max(0.0, 1.0 - d) ** power
            row.append((255, 255, 255, clamp8(peak * a)))
        rows.append(row)
    return rows


# A bullet, 5 x 5: a hot middle with a halo.
BOLT = [
    [(255, 255, 255, a) for a in r]
    for r in [
        [0, 40, 90, 40, 0],
        [40, 170, 235, 170, 40],
        [90, 235, 255, 235, 90],
        [40, 170, 235, 170, 40],
        [0, 40, 90, 40, 0],
    ]
]

# A spark, 3 x 3: a plus.
SPARK = [
    [(255, 255, 255, a) for a in r] for r in [[60, 200, 60], [200, 255, 200], [60, 200, 60]]
]


def atlas():
    """Packs the actors into one sheet; returns its rows and the frames'
    rects in index order (the order systems/looks.luau counts in)."""
    frames = []
    frames += [hero_idle(k) for k in range(HERO_IDLE_FRAMES)]  # 0-9
    frames += [hero_hurt(True), hero_hurt(False)]  # 10, 11
    frames += [HERO_DEAD]  # 12
    frames += [grid(m, TINT) for m in MITES]  # 13-15
    frames += [grid(d, DART_PAL) for d in DARTS]  # 16-18
    frames += [BOLT, SPARK]  # 19, 20
    frames += [glow(36, 2.2, 150)]  # 21: the halo
    W, H = 256, 64
    sheet = [[CLEAR] * W for _ in range(H)]
    rects = []
    x = y = 0
    row_h = 0
    for f in frames:
        fw, fh = len(f[0]), len(f)
        if x + fw > W:
            x, y = 0, y + row_h + 1
            row_h = 0
        for j in range(fh):
            for i in range(fw):
                sheet[y + j][x + i] = f[j][i]
        rects.append((x, y, fw, fh))
        x += fw + 1
        row_h = max(row_h, fh)
    return sheet, rects


def atlas_sidecar(rects):
    lines = [
        "# Every actor, bullet, and spark of Swarm, one atlas: written by",
        "# tools/art.py with the sheet, so run that rather than editing this.",
        "# Frames: 0-9 the hero idle, 10-11 hurt, 12 dead; 13-15 mites;",
        "# 16-18 the dasher walking, winding up, spent; 19 bullet; 20 spark;",
        "# 21 the hero's halo. 9 pixels a unit, as the camera shows them.",
        "pixels_per_unit = 9",
        "",
    ]
    for r in rects:
        lines += ["[[frames]]", "rect = [%d, %d, %d, %d]" % r, ""]
    lines += [
        "[clips.idle]",
        "from = 0",
        "to = %d" % (HERO_IDLE_FRAMES - 1),
        "fps = 12",
        "",
        "[clips.hurt]",
        "frames = [10, 11]",
        "fps = 20",
        "",
        "[clips.dead]",
        "frames = [12]",
        "loop = false",
        "",
    ]
    return "\n".join(lines)


# The floor ----------------------------------------------------------------

TILE = 108  # 12 units
PLATE = 36  # 4 units
BASE = (31, 40, 56)
SEAM = (11, 15, 22)


class Tile:
    """A floor tile: colour and height per pixel (height makes the normal
    map, so the hero's light catches the plates' edges and rivets)."""

    def __init__(self, rng):
        self.rng = rng
        self.c = [[list(BASE) for _ in range(TILE)] for _ in range(TILE)]
        self.h = [[1.0] * TILE for _ in range(TILE)]

    def add(self, x, y, d, h=None):
        if 0 <= x < TILE and 0 <= y < TILE:
            px = self.c[y][x]
            for i in range(3):
                px[i] += d[i] if isinstance(d, tuple) else d
            if h is not None:
                self.h[y][x] = h

    def put(self, x, y, col, h=None):
        if 0 <= x < TILE and 0 <= y < TILE:
            self.c[y][x] = list(col)
            if h is not None:
                self.h[y][x] = h


def mottle(rng, cells=4, amp=5.0):
    """Low noise across a tile: a coarse grid of values, smoothly between."""
    g = [[rng.uniform(-amp, amp) for _ in range(cells + 1)] for _ in range(cells + 1)]
    # The edges match, so tiles meet without a step.
    for i in range(cells + 1):
        g[i][cells] = g[i][0]
        g[cells][i] = g[0][i]
    step = TILE / cells
    out = [[0.0] * TILE for _ in range(TILE)]
    for y in range(TILE):
        gy = y / step
        j = min(int(gy), cells - 1)
        fy = gy - j
        fy = fy * fy * (3 - 2 * fy)
        for x in range(TILE):
            gx = x / step
            i = min(int(gx), cells - 1)
            fx = gx - i
            fx = fx * fx * (3 - 2 * fx)
            a = g[j][i] + (g[j][i + 1] - g[j][i]) * fx
            b = g[j + 1][i] + (g[j + 1][i + 1] - g[j + 1][i]) * fx
            out[y][x] = a + (b - a) * fy
    return out


def plate(t, ox, oy, pw, ph, kind):
    """A plate pw x ph pixels at (ox, oy) of the tile: its seam, its bevel,
    its rivets, and what is on it."""
    rng = t.rng
    # The seam round it, the bevel inside the seam (lit above and left), the face.
    for j in range(ph):
        for i in range(pw):
            x, y = ox + i, oy + j
            if i in (0, pw - 1) or j in (0, ph - 1):
                t.put(x, y, SEAM, 0.0)
            elif i == 1 or j == 1:
                t.add(x, y, 9, 0.55)
            elif i == pw - 2 or j == ph - 2:
                t.add(x, y, -9, 0.55)
            else:
                t.add(x, y, rng.choice((-2, -1, 0, 0, 1, 2)))
    # A rivet in each corner (and along a long edge).
    xs = [4, pw - 6] + ([pw // 2 - 1] if pw > 40 else [])
    ys = [4, ph - 6] + ([ph // 2 - 1] if ph > 40 else [])
    for cx in xs:
        for cy in ys:
            if cx not in (4, pw - 6) and cy not in (4, ph - 6):
                continue
            t.add(ox + cx, oy + cy, 22, 1.5)
            t.add(ox + cx + 1, oy + cy, 8, 1.4)
            t.add(ox + cx, oy + cy + 1, 8, 1.4)
            t.add(ox + cx + 1, oy + cy + 1, -6, 1.3)
    mx, my = ox + pw / 2 - 0.5, oy + ph / 2 - 0.5
    if kind == "tread":
        # Diamond plate: raised dashes, every other one turned.
        for gj in range((ph - 12) // 5):
            for gi in range((pw - 12) // 5):
                cx, cy = 7 + gi * 5 + (2 if gj % 2 else 0), 7 + gj * 5
                if cx > pw - 7:
                    continue
                d = 1 if (gi + gj) % 2 else -1
                for s in (-1, 0, 1):
                    t.add(ox + cx + s, oy + cy + s * d, 7, 1.25)
                t.add(ox + cx + 2, oy + cy + d + 1, -3)
    elif kind == "vent":
        x0, x1 = int(mx) - 10, int(mx) + 10
        y0, y1 = int(my) - 10, int(my) + 10
        for j in range(y0, y1):
            for i in range(x0, x1):
                edge = i in (x0, x1 - 1) or j in (y0, y1 - 1)
                t.add(i, j, -5 if edge else -2, 0.8 if edge else 0.9)
        for k in range(5):
            y = y0 + 3 + k * 3
            for i in range(x0 + 2, x1 - 2):
                t.put(i, y, (8, 11, 17), 0.2)
                t.add(i, y + 1, 7, 0.9)
    elif kind == "hatch":
        # A round hatch, its lid bolted; a big one on a big plate.
        big = min(pw, ph) > 40
        r0 = 21.0 if big else 10.4
        for j in range(ph):
            for i in range(pw):
                r = math.hypot(ox + i - mx, oy + j - my)
                if r0 < r <= r0 + 1.2:
                    t.put(ox + i, oy + j, (14, 19, 28), 0.4)
                elif r <= r0:
                    t.add(ox + i, oy + j, -3, 0.95)
                    if r0 - 0.8 < r and oy + j < my:
                        t.add(ox + i, oy + j, 6, 0.8)
                    if big and 13.5 < r <= 14.5:
                        t.put(ox + i, oy + j, (18, 24, 34), 0.7)
        bolts = 8 if big else 4
        for a in range(bolts):
            ang = math.pi / bolts + a * 2 * math.pi / bolts
            rr = r0 - 3.0
            t.add(int(round(mx + rr * math.cos(ang))), int(round(my + rr * math.sin(ang))), 18, 1.4)
        for i in range(-3 if not big else -6, 3 if not big else 6):
            t.add(int(mx) + i, int(my), 10, 1.3)
            t.add(int(mx) + i, int(my) + 1, -4, 1.2)
    elif kind == "hazard":
        for j in range(4, 10):
            for i in range(4, pw - 4):
                if ((i + j) // 3) % 2 == 0:
                    t.put(ox + i, oy + j, (78, 63, 30), 1.05)
                else:
                    t.put(ox + i, oy + j, (24, 28, 36), 1.05)
        for i in range(4, pw - 4):
            t.add(ox + i, oy + 3, -6)
            t.add(ox + i, oy + 10, -6)
    elif kind == "strip":
        # A guide light in a groove: the hero's cyan, dim, under the deck.
        y = int(my) - 1
        for i in range(5, pw - 5):
            t.put(ox + i, y, (9, 13, 19), 0.5)
            t.put(ox + i, y + 1, (30, 104, 124) if i % 4 else (18, 52, 64), 0.6)
            t.put(ox + i, y + 2, (9, 13, 19), 0.5)
            t.add(ox + i, y + 3, 5)
    elif kind == "worn":
        for j in range(2, ph - 2):
            for i in range(2, pw - 2):
                t.add(ox + i, oy + j, -5)
        for _ in range(pw * ph // 300):
            x, y = rng.randint(6, pw - 8), rng.randint(6, ph - 8)
            dx = rng.choice((-1, 1))
            for s in range(rng.randint(4, 9)):
                t.add(ox + x + s * dx, oy + y + s // 2, 5)


KINDS = ["plain"] * 6 + ["tread"] * 2 + ["vent", "hatch", "strip", "worn", "worn", "hazard"]
BIG_KINDS = ["plain", "plain", "tread", "hatch", "worn"]

# How a tile is cut into plates, in plates of 4 units: a big square, a long
# one, or nine small; one layout a tile, so the deck is not a sheet of
# squares.
LAYOUTS = [
    [(0, 0, 2, 2), (2, 0, 1, 1), (2, 1, 1, 1), (0, 2, 1, 1), (1, 2, 1, 1), (2, 2, 1, 1)],
    [(1, 1, 2, 2), (0, 0, 1, 1), (1, 0, 1, 1), (2, 0, 1, 1), (0, 1, 1, 1), (0, 2, 1, 1)],
    [(0, 0, 3, 1), (0, 1, 1, 1), (1, 1, 1, 1), (2, 1, 1, 1), (0, 2, 3, 1)],
    [(0, 0, 1, 3), (1, 0, 1, 1), (2, 0, 1, 1), (1, 1, 2, 2)],
    [(x, y, 1, 1) for y in range(3) for x in range(3)],
    [(0, 0, 1, 1), (1, 0, 2, 1), (0, 1, 1, 2), (1, 1, 1, 1), (2, 1, 1, 1), (1, 2, 2, 1)],
    [(x, y, 1, 1) for y in range(3) for x in range(3)],
    [(0, 0, 2, 1), (2, 0, 1, 2), (0, 1, 1, 1), (1, 1, 1, 1), (0, 2, 2, 1), (2, 2, 1, 1)],
]


# Tiles with a guide light on their second plate, so there are always some.
GUIDES = (0, 3, 5)


def floor_tile(n):
    rng = random.Random(1000 + n)
    t = Tile(rng)
    m = mottle(rng)
    for y in range(TILE):
        for x in range(TILE):
            for i in range(3):
                t.c[y][x][i] += m[y][x]
    for k, (px, py, w, h) in enumerate(LAYOUTS[n % len(LAYOUTS)]):
        kinds = BIG_KINDS if (w > 1 and h > 1) else KINDS
        kind = rng.choice(kinds)
        if kind == "hazard" and w < h:
            kind = "plain"
        if k == 1 and n in GUIDES:
            kind = "strip"
        plate(t, px * PLATE, py * PLATE, w * PLATE, h * PLATE, kind)
    return t


def normal(t, x, y, k=1.6):
    def h(i, j):
        return t.h[min(max(j, 0), TILE - 1)][min(max(i, 0), TILE - 1)]

    nx = (h(x - 1, y) - h(x + 1, y)) * 0.5 * k
    # Rows go down the picture and the world's y goes up.
    ny = (h(x, y + 1) - h(x, y - 1)) * 0.5 * k
    n = math.sqrt(nx * nx + ny * ny + 1.0)
    return (clamp8((nx / n * 0.5 + 0.5) * 255), clamp8((ny / n * 0.5 + 0.5) * 255),
            clamp8((1.0 / n * 0.5 + 0.5) * 255), 255)


FLOOR_COLS, FLOOR_ROWS = 4, 2
FLOOR_W, FLOOR_H = 512, 256  # the tiles, on a sheet a power of two


def floor():
    tiles = [floor_tile(i) for i in range(FLOOR_COLS * FLOOR_ROWS)]
    color = [[CLEAR] * FLOOR_W for _ in range(FLOOR_H)]
    norm = [[(128, 128, 255, 255)] * FLOOR_W for _ in range(FLOOR_H)]
    rects = []
    for n, t in enumerate(tiles):
        ox, oy = (n % FLOOR_COLS) * TILE, (n // FLOOR_COLS) * TILE
        rects.append((ox, oy, TILE, TILE))
        for y in range(TILE):
            for x in range(TILE):
                c = t.c[y][x]
                color[oy + y][ox + x] = (clamp8(c[0]), clamp8(c[1]), clamp8(c[2]), 255)
                norm[oy + y][ox + x] = normal(t, x, y)
    return color, norm, rects


def floor_sidecar(rects):
    lines = [
        "# The deck under the swarm: eight tiles of 12 x 12 units (108 pixels, 9 a",
        "# unit), each cut into plates, with floor.normal.png beside it so the",
        "# hero's light catches the plates' edges. Written by tools/art.py with the",
        "# sheet. systems/looks.luau lays 14 x 7 of them under the camera, each",
        "# world cell its own tile.",
        "pixels_per_unit = 9",
        "",
    ]
    for r in rects:
        lines += ["[[frames]]", "rect = [%d, %d, %d, %d]" % r, ""]
    return "\n".join(lines)


# The UI -------------------------------------------------------------------


def frame_image(alpha):
    """16 x 16, nine-sliced at 5 (the HUD's and the menu's panels): a dark
    glass fill, a steel edge, and cyan corner brackets. The HUD's glass shows
    the deck through it; the menu's is near opaque, so nothing of the HUD
    shows through its words."""
    fill = (9, 16, 30, alpha)
    edge = (38, 66, 102, 255)
    corner = (92, 225, 255, 255)
    rows = []
    for y in range(16):
        row = []
        for x in range(16):
            outer = x in (0, 15) or y in (0, 15)
            near = (x < 5 or x > 10) and (y < 5 or y > 10)
            if outer and near:
                row.append(corner)
            elif outer:
                row.append(edge)
            elif y == 1:
                row.append((24, 38, 60, max(alpha, 222)))
            else:
                row.append(fill)
        rows.append(row)
    return rows


if __name__ == "__main__":
    sheet, rects = atlas()
    png("sprites/swarm.png", sheet)
    text("sprites/swarm.toml", atlas_sidecar(rects))
    color, norm, rects = floor()
    png("sprites/floor.png", color)
    png("sprites/floor.normal.png", norm)
    text("sprites/floor.toml", floor_sidecar(rects))
    png("ui/frame.png", frame_image(214))
    png("ui/menu.png", frame_image(250))
    print("wrote sprites/swarm.png (+ .toml), sprites/floor.png, sprites/floor.normal.png (+ .toml), ui/frame.png, ui/menu.png")
