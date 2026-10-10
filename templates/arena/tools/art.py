"""Arena's art, drawn as pixel grids and by rule, and written as PNGs.

Run it once and the files land in assets/, where the engine picks them up:

    python tools/art.py

It writes the same files every run (the noise comes from fixed seeds), and
needs only Python's standard library. The PNGs it writes are kept in the
project, so you need Python only to change them.

Everything is drawn at 12 pixels a world unit (PPU). The camera
(scenes/main/camera.toml) shows 22.5 units top to bottom, which is 24
screen pixels a unit at 960 x 540: every art pixel is two screen pixels
there, and four at 1920 x 1080, so the picture stays crisp.

What it writes, and who uses it:

- sprites/actors.png and its sidecar: the hero, the enemy, the bullet,
  the braziers, and the bits the effects are made of, one sheet.
  systems/looks.luau picks the frames; the sidecar lists them by number.
  The enemy is grey on purpose: the colour gameplay gives its Sprite
  (orange from sim.waves, paler once hit from sim.hits) tints it.
- sprites/room.png and its sidecar: the floor (drawn by @arena, the size of
  the arena) and the walls round it (drawn by @wall).
- ui/panel.png: the frame the HUD, the pause screen, and the game over
  screen sit in, nine-sliced at 5 pixels.

The sheets are a power of two on each side, as the engine wants them.

To recolour the game, change the colours just below. To change the hero or
the enemy, edit their pictures (one character a pixel, looked up in the
palette beside them).
"""

import math
import os
import random
import struct
import zlib

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets")

# The sizes ------------------------------------------------------------------

PPU = 12  # art pixels a world unit
ARENA_W, ARENA_H = 32, 18  # the arena in units: @arena's Sprite size in arena.toml
STONE = 2  # a floor stone's side, in units
SIDE, SOUTH = 9, 9  # the walls' thickness at the sides and bottom, in pixels
CAP, FACE = 6, 12  # the top wall: its cap, and the face below it, in pixels

# The colours ----------------------------------------------------------------


def hexc(s, a=255):
    s = s.lstrip("#")
    return (int(s[0:2], 16), int(s[2:4], 16), int(s[4:6], 16), a)


CLEAR = (0, 0, 0, 0)

FLOOR = hexc("2E3C45")  # the stones
FLOOR_SEAM = hexc("131B20")  # the gaps between them
FLOOR_PAINT = hexc("4F7C80")  # the worn rings painted on the floor
IRON = hexc("3A3F47")  # the drain grate in the middle
VIGNETTE = 0.25  # how much darker the floor is at the walls than in the middle

WALL_TOP = hexc("617986")  # the walls' tops
WALL_LIGHT = hexc("86A0AD")  # their lit edge
WALL_FACE = hexc("3C4E5A")  # the top wall's face
WALL_DARK = hexc("25323B")  # mortar, and the face's foot
INK = hexc("0B1013")  # outlines round the walls
PENNANTS = [hexc("C4872A"), hexc("A8402C")]  # the pennants on the top wall, in turn

HERO = {
    ".": CLEAR,
    "o": hexc("1B140C"),  # outline (added round the picture)
    "k": hexc("1B140C"),  # a dark line inside it
    "Y": hexc("F2C14E"),  # helmet and shoulders
    "y": hexc("C4872A"),  # their shade
    "L": hexc("FFE9A3"),  # their shine
    "V": hexc("8BEAFF"),  # visor
    "v": hexc("2C86A8"),  # visor, dark
    "S": hexc("46505E"),  # suit
    "s": hexc("2C333D"),  # suit, shade
    "G": hexc("C3CCD4"),  # gun
    "g": hexc("67727E"),  # gun, dark
    "B": hexc("2E333C"),  # boots
    "b": hexc("1F2329"),  # the far boot, from the side
}

# The enemy is grey: its Sprite's colour tints it (multiplies it).
ENEMY = {
    ".": CLEAR,
    "o": hexc("222222"),  # outline (added round the picture)
    "D": hexc("787878"),  # shade
    "B": hexc("B4B4B4"),  # body
    "H": hexc("E6E6E6"),  # light
    "e": hexc("101010"),  # brows, pupils, mouth
    "W": hexc("FFFFFF"),  # eyes, fangs
}

PANEL_FILL = hexc("0E161B", 228)  # the UI frame's glass
PANEL_EDGE = hexc("4F6672")  # its edge
PANEL_CORNER = HERO["Y"]  # its corners, the hero's yellow

# PNG out --------------------------------------------------------------------


def _paeth(a, b, c):
    p = a + b - c
    pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
    if pa <= pb and pa <= pc:
        return a
    return b if pb <= pc else c


def png(path, rows):
    """Writes rows (lists of (r, g, b, a), all one length) as a PNG under
    assets/. Each row takes the filter that packs it smallest."""
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


def text(path, body):
    full = os.path.join(ROOT, path)
    os.makedirs(os.path.dirname(full), exist_ok=True)
    with open(full, "w", newline="\n") as f:
        f.write(body)


def clamp8(v):
    return max(0, min(255, int(round(v))))


def shade(c, d):
    """A colour made lighter (d > 0) or darker (d < 0) by d levels."""
    return (clamp8(c[0] + d), clamp8(c[1] + d), clamp8(c[2] + d), c[3])


def scale(c, k):
    return (clamp8(c[0] * k), clamp8(c[1] * k), clamp8(c[2] * k), c[3])


def mix(a, b, t):
    return tuple(clamp8(a[i] + (b[i] - a[i]) * t) for i in range(3)) + (a[3],)


def grid(picture, palette):
    """A picture as strings, one character a pixel, looked up in palette."""
    for line in picture:
        assert len(line) == len(picture[0]), line
    return [[palette[ch] for ch in line] for line in picture]


def outlined(picture, palette):
    """A 10 x 10 picture in a 12 x 12 frame, with a one-pixel line of the
    palette's "o" round its shape (every clear pixel beside a drawn one),
    so it reads against any floor."""
    inner = grid(picture, palette)
    n = len(inner) + 2
    out = [[CLEAR] * n for _ in range(n)]
    for y, row in enumerate(inner):
        for x, px in enumerate(row):
            out[y + 1][x + 1] = px
    line = palette["o"]
    for y in range(n):
        for x in range(n):
            if out[y][x][3]:
                continue
            for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                i, j = x + dx, y + dy
                if 0 <= i < n and 0 <= j < n and out[j][i][3] and out[j][i] != line:
                    out[y][x] = line
                    break
    return out


# The hero, 12 x 12 ----------------------------------------------------------
#
# A gunner in a round yellow helmet, seen from above and a little in front,
# as three views: from the side (facing right; the game mirrors it to face
# left), from the front, and from the back. Each view is a body (8 rows)
# on legs (2 rows); a step swaps the legs. The outline is added round it.
# "k" is a dark line inside the picture (the visor's rim, the hand).

HERO_SIDE = [
    "..LLYYY...",
    ".LYYYYYY..",
    ".YYYYYkVV.",
    ".YYYYYkVv.",
    ".yYYYYYYy.",
    "..yyyyyy..",
    ".YSSSkGGGG",
    ".ySSSsgg..",
]
HERO_FRONT = [
    "..LLYYYY..",
    ".LYYYYYYy.",
    ".YkVVVVky.",
    ".YkVVVvky.",
    ".yYYYYYYy.",
    "..yyyyyy..",
    ".YSSSSSSYg",
    ".ySSSSSSyG",
]
HERO_BACK = [
    "..LLYYYY.G",
    ".LYYYYYYyG",
    ".YYYYYYYyG",
    ".YYYYYYYyG",
    ".yYYYYYYyg",
    "..yyyyyy.g",
    ".YsggggsYg",
    ".ysggggsy.",
]
# Legs: standing, and the two steps. From the side the far leg is darker.
SIDE_LEGS = (["..ss.SS...", "..bb.BB..."], [".ss...SS..", ".bb....BB."], [".SS...ss..", ".BB....bb."])
FRONT_LEGS = (["..SS..SS..", "..BB..BB.."], ["..BB..SS..", "......BB.."], ["..SS..BB..", "..BB......"])


def hero(body, legs):
    return outlined(body + legs, HERO)


def silhouette(frame):
    """Hit: the whole hero white with a red edge."""
    white, red = hexc("FFFFFF"), hexc("FF4A4A")
    return [[CLEAR if px[3] == 0 else (red if px == HERO["o"] else white) for px in row] for row in frame]


def fallen(frame):
    """Out of health: the side view lying on its back, its colours dulled."""
    n = len(frame)
    turned = [[frame[n - 1 - x][y] for x in range(n)] for y in range(n)]
    out = []
    for row in turned:
        line = []
        for px in row:
            if px[3] == 0:
                line.append(CLEAR)
            else:
                v = (px[0] * 3 + px[1] * 6 + px[2]) / 10
                line.append(scale(mix(px, (v, v, v, 255), 0.55), 0.7))
        out.append(line)
    return out


# The enemy, 12 x 12 ---------------------------------------------------------
#
# A horned imp: angry brows, eyes that look the way it faces (right; the
# game mirrors it to face the hero), fangs, and two feet that waddle. Two
# frames of walking, and the same two cracked once it has been hit. The
# outline is added round it.

ENEMY_BODY = [
    "H........H",
    "BH......HB",
    ".BBBBBBBB.",
    "BeBBBBBBeB",
    "BBeBBBBeBB",
    "BWeBBBBWeB",
    "BWWBBBBWWB",
    "DBeWeeWeBD",
    ".DDeeeeDD.",
]
ENEMY_FEET = ([".DD....DD."], ["..DD..DD.."])
# Where the cracks run once it is hit (x, y in the picture above).
CRACKS = [(5, 1), (4, 2), (5, 3), (3, 2), (6, 4)]


def enemy(feet, cracked):
    picture = [list(r) for r in ENEMY_BODY + feet]
    if cracked:
        for x, y in CRACKS:
            picture[y][x] = "e"
    return outlined(["".join(r) for r in picture], ENEMY)


# The small things -----------------------------------------------------------


def glow(size, power, peak=255):
    """A round glow, white, its alpha falling from the middle (drawn added,
    so its Sprite's colour tints it)."""
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


def bolt():
    """A bullet, 7 x 7: a hot middle, then a halo fading out."""
    rows = []
    for y in range(7):
        row = []
        for x in range(7):
            d = math.hypot(x - 3, y - 3)
            a = 1.0 if d < 1.2 else max(0.0, 1.0 - (d - 1.2) / 2.6) ** 1.5 * 0.8
            row.append((255, 255, 255, clamp8(255 * a)))
        rows.append(row)
    return rows

# A spark, 3 x 3: a plus.
SPARK = [[(255, 255, 255, a) for a in r] for r in [[50, 200, 50], [200, 255, 200], [50, 200, 50]]]


def puff():
    """A puff of dust, 7 x 7: a soft lumpy ball, white (the effect
    colours it)."""
    rng = random.Random(3)
    rows = []
    for y in range(7):
        row = []
        for x in range(7):
            d = math.hypot(x - 3, y - 3) / 3.6
            a = max(0.0, 1.0 - d * d) * (0.75 + 0.25 * rng.random())
            row.append((255, 255, 255, clamp8(255 * a)))
        rows.append(row)
    return rows


def shadow(w, h, alpha):
    """A soft dark oval, w x h, for under a hero or an enemy."""
    rows = []
    for y in range(h):
        row = []
        for x in range(w):
            d = ((x - (w - 1) / 2) / (w / 2)) ** 2 + ((y - (h - 1) / 2) / (h / 2)) ** 2
            row.append((0, 0, 0, clamp8(alpha * min(1.0, 1.6 * (1.0 - d)))) if d < 1 else CLEAR)
        rows.append(row)
    return rows


# A brazier on each corner pillar (braziers.toml): an iron bowl of coals;
# its flames are particles (fx/flame), its glow a Light.
BRAZIER = grid(
    [
        "............",
        "............",
        "............",
        "..oooooooo..",
        ".ohcCYYCcho.",
        ".oicCYCCcio.",
        "..oiiiiiio..",
        "..ohIIIIho..",
        "...oIIIIo...",
        "....oiio....",
        "...oIooIo...",
        "...oo..oo...",
    ],
    {
        ".": CLEAR,
        "o": hexc("0E0F12"),
        "I": hexc("3E434C"),
        "i": hexc("272B31"),
        "h": hexc("6E7682"),
        "c": hexc("C2452A"),
        "C": hexc("FF9A3C"),
        "Y": hexc("FFE7A0"),
    },
)


# The actors' sheet ----------------------------------------------------------

# The frames, in the order systems/looks.luau counts them.
FRAMES = [
    ("hero side, standing", lambda: hero(HERO_SIDE, SIDE_LEGS[0])),  # 0
    ("hero side, step", lambda: hero(HERO_SIDE, SIDE_LEGS[1])),  # 1
    ("hero side, other step", lambda: hero(HERO_SIDE, SIDE_LEGS[2])),  # 2
    ("hero front, standing", lambda: hero(HERO_FRONT, FRONT_LEGS[0])),  # 3
    ("hero front, step", lambda: hero(HERO_FRONT, FRONT_LEGS[1])),  # 4
    ("hero front, other step", lambda: hero(HERO_FRONT, FRONT_LEGS[2])),  # 5
    ("hero back, standing", lambda: hero(HERO_BACK, FRONT_LEGS[0])),  # 6
    ("hero back, step", lambda: hero(HERO_BACK, FRONT_LEGS[1])),  # 7
    ("hero back, other step", lambda: hero(HERO_BACK, FRONT_LEGS[2])),  # 8
    ("hero side, hit", lambda: silhouette(hero(HERO_SIDE, SIDE_LEGS[0]))),  # 9
    ("hero front, hit", lambda: silhouette(hero(HERO_FRONT, FRONT_LEGS[0]))),  # 10
    ("hero back, hit", lambda: silhouette(hero(HERO_BACK, FRONT_LEGS[0]))),  # 11
    ("hero fallen", lambda: fallen(hero(HERO_SIDE, SIDE_LEGS[0]))),  # 12
    ("enemy, step", lambda: enemy(ENEMY_FEET[0], False)),  # 13
    ("enemy, other step", lambda: enemy(ENEMY_FEET[1], False)),  # 14
    ("enemy hit, step", lambda: enemy(ENEMY_FEET[0], True)),  # 15
    ("enemy hit, other step", lambda: enemy(ENEMY_FEET[1], True)),  # 16
    ("bullet", bolt),  # 17
    ("spark", lambda: SPARK),  # 18
    ("puff", puff),  # 19
    ("shadow", lambda: shadow(10, 4, 120)),  # 20
    ("brazier", lambda: BRAZIER),  # 21
]


def actors():
    """Packs the frames into rows of a sheet; returns its rows and the
    frames' rects."""
    W, H = 128, 64
    sheet = [[CLEAR] * W for _ in range(H)]
    rects = []
    x = y = row_h = 0
    for _, draw in FRAMES:
        f = draw()
        fw, fh = len(f[0]), len(f)
        if x + fw > W:
            x, y, row_h = 0, y + row_h + 1, 0
        for j in range(fh):
            for i in range(fw):
                sheet[y + j][x + i] = f[j][i]
        rects.append((x, y, fw, fh))
        x += fw + 1
        row_h = max(row_h, fh)
    return sheet, rects


def actors_sidecar(rects):
    lines = [
        "# The actors' sheet (sprites/actors.png): written by tools/art.py with",
        "# the picture, so change that and run it rather than editing this.",
        "# 12 pixels a unit. systems/looks.luau picks the frames by number:",
    ]
    for n, (name, _) in enumerate(FRAMES):
        lines.append("#   %2d %s" % (n, name))
    lines += ["pixels_per_unit = %d" % PPU, ""]
    for r in rects:
        lines += ["[[frames]]", "rect = [%d, %d, %d, %d]" % r, ""]
    return "\n".join(lines)


# The room: the floor and the walls ------------------------------------------


def courses(rng, W, H, height):
    """Lays the floor's stones in courses (rows) height pixels tall, each
    course its own lengths, so the joints do not line up. Returns a list
    of stones, (x, y, w, h) in pixels."""
    out = []
    for y in range(0, H, height):
        x = -rng.randrange(0, height)
        while x < W:
            w = rng.choice((1, 1.25, 1.5, 1.5, 2)) * height
            w = int(w)
            out.append((x, y, w, height))
            x += w
    return out


def floor():
    """The floor, ARENA_W x ARENA_H units: worn stones with bevelled edges,
    two rings painted round the middle, the top wall's shadow, and darker
    toward the walls."""
    rng = random.Random(11)
    W, H = ARENA_W * PPU, ARENA_H * PPU
    img = [[FLOOR] * W for _ in range(H)]
    for sx, sy, w, h in courses(rng, W, H, STONE * PPU):
        tone = rng.choice((-7, -4, -2, 0, 0, 2, 3, 6))  # each stone its own shade
        for ly in range(h):
            for lx in range(w):
                x, y = sx + lx, sy + ly
                if not (0 <= x < W and 0 <= y < H):
                    continue
                # A stone is a little lighter at its top left, where the
                # light comes from, than at its bottom right.
                lean = round(4 - 8 * (lx / w + ly / h) / 2)
                if lx == 0 or ly == 0:
                    c = FLOOR_SEAM
                elif lx == 1 or ly == 1:
                    c = shade(FLOOR, tone + 10)
                elif lx == w - 1 or ly == h - 1:
                    c = shade(FLOOR, tone - 10)
                else:
                    c = shade(FLOOR, tone + lean + rng.choice((-2, -1, 0, 0, 0, 1, 2)))
                img[y][x] = c
    # A few chips and cracks.
    for _ in range(26):
        x, y = rng.randrange(4, W - 4), rng.randrange(4, H - 4)
        dx, dy = rng.choice((-1, 1)), rng.choice((-1, 0, 1))
        for s in range(rng.randrange(3, 9)):
            px, py = x + s * dx, y + (s * dy) // 2
            if 0 <= px < W and 0 <= py < H and img[py][px] != FLOOR_SEAM:
                img[py][px] = shade(img[py][px], -12)
    # Scorch marks from old fights: dark, ragged, softest at their edge.
    for _ in range(7):
        x0, y0, r0 = rng.randrange(20, W - 20), rng.randrange(20, H - 20), rng.uniform(7, 15)
        for y in range(int(y0 - r0) - 1, int(y0 + r0) + 2):
            for x in range(int(x0 - r0) - 1, int(x0 + r0) + 2):
                if 0 <= x < W and 0 <= y < H:
                    d = math.hypot(x - x0, y - y0) / r0 + rng.uniform(-0.18, 0.18)
                    if d < 1:
                        img[y][x] = scale(img[y][x], 0.62 + 0.38 * d)
    # Grit: small stones, lit above and shadowed below.
    for _ in range(70):
        x, y = rng.randrange(2, W - 2), rng.randrange(2, H - 3)
        img[y][x] = shade(img[y][x], 16)
        img[y + 1][x] = shade(img[y + 1][x], -10)
    # The rings round the middle, worn: some of their pixels are gone.
    cx, cy = W / 2 - 0.5, H / 2 - 0.5
    for y in range(H):
        for x in range(W):
            r = math.hypot(x - cx, y - cy) / PPU
            on = 3.85 < r < 4.15 or 6.0 < r < 6.5
            if on and img[y][x] != FLOOR_SEAM and rng.random() < 0.7:
                img[y][x] = mix(img[y][x], FLOOR_PAINT, 0.35)
    # A drain grate in the middle: an iron ring lit on top, bars over the dark.
    for y in range(H):
        for x in range(W):
            r = math.hypot(x - cx, y - cy)
            if r <= 20.5:
                if r > 19.5:
                    c = INK
                elif r > 16.5:
                    c = shade(IRON, 14 if y < cy else -8)
                elif r > 15.5:
                    c = INK
                elif int(y - cy + 100) % 4 < 2:
                    c = shade(IRON, 6 if int(y - cy + 100) % 4 == 0 else -6)
                else:
                    c = shade(INK, 3)
                img[y][x] = c
    # The top wall's shadow, and the dark toward every wall.
    for y in range(H):
        for x in range(W):
            ex = abs(x - cx) / (W / 2)
            ey = abs(y - cy) / (H / 2)
            e = max(0.0, max(ex, ey) - 0.55) / 0.45
            k = 1.0 - VIGNETTE * e * e
            if y < 10:
                k *= 0.62 + 0.038 * y
            img[y][x] = scale(img[y][x], k)
    return img


def walls():
    """The walls round the floor: their tops all round, and the top wall's
    face of bricks below its cap (we look down on the room a little from
    the south, so the top wall shows its face). The middle is clear: the
    floor is drawn there."""
    rng = random.Random(5)
    fw, fh = ARENA_W * PPU, ARENA_H * PPU
    W, H = fw + 2 * SIDE, fh + CAP + FACE + SOUTH
    img = [[CLEAR] * W for _ in range(H)]
    top = CAP + FACE  # the floor's first row

    def put(x, y, c):
        if 0 <= x < W and 0 <= y < H:
            img[y][x] = c

    # The caps: blocks of stone along each wall.
    def cap_block(x0, y0, x1, y1):
        t = rng.choice((-5, -2, 0, 2, 4))
        for y in range(y0, y1):
            for x in range(x0, x1):
                if x == x0 or y == y0:
                    c = shade(WALL_LIGHT, t)
                elif x == x1 - 1 or y == y1 - 1:
                    c = shade(WALL_DARK, t)
                else:
                    c = shade(WALL_TOP, t + rng.choice((-2, 0, 0, 1, 2)))
                put(x, y, c)

    block = 2 * PPU
    for x0 in range(0, W, block):  # the top wall's cap
        cap_block(x0, 0, min(x0 + block, W), CAP)
    off = block // 2
    for x0 in range(-off, W, block):  # the bottom wall's, staggered
        cap_block(max(x0, 0), H - SOUTH, min(x0 + block, W), H)
    for y0 in range(CAP, H - SOUTH, block):  # the side walls'
        cap_block(0, y0, SIDE, min(y0 + block, H - SOUTH))
        cap_block(W - SIDE, y0, W, min(y0 + block, H - SOUTH))
    # The top wall's face: two courses of bricks, darker toward the floor.
    course = FACE // 2
    for k in range(2):
        y0 = CAP + k * course
        stagger = (k % 2) * 8
        for x0 in range(SIDE - stagger, W - SIDE, 16):
            t = rng.choice((-4, -2, 0, 1, 3))
            for y in range(y0, y0 + course):
                for x in range(max(x0, SIDE), min(x0 + 16, W - SIDE)):
                    if y == y0 or x == x0:
                        c = WALL_DARK
                    else:
                        c = shade(WALL_FACE, t + rng.choice((-1, 0, 0, 1)) - 4 * k)
                        if y == y0 + 1:
                            c = shade(c, 8)
                    put(x, y, c)
    for x in range(SIDE, W - SIDE):  # the face's foot, where it meets the floor
        put(x, top - 1, INK)
        put(x, top - 2, shade(WALL_DARK, -6))
    # Pennants hung along the face, in turn of colour: a rod, the cloth lit
    # at its left, a pale band, and a swallowtail.
    for i in range(5):
        col = PENNANTS[i % len(PENNANTS)]
        x0 = (W * (i + 1)) // 6 - 3
        for x in range(x0 - 1, x0 + 7):
            put(x, CAP, INK)
        for d in range(FACE - 2):
            y = CAP + 1 + d
            for x in range(x0, x0 + 6):
                if d >= FACE - 4 and x0 + 2 <= x <= x0 + 3:
                    continue  # the tail's cut
                c = col
                if x == x0:
                    c = shade(col, 22)
                elif x == x0 + 5:
                    c = shade(col, -30)
                if d in (3, 4):
                    c = shade(c, 45)
                put(x, y, c)
    # Pillars at the corners: a bigger block, lit.
    for (px, py) in ((0, 0), (W - 15, 0), (0, H - 15), (W - 15, H - 15)):
        for y in range(py, py + 15):
            for x in range(px, px + 15):
                if x in (px, px + 14) or y in (py, py + 14):
                    c = INK
                elif x == px + 1 or y == py + 1:
                    c = WALL_LIGHT
                elif x == px + 13 or y == py + 13:
                    c = WALL_DARK
                else:
                    c = shade(WALL_TOP, 6)
                put(x, y, c)
    # The inner rim of the caps (the drop to the floor), and an outline round it all.
    for x in range(SIDE, W - SIDE):
        put(x, H - SOUTH, INK)
    for y in range(top, H - SOUTH):
        put(SIDE - 1, y, INK)
        put(W - SIDE, y, INK)
    for x in range(W):
        put(x, 0, INK)
        put(x, H - 1, INK)
    for y in range(H):
        put(0, y, INK)
        put(W - 1, y, INK)
    return img


def pow2(n):
    """The power of two at or above n: the engine pads a sheet to one."""
    p = 1
    while p < n:
        p *= 2
    return p


def room():
    """One sheet: the walls (frame 1) above the floor (frame 0)."""
    f = floor()
    w = walls()
    W, H = pow2(max(len(w[0]), len(f[0]))), pow2(len(w) + 1 + len(f))
    sheet = [[CLEAR] * W for _ in range(H)]
    for y, row in enumerate(w):
        sheet[y][: len(row)] = row
    for y, row in enumerate(f):
        sheet[len(w) + 1 + y][: len(row)] = row
    floor_rect = (0, len(w) + 1, len(f[0]), len(f))
    wall_rect = (0, 0, len(w[0]), len(w))
    return sheet, floor_rect, wall_rect


def room_sidecar(floor_rect, wall_rect):
    fw, fh = floor_rect[2], floor_rect[3]
    ww, wh = wall_rect[2], wall_rect[3]
    return "\n".join([
        "# The room (sprites/room.png): written by tools/art.py with the picture.",
        "# Frame 0 is the floor, %d x %d units: @arena draws it (arena.toml)." % (fw // PPU, fh // PPU),
        "# Frame 1 is the walls round it, %g x %g units, clear in the middle:" % (ww / PPU, wh / PPU),
        "# @wall draws it (wall.toml). 12 pixels a unit.",
        "pixels_per_unit = %d" % PPU,
        "",
        "[[frames]]",
        "rect = [%d, %d, %d, %d]" % floor_rect,
        "",
        "[[frames]]",
        "rect = [%d, %d, %d, %d]" % wall_rect,
        "",
    ])


# The UI frame ---------------------------------------------------------------


def panel():
    """16 x 16, nine-sliced at 5 (the UI files say slice = 5): dark glass,
    a steel edge, and the hero's yellow at the corners."""
    n = 16
    rows = []
    for y in range(n):
        row = []
        for x in range(n):
            edge = x in (0, n - 1) or y in (0, n - 1)
            inner = x in (1, n - 2) or y in (1, n - 2)
            corner = (x < 4 or x > n - 5) and (y < 4 or y > n - 5)
            if edge and corner:
                row.append(PANEL_CORNER)
            elif edge:
                row.append(PANEL_EDGE)
            elif inner:
                row.append(INK[:3] + (PANEL_FILL[3],))
            elif y == 2:
                row.append(shade(PANEL_FILL, 12)[:3] + (PANEL_FILL[3],))
            else:
                row.append(PANEL_FILL)
        rows.append(row)
    # The corners cut off, so the frame is not a hard box.
    for x, y in ((0, 0), (n - 1, 0), (0, n - 1), (n - 1, n - 1)):
        rows[y][x] = CLEAR
    return rows


if __name__ == "__main__":
    sheet, rects = actors()
    png("sprites/actors.png", sheet)
    text("sprites/actors.toml", actors_sidecar(rects))
    sheet, floor_rect, wall_rect = room()
    png("sprites/room.png", sheet)
    text("sprites/room.toml", room_sidecar(floor_rect, wall_rect))
    png("ui/panel.png", panel())
    print("wrote sprites/actors.png (+ .toml), sprites/room.png (+ .toml), ui/panel.png")
