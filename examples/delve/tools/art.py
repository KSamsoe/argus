"""Delve's art, drawn as pixel grids and small procedures, written as PNGs.

Everything Delve draws comes from here: the dungeon's tiles (stone floors,
brick walls, the rock between rooms, the shadows walls cast, and what lies
on the floor), the hero, the three monsters, the potion and the stairs, and
the frames the HUD and the menu sit in. Run it once and the files land in
assets/, where the importer picks them up; standard library only, and the
same files every time (the noise comes from fixed seeds).

    python tools/art.py

A cell of the floor is 12 x 12 pixels, drawn at one world unit; the camera
(scenes/delve/turn.toml) shows 24 screen pixels a unit at 960 x 540, so
every art pixel is two screen pixels there, four at 1920 x 1080.

The tileset's frames are numbered as systems/floor_art.luau paints them
(tile n is frame n - 1); the table is at the end of this file.
"""

import os
import random
import struct
import zlib

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets")
T = 12  # pixels a cell

# ----- PNG -------------------------------------------------------------------


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


class Img:
    """A w x h picture of (r, g, b, a) pixels."""

    def __init__(self, w, h, fill=CLEAR):
        self.w, self.h = w, h
        self.px = [[fill] * w for _ in range(h)]

    def get(self, x, y):
        if 0 <= x < self.w and 0 <= y < self.h:
            return self.px[y][x]
        return CLEAR

    def set(self, x, y, c):
        if 0 <= x < self.w and 0 <= y < self.h:
            self.px[y][x] = c

    def rect(self, x0, y0, x1, y1, c):
        for y in range(y0, y1 + 1):
            for x in range(x0, x1 + 1):
                self.set(x, y, c)

    def blit(self, other, ox, oy):
        for y in range(other.h):
            for x in range(other.w):
                c = other.px[y][x]
                if c[3]:
                    self.set(ox + x, oy + y, c)

    def flipped(self):
        out = Img(self.w, self.h)
        out.px = [list(reversed(r)) for r in self.px]
        return out

    def save(self, path):
        png(path, self.w, self.h, self.px)


def grid(picture, palette):
    """A picture as strings, one character a pixel, looked up in palette."""
    img = Img(len(picture[0]), len(picture))
    for y, line in enumerate(picture):
        assert len(line) == img.w, (line, len(line))
        for x, ch in enumerate(line):
            img.px[y][x] = palette[ch]
    return img


def outlined(img, color):
    """The picture with a one-pixel line of color round its shape (the
    four neighbours of every drawn pixel that is clear), so a sprite reads
    against any floor."""
    out = Img(img.w, img.h)
    out.px = [row[:] for row in img.px]
    for y in range(img.h):
        for x in range(img.w):
            if img.px[y][x][3]:
                continue
            if any(img.get(x + dx, y + dy)[3] for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1))):
                out.px[y][x] = color
    return out


def normals(heights, strength=2.0):
    """A normal map (y up, as argus-light reads it) from a height grid of
    numbers 0 to 1, None where the picture is clear. Edges wrap within
    each 12 x 12 frame, so a tile's bevels meet its neighbour's."""
    h = len(heights)
    w = len(heights[0])
    out = Img(w, h)

    def at(fx, fy, x, y):
        v = heights[fy + (y - fy) % T][fx + (x - fx) % T]
        return 0.0 if v is None else v

    for y in range(h):
        for x in range(w):
            if heights[y][x] is None:
                continue
            fx, fy = (x // T) * T, (y // T) * T
            dx = (at(fx, fy, x + 1, y) - at(fx, fy, x - 1, y)) * strength
            # Rows go down; the normal's y is up.
            dy = (at(fx, fy, x, y - 1) - at(fx, fy, x, y + 1)) * strength
            nx, ny, nz = -dx, -dy, 1.0
            k = (nx * nx + ny * ny + nz * nz) ** 0.5
            nx, ny, nz = nx / k, ny / k, nz / k
            out.px[y][x] = (
                int(round((nx * 0.5 + 0.5) * 255)),
                int(round((ny * 0.5 + 0.5) * 255)),
                int(round((nz * 0.5 + 0.5) * 255)),
                255,
            )
    return out


def mix(a, b, t):
    return tuple(int(round(a[i] + (b[i] - a[i]) * t)) for i in range(3)) + (a[3],)


# ----- palette ---------------------------------------------------------------

INK = hexc("100C14")  # outline

# Floor stone: a cool grey with a little violet, so the torch warms it and
# the ambient light cools it.
MORTAR = hexc("221F29")
STONE = [hexc("3A3542"), hexc("3F3A48"), hexc("443F4E")]
STONE_HI = hexc("524C5F")
STONE_LO = hexc("2F2B37")

# The rock between rooms, seen from above, and its rim where it meets a room.
ROCK = hexc("221F29")
ROCK_DARK = hexc("19171F")
ROCK_SPECK = hexc("2A2733")
RIM = hexc("625A73")
RIM2 = hexc("3D3748")

# A wall's face: bricks under a lit cap, a dark foot.
CAP = hexc("6F6680")
BRICK = [hexc("514860"), hexc("5A5069"), hexc("4A4258")]
BRICK_HI = hexc("6A5F7B")
BRICK_LO = hexc("3D3549")
BMORTAR = hexc("29232F")
FOOT = hexc("15121A")

MOSS = [hexc("2F4A2E"), hexc("3E6338"), hexc("557F45")]
WATER = hexc("223047")
WATER_HI = hexc("4D6F94")
BONE = hexc("D9D0B4")
BONE_LO = hexc("9C927A")
BLOOD = hexc("6E1A24")
BLOOD_HI = hexc("962632")
WEB = hexc("B8B4C0", 150)


# ----- the dungeon tileset ---------------------------------------------------

rng = random.Random(7)


class Tile:
    """A 12 x 12 tile and its heights (for the normal map)."""

    def __init__(self, fill=CLEAR, height=None):
        self.img = Img(T, T, fill)
        self.ht = [[height] * T for _ in range(T)]

    def set(self, x, y, c, ht=None):
        if 0 <= x < T and 0 <= y < T:
            self.img.px[y][x] = c
            if ht is not None:
                self.ht[y][x] = ht


def stones(rects, cracked=False, worn=False):
    """A floor tile of flagstones: mortar along its top row and left column
    (the neighbours close the other two sides), each stone a shade of its
    own with a lit top-left edge, a dark bottom-right one, chipped corners,
    and a few specks."""
    t = Tile(MORTAR, 0.0)
    for (x0, y0, x1, y1) in rects:
        base = rng.choice(STONE)
        for y in range(y0, y1 + 1):
            for x in range(x0, x1 + 1):
                c, ht = base, 1.0
                if y == y0 or x == x0:
                    c, ht = mix(base, STONE_HI, 0.55), 0.8
                if y == y1 or x == x1:
                    c, ht = mix(base, STONE_LO, 0.7), 0.7
                t.set(x, y, c, ht)
        # Chipped corners.
        for (cx, cy) in ((x0, y0), (x1, y0), (x0, y1), (x1, y1)):
            if rng.random() < 0.6:
                t.set(cx, cy, MORTAR, 0.0)
        # Specks.
        area = (x1 - x0 + 1) * (y1 - y0 + 1)
        for _ in range(max(1, area // 14)):
            x = rng.randint(x0 + 1, max(x0 + 1, x1 - 1))
            y = rng.randint(y0 + 1, max(y0 + 1, y1 - 1))
            dark = rng.random() < 0.5
            t.set(x, y, mix(base, STONE_LO if dark else STONE_HI, 0.45), 0.9 if dark else 1.0)
        if worn:
            for _ in range(area // 6):
                x = rng.randint(x0 + 1, max(x0 + 1, x1 - 1))
                y = rng.randint(y0 + 1, max(y0 + 1, y1 - 1))
                t.set(x, y, mix(base, STONE_LO, 0.3), 0.85)
    if cracked:
        x, y = rng.randint(3, 6), 2
        while y < 11:
            t.set(x, y, MORTAR, 0.2)
            y += 1
            x += rng.choice((-1, 0, 0, 1))
            x = min(10, max(2, x))
    return t


def floors():
    return [
        stones([(1, 1, 11, 11)]),
        stones([(1, 1, 5, 11), (7, 1, 11, 11)]),
        stones([(1, 1, 11, 5), (1, 7, 11, 11)]),
        stones([(1, 1, 6, 5), (8, 1, 11, 5), (1, 7, 11, 11)]),
        stones([(1, 1, 5, 5), (7, 1, 11, 5), (1, 7, 5, 11), (7, 7, 11, 11)]),
        stones([(1, 1, 11, 11)], cracked=True),
        stones([(1, 1, 7, 6), (9, 1, 11, 6), (1, 8, 3, 11), (5, 8, 11, 11)], worn=True),
        stones([(1, 1, 11, 7), (1, 9, 6, 11), (8, 9, 11, 11)]),
    ]


def rock(dark=False):
    """The rock between rooms, from above: dark, with specks and a seam."""
    base = ROCK_DARK if dark else ROCK
    t = Tile(base, 0.55)
    for _ in range(9 if not dark else 5):
        x, y = rng.randrange(T), rng.randrange(T)
        t.set(x, y, ROCK_SPECK if not dark else ROCK, 0.65)
    for _ in range(3):
        x, y = rng.randrange(T), rng.randrange(T)
        t.set(x, y, mix(base, INK, 0.5), 0.45)
    return t


def top(mask):
    """A wall seen from above beside a room: rock, with a lit rim on each
    side that meets open floor (mask: 1 the cell above, 2 right, 4 left)."""
    t = rock()
    for i in range(T):
        if mask & 1:
            t.set(i, 0, RIM, 0.9)
            t.set(i, 1, RIM2, 0.7)
        if mask & 2:
            t.set(T - 1, i, RIM, 0.9)
            t.set(T - 2, i, RIM2, 0.7)
        if mask & 4:
            t.set(0, i, RIM, 0.9)
            t.set(1, i, RIM2, 0.7)
    return t


def face(sides, extra=None):
    """A wall's face, the cell above open floor: a lit cap, three courses of
    bricks, a dark foot; sides 1 or 2 when its left or right is open too
    (the corner of the wall shows as a lit edge)."""
    t = Tile(BMORTAR, 0.3)
    for x in range(T):
        t.set(x, 0, CAP, 1.0)
        t.set(x, T - 1, FOOT, 0.0)
    courses = [(1, 3, 0), (5, 7, 3), (9, 10, 0)]
    for (y0, y1, off) in courses:
        x = -off
        while x < T:
            x0, x1 = x, x + 4
            base = rng.choice(BRICK)
            for y in range(y0, y1 + 1):
                for xx in range(x0, x1 + 1):
                    c = base
                    if y == y0:
                        c = mix(base, BRICK_HI, 0.6)
                    elif y == y1 and y1 - y0 > 1:
                        c = mix(base, BRICK_LO, 0.6)
                    t.set(xx % T, y, c, 0.85 if y == y0 else 0.75)
            x += 6
    if sides & 1:
        for y in range(T - 1):
            t.set(0, y, CAP, 1.0)
    if sides & 2:
        for y in range(T - 1):
            t.set(T - 1, y, CAP, 1.0)
    if extra == "moss":
        for (x, y, k) in ((1, 9, 0), (2, 9, 1), (2, 10, 0), (3, 10, 1), (8, 10, 0), (9, 9, 2), (9, 10, 1), (10, 10, 0), (3, 8, 2)):
            t.set(x, y, MOSS[k], 0.8)
    elif extra == "crack":
        for (x, y) in ((6, 1), (6, 2), (7, 3), (7, 4), (6, 5), (6, 6), (5, 7)):
            t.set(x, y, BMORTAR, 0.2)
    return t


def shade(kind):
    """The shadow a wall casts on the floor beside it: along the top when
    the wall is above (N), along the left when it is to the left (W), both
    (NW), or just the corner (C, a wall only diagonally up-left)."""
    t = Tile()
    a_top = (0, 0, 0, 120), (0, 0, 0, 70), (0, 0, 0, 30)
    a_left = (0, 0, 0, 80), (0, 0, 0, 35)
    for y in range(T):
        for x in range(T):
            a = 0
            if kind in ("N", "NW") and y < 3:
                a = max(a, a_top[y][3])
            if kind in ("W", "NW") and x < 2:
                a = max(a, a_left[x][3])
            if kind == "C" and x < 2 and y < 2:
                a = max(a, (90, 40)[max(x, y)])
            if a:
                t.set(x, y, (0, 0, 0, a))
    return t


def decal(kind):
    """What lies on the floor: cracks, pebbles, moss, a puddle, bones, a web
    in a corner, and what a fight leaves."""
    t = Tile()
    if kind == "crack":
        for (x, y) in ((3, 3), (4, 4), (4, 5), (5, 6), (6, 6), (7, 7), (7, 8), (8, 9), (5, 7)):
            t.set(x, y, MORTAR)
    elif kind == "pebbles":
        for (x, y) in ((3, 7), (8, 4), (6, 9)):
            t.set(x, y, STONE_HI)
            t.set(x, y + 1, MORTAR)
        t.set(4, 7, STONE[2])
    elif kind == "moss":
        spots = ((2, 8, 1), (3, 8, 2), (3, 9, 1), (4, 9, 0), (2, 9, 0), (4, 8, 1), (5, 9, 1), (3, 10, 0), (8, 3, 1), (9, 3, 0), (8, 4, 0))
        for (x, y, k) in spots:
            t.set(x, y, MOSS[k])
    elif kind == "puddle":
        for y, (x0, x1) in ((6, (4, 7)), (7, (3, 8)), (8, (4, 8))):
            for x in range(x0, x1 + 1):
                t.set(x, y, (WATER[0], WATER[1], WATER[2], 210))
        t.set(5, 6, WATER_HI)
        t.set(6, 7, WATER_HI)
    elif kind == "bones":
        for (x, y) in ((3, 8), (4, 7), (5, 6), (6, 5)):
            t.set(x, y, BONE)
        t.set(2, 8, BONE_LO)
        t.set(6, 4, BONE_LO)
        t.set(7, 5, BONE_LO)
        t.set(8, 8, BONE)
        t.set(9, 8, BONE_LO)
    elif kind == "skull":
        pic = ["..bbb..", ".bBBBb.", ".BkBkB.", ".bBBBb.", "..b.b.."]
        pal = {".": None, "b": BONE_LO, "B": BONE, "k": INK}
        for y, line in enumerate(pic):
            for x, ch in enumerate(line):
                if pal[ch]:
                    t.set(3 + x, 4 + y, pal[ch])
    elif kind == "web":
        # Strands out of the corner and two threads across them.
        for y in range(8):
            for x in range(8):
                r = (x * x + y * y) ** 0.5
                if r > 7.2:
                    continue
                ray = x == 0 or y == 0 or x == y or abs(x - 2 * y) == 0 or abs(y - 2 * x) == 0
                arc = abs(r - 3.2) < 0.5 or abs(r - 6.0) < 0.5
                if ray or arc:
                    t.set(x, y, WEB if ray else (WEB[0], WEB[1], WEB[2], 100))
    elif kind in ("rat", "bat", "goblin", "blood"):
        big = kind == "goblin"
        spots = [(5, 6), (6, 6), (6, 7), (5, 7), (4, 7), (7, 6), (6, 5)]
        if big:
            spots += [(3, 6), (4, 6), (7, 7), (8, 7), (5, 8), (6, 8), (4, 5), (5, 5), (8, 6), (3, 8)]
        if kind != "bat":
            for (x, y) in spots:
                t.set(x, y, BLOOD)
            t.set(5, 6, BLOOD_HI)
            t.set(2, 9, BLOOD)
            t.set(9, 4, BLOOD)
        if kind == "rat":
            t.set(7, 8, hexc("6E5644"))
            t.set(8, 8, hexc("6E5644"))
            t.set(9, 9, hexc("C08888"))
        elif kind == "bat":
            dark = hexc("2C1E3A")
            for (x, y) in spots:
                t.set(x, y, dark)
            for (x, y) in ((3, 4), (4, 5), (8, 4), (7, 5), (2, 4), (9, 4)):
                t.set(x, y, hexc("4A3466"))
        elif kind == "goblin":
            # A dropped knife.
            for (x, y) in ((8, 9), (9, 9), (10, 9)):
                t.set(x, y, hexc("B8C0CC"))
            t.set(7, 9, hexc("5A3A22"))
    return t


def dungeon():
    tiles = []
    tiles += floors()  # frames 0-7
    tiles += [top(m) for m in range(8)]  # 8-15
    tiles += [face(s) for s in range(4)]  # 16-19
    tiles += [rock(dark=True), rock(dark=True)]  # 20-21
    tiles += [face(0, "moss"), face(0, "crack")]  # 22-23
    tiles += [shade(k) for k in ("N", "W", "NW", "C")]  # 24-27
    tiles += [decal(k) for k in ("crack", "pebbles", "moss", "puddle")]  # 28-31
    tiles += [decal(k) for k in ("bones", "web", "rat", "bat", "goblin", "blood", "skull")]  # 32-38
    # The normal map: the stones' and bricks' heights; what lies on the
    # floor is flat, so it leaves the stone under it as it is.
    hts = []
    for i, t in enumerate(tiles):
        if i >= 24:
            hts.append([[0.5 if t.img.px[y][x][3] else None for x in range(T)] for y in range(T)])
        else:
            hts.append(t.ht)
    return [t.img for t in tiles], hts


# ----- sheets and their sidecars ---------------------------------------------


def pot(n):
    p = 1
    while p < n:
        p *= 2
    return p


def write_sheet(name, frames, cols, doc, heights=None, extra=""):
    """Frames of one size, cols a row, on a power-of-two image (so the
    importer need not pad it), and the sidecar that cuts them out by rect:
    assets/<name>.png and assets/<name>.toml (and <name>.normal.png from
    heights, a grid of numbers per frame)."""
    fw, fh = frames[0].w, frames[0].h
    rows = (len(frames) + cols - 1) // cols
    w, h = pot(fw * min(cols, len(frames))), pot(fh * rows)
    img = Img(w, h)
    rects = []
    for i, f in enumerate(frames):
        x, y = (i % cols) * fw, (i // cols) * fh
        img.blit(f, x, y)
        rects.append((x, y))
    img.save(name + ".png")
    if heights is not None:
        hts = [[None] * w for _ in range(h)]
        for (x, y), ht in zip(rects, heights):
            for yy in range(fh):
                for xx in range(fw):
                    hts[y + yy][x + xx] = ht[yy][xx]
        normals(hts).save(name + ".normal.png")
    lines = ["# " + l if l else "#" for l in doc.strip().splitlines()]
    lines.append("# Written by tools/art.py with the picture; run it again rather than editing this.")
    if extra:
        lines += ["", extra.strip()]
    for (x, y) in rects:
        lines += ["", "[[frames]]", "rect = [%d, %d, %d, %d]" % (x, y, fw, fh)]
    with open(os.path.join(ROOT, name + ".toml"), "w", newline="\n") as f:
        f.write("\n".join(lines) + "\n")


# ----- sprites -----------------------------------------------------------------

# The hero: a helmeted adventurer in a red cloak with a torch held out in
# front. Frames: idle (two, the flame and the cloak shift), dead.
HERO_PAL = {
    ".": CLEAR,
    "H": hexc("AEB6C4"),  # helmet
    "h": hexc("6E7686"),  # helmet's shade
    "S": hexc("F0C8A0"),  # skin
    "e": hexc("2A1A14"),  # eye
    "T": hexc("2F6E8C"),  # tunic
    "t": hexc("214E66"),  # tunic's shade
    "C": hexc("B8382E"),  # cloak
    "c": hexc("82241E"),  # cloak's shade
    "b": hexc("7A5230"),  # belt and torch handle
    "L": hexc("3A3348"),  # legs
    "B": hexc("5A3A26"),  # boots
    "Y": hexc("FFE680"),  # flame core
    "y": hexc("FF9A30"),  # flame
}
HERO = [
    [
        "..........y.",
        "....HHH..yYy",
        "...HHHHH.yYy",
        "..chSSeS..b.",
        "..chSSSS..b.",
        ".cC.SSS..Sb.",
        ".cCTTTTTTS..",
        ".cCTTTTt....",
        ".cCtbbbt....",
        "..cTTtTT....",
        "...LL.LL....",
        "...BB.BB....",
    ],
    [
        "...........y",
        "....HHH...yY",
        "...HHHHH.yYy",
        "..chSSeS..b.",
        "..chSSSS..b.",
        "..C.SSS..Sb.",
        ".cCTTTTTTS..",
        ".cCTTTTt....",
        "ccCtbbbt....",
        ".ccTTtTT....",
        "...LL.LL....",
        "...BB.BB....",
    ],
    [
        "............",
        "............",
        "............",
        "............",
        "............",
        "............",
        "............",
        "....b.......",
        "..HH.bcCCc..",
        ".HHSScCTTCC.",
        ".hHSStTTtLLB",
        "..yYy..cCcLB",
    ],
]

RAT_PAL = {
    ".": CLEAR,
    "F": hexc("8C6E58"),  # fur
    "f": hexc("664E3E"),  # fur's shade
    "W": hexc("B09480"),  # belly
    "E": hexc("D89A9A"),  # ears, nose, tail
    "R": hexc("FF3A30"),  # eye
}
RAT = [
    [
        "............",
        "............",
        "............",
        "............",
        "......E.E...",
        ".....FFFFF..",
        "....FFFFRFF.",
        "E..fFFFFFFFE",
        ".E.fFFFFFWW.",
        "..EffFFWWW..",
        "....F..F....",
        "............",
    ],
    [
        "............",
        "............",
        "............",
        "............",
        ".......E.E..",
        "......FFFFF.",
        "....FFFFFRFF",
        "...fFFFFFFFE",
        "EE.fFFFFFWW.",
        "..EffFFWWW..",
        "...F....F...",
        "............",
    ],
]

BAT_PAL = {
    ".": CLEAR,
    "W": hexc("5E3E86"),  # wing
    "w": hexc("3E2860"),  # wing's shade
    "B": hexc("7A5A9E"),  # body
    "E": hexc("9C7CC0"),  # ears
    "Y": hexc("FFE040"),  # eyes
    "k": hexc("F4F0F8"),  # fangs
}
BAT = [
    [
        "............",
        ".W........W.",
        ".WW......WW.",
        ".WWW.EE.WWW.",
        ".WwWBBBBWwW.",
        "..wWBYBYWw..",
        "..w.BBBB.w..",
        ".....kk.....",
        "............",
        "............",
        "............",
        "............",
    ],
    [
        "............",
        "............",
        "............",
        ".....EE.....",
        "....BBBB....",
        ".WWWBYBYWWW.",
        "WWwwBBBBwwWW",
        "W.w..kk..w.W",
        "..w......w..",
        "............",
        "............",
        "............",
    ],
]

GOB_PAL = {
    ".": CLEAR,
    "G": hexc("6FA43E"),  # skin
    "g": hexc("4C7A2A"),  # skin's shade
    "R": hexc("FF4030"),  # eyes
    "k": hexc("F2EEDC"),  # teeth
    "V": hexc("7A4E2C"),  # leather
    "v": hexc("57361E"),  # leather's shade
    "D": hexc("C8D0DC"),  # blade
    "h": hexc("4A3020"),  # hilt
    "L": hexc("3E3428"),  # legs
}
GOBLIN = [
    [
        "............",
        ".g.......g..",
        ".gGGGGGGGg..",
        "..GRGGGRG...",
        "..GGGGGGG...",
        "...GkkkG..D.",
        "..vVVVVVvGD.",
        ".GvVVVVVvGh.",
        ".G.VVvVV....",
        "...VV.VV....",
        "...LL.LL....",
        "..gg..gg....",
    ],
    [
        "............",
        "............",
        ".g.......g..",
        ".gGGGGGGGg..",
        "..GRGGGRG.D.",
        "..GGGGGGG.D.",
        "...GkkkG.Gh.",
        "..vVVVVVvG..",
        ".GvVVVVVv...",
        ".G.VVvVV....",
        "...LL.LL....",
        "..gg..gg....",
    ],
]

POT_PAL = {
    ".": CLEAR,
    "c": hexc("A07848"),  # cork
    "g": hexc("C8DCE8"),  # glass
    "R": hexc("E8344E"),  # the draught
    "r": hexc("A01E36"),  # its shade
    "W": hexc("FFF4F4"),  # a glint
}
POTION = [
    "............",
    "............",
    ".....cc.....",
    ".....gg.....",
    "....gRRg....",
    "...gRRRRg...",
    "..gRWRRRRg..",
    "..gRRRRRrg..",
    "..gRRRRrrg..",
    "...grrrrg...",
    "....gggg....",
    "............",
]

# The way down, from above: a stone-rimmed opening in the floor, its
# steps going down away from the viewer, each tread darker than the one
# before and the walls beside them closing in, into the dark.
STAIR_PAL = {
    ".": CLEAR,
    "P": hexc("7A7290"),  # the rim
    "p": hexc("554E66"),  # its corners
    "V": hexc("07070C"),  # the dark below
    "S": hexc("24222F"),  # the stairwell's walls
    "0": hexc("A8A2BA"),  # the treads, near to far
    "1": hexc("8A84A0"),
    "2": hexc("6A6684"),
    "3": hexc("4E4B66"),
    "4": hexc("36344C"),
    "r": hexc("14131E"),  # a step's riser
}
STAIRS = [
    "pPPPPPPPPPPp",
    "PVVVVVVVVVVP",
    "PSSS4444SSSP",
    "PSSrrrrrrSSP",
    "PSS333333SSP",
    "PSrrrrrrrrSP",
    "PS22222222SP",
    "PSrrrrrrrrSP",
    "P1111111111P",
    "PrrrrrrrrrrP",
    "P0000000000P",
    "pPPPPPPPPPPp",
]


def sprite(frames, pal, outline=True):
    imgs = [grid(f, pal) for f in frames]
    if outline:
        imgs = [outlined(i, INK) for i in imgs]
    return imgs


def flash(img, toward, k, line):
    """The frame a blow lands on: every colour of the shape pulled toward
    toward by k, and its outline in line (a hit flash, one turn long)."""
    out = Img(img.w, img.h)
    for y in range(img.h):
        for x in range(img.w):
            c = img.px[y][x]
            if not c[3]:
                continue
            out.px[y][x] = line if c == INK else mix(c, toward, k)
    return out


# ----- UI frames -------------------------------------------------------------


def ring(img, i, c, cut=0):
    """The ring i pixels in from the edge, in c (its corners cut by cut)."""
    n = img.w
    for k in range(i + cut, n - i - cut):
        for (x, y) in ((k, i), (k, n - 1 - i), (i, k), (n - 1 - i, k)):
            img.set(x, y, c)


def panel():
    """The HUD's plate and the menu's pages: dark stone under a bronze edge
    with a rivet in each corner. Nine-sliced at 4 pixels."""
    img = Img(12, 12, hexc("15121A", 236))
    ring(img, 0, hexc("4E3A22"), cut=1)
    ring(img, 1, hexc("B48C52"))
    ring(img, 2, hexc("2A2018"))
    for (x, y) in ((1, 1), (10, 1), (1, 10), (10, 10)):
        img.set(x, y, hexc("F4DC9C"))
    for (x, y) in ((0, 1), (1, 0), (11, 1), (10, 0), (0, 10), (1, 11), (11, 10), (10, 11)):
        img.set(x, y, hexc("4E3A22"))
    return img


def plate():
    """The message line's: the same dark, a thin cold edge, quieter."""
    img = Img(12, 12, hexc("0E0C12", 196))
    ring(img, 0, hexc("3C3446", 220), cut=1)
    return img


def button():
    """A button: white (the panel's colour token tints it), lit along its
    top and left, shaded along its bottom and right, its corners cut."""
    img = Img(12, 12, hexc("FFFFFF"))
    ring(img, 0, hexc("17141C"), cut=1)
    for k in range(1, 11):
        img.set(k, 1, hexc("FFFFFF"))
        img.set(1, k, hexc("E8E4F0"))
        img.set(k, 10, hexc("9890A4"))
        img.set(10, k, hexc("A8A0B4"))
    for k in range(2, 10):
        for j in range(2, 10):
            img.set(k, j, hexc("D0CADC"))
    return img


# ----- the files ---------------------------------------------------------------

LAYOUT = """
The frames:
  0-7    floor stones: 0 a slab, 1-4 two to four stones, 5 cracked, 6 worn, 7 three
  8-15   a wall from above beside a room, 8 + mask (1 open above, 2 open right, 4 open left)
  16-19  a wall's face (open below), 16 + sides (1 open left, 2 open right)
  20-21  rock with no room beside it
  22-23  a wall's face, mossy and cracked
  24-27  a wall's shadow on the floor: from above, from the left, both, the corner
  28-33  on the floor: a crack, pebbles, moss, a puddle, bones, a web
  34-37  what a fight leaves: a rat's, a bat's, a goblin's, blood
  38     a skull
"""

if __name__ == "__main__":
    tiles, heights = dungeon()
    write_sheet(
        "tiles/dungeon",
        tiles,
        8,
        """
The dungeon's tiles: 39 frames of 12 x 12, eight a row. The floor's drawn
layers name it (maps/floor.tilemap.toml) and systems/floor_art.luau picks
them (tile n is frame n - 1); nothing collides with it, the walls layer
does that. dungeon.normal.png beside it is its normal map, so the hero's
torch rakes across the stones' edges and the bricks.
"""
        + LAYOUT,
        heights,
    )
    # The hero: two idle frames, fallen, and hurt (a red flash).
    hero = sprite(HERO, HERO_PAL)
    hero.append(flash(hero[0], hexc("FF4A3A"), 0.22, hexc("FF2A2A")))
    write_sheet(
        "sprites/hero",
        hero,
        len(hero),
        """
The hero: two idle frames (the flame and the cloak shift), fallen, and
hurt (a red flash, the turn a monster strikes). systems/look.luau picks
the frame and mirrors it the way the hero last stepped.
""",
    )
    # The monsters: two idle frames and struck (a white flash).
    for name, frames, pal, what in (
        ("rat", RAT, RAT_PAL, "A rat"),
        ("bat", BAT, BAT_PAL, "A bat (wings up, wings down)"),
        ("goblin", GOBLIN, GOB_PAL, "A goblin with a knife"),
    ):
        imgs = sprite(frames, pal)
        imgs.append(flash(imgs[0], hexc("FFF6EC"), 0.7, hexc("E02828")))
        doc = what + """: two idle frames facing right, and struck (a
white flash, the turn the hero's blow lands). systems/look.luau swaps the
idle frames each turn and mirrors them to face the hero.
"""
        write_sheet("sprites/" + name, imgs, len(imgs), doc)
    write_sheet("sprites/potion", sprite([POTION], POT_PAL), 1, "A healing potion where a monster fell, drawn at its own colour (fx/shine).")
    write_sheet("sprites/stairs", sprite([STAIRS], STAIR_PAL, outline=False), 1, "The way down: steps into the dark, under a cold light (systems/look.luau).")
    write_sheet("ui/panel", [panel()], 1, "The HUD's plate and the menu's pages: dark stone under a bronze edge.\nNine-sliced at 4 pixels (UiPanel slice), drawn at twice its pixels.")
    write_sheet("ui/plate", [plate()], 1, "The message line's plate: the panel's dark, a thin cold edge.")
    write_sheet("ui/button", [button()], 1, "A menu button, white so the button colour tokens tint it (ui.toml).")
    print("wrote tiles/dungeon (and its normal map), sprites/hero, rat, bat, goblin, potion, stairs, ui/panel, plate, button")
