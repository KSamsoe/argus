"""The roguelike's art: pixel pictures drawn as text and small rules, written
as PNGs under assets/. Standard library only, and the same files every run
(the noise comes from a fixed seed). After changing anything here, run it
again from the project's folder:

    python tools/art.py

What it writes, each PNG with a sidecar (.toml) beside it that cuts it into
frames:

    assets/tiles/dungeon.png      the dungeon's tiles: floor stones, wall
                                  faces (some with torches) and tops, the rock
                                  between rooms, the shadow a wall casts, and
                                  what a fight leaves
    assets/sprites/creatures.png  the hero, the rat, the goblin, and a bat (not
                                  in the game yet), a row each
    assets/ui/panel.png           the plate the HUD and the pause screen sit on
    assets/ui/plate.png           the quieter one under the message line

systems/look.luau picks the frames: which tile each cell gets, and each
creature's frame every turn.

What to change first: the colours just below, then the creatures' pictures
(HERO, RAT, GOBLIN, BAT), where one character is one pixel. A cell is CELL x
CELL pixels drawn at one world unit; the camera (scenes/dungeon/camera.toml)
shows 32 screen pixels a unit at 960 x 540, so each art pixel is two.
"""

import os
import random
import struct
import zlib

CELL = 16  # pixels a cell, for tiles and creatures alike
SEED = 7  # the noise's seed: another number, other specks and stones

# ----- the palette -----------------------------------------------------------

INK = "0E0C12"  # the line round every creature
VOID = "0C0B10"  # the rock far from any room (the same as the camera's Background)
VOID_SPECK = "141219"

# The floor: flagstones in a cool grey, a little violet, in dark mortar.
MORTAR = "1C1922"
STONE = ["36313D", "3D3745", "45404F"]
STONE_HI = "534C60"
STONE_LO = "2B2732"

# A wall seen from above (the rock beside a room) and its lit rim.
ROCK = "1F1C25"
ROCK_SPECK = "2A2631"
RIM = "6A6179"
RIM_LO = "3E3849"

# A wall's face: three courses of bricks under a lit cap, a dark foot.
CAP = "756B84"
CAP_LO = "5A5167"
BRICK = ["4F4660", "584E6A", "494058"]
BRICK_HI = "6B607C"
BRICK_LO = "3B3348"
BRICK_MORTAR = "27212E"
FOOT = "131017"

# The torches on the walls, and the glow they put on the bricks.
FLAME_CORE = "FFF2B0"
FLAME = "FFB43C"
FLAME_OUT = "E8582A"
GLOW = "F0A050"
TORCH_WOOD = "6A4424"

# What lies about: moss, bones, and what a fight leaves.
MOSS = ["2E482D", "3C6037", "567E44"]
BONE = "D8CFB2"
BONE_LO = "9A9078"
BLOOD = "6C1A24"
BLOOD_HI = "952632"

# The UI's plates: dark stone in an iron frame with brass rivets.
PLATE_FILL = ("120F17", 236)
IRON = "6E6880"
IRON_HI = "A49DB6"
IRON_LO = "332E3D"
BRASS = "E2B866"

# ----- the creatures -----------------------------------------------------------
# Each faces right (systems/look.luau mirrors them). A line round each is added
# by rule (INK), so leave a clear pixel at every edge. The second idle frame,
# the hit flash, and the hero's fall are made from these by rule too.

HERO_COLORS = {
    ".": None,
    "r": "C8343A",  # plume
    "H": "B4BCCB",  # helmet
    "h": "737C8E",  # helmet's shade
    "S": "EFC6A0",  # skin
    "e": "241712",  # eye
    "T": "3570A8",  # tunic
    "t": "24507E",  # tunic's shade
    "B": "7A5230",  # belt
    "L": "3A3448",  # legs
    "F": "5E3E28",  # boots
    "W": "E4E8F0",  # blade
    "w": "9AA2B4",  # blade's edge
    "G": "E0B050",  # hilt
    "D": "8E5E34",  # shield
    "d": "5E3C22",  # shield's rim
    "M": "C8CCD6",  # shield's boss
}
HERO = [
    "................",
    "......rrr....W..",
    ".....HHHHH...Ww.",
    "....HHHHHHH..Ww.",
    "....hHHHHHH..Ww.",
    "....hhSSSeS..Ww.",
    ".....SSSSSS..Ww.",
    "..dd..TTTT..GGG.",
    ".dDDd.TTTTTSSS..",
    ".dMDdTTTTTt.....",
    ".dDDdTTTTTt.....",
    "..dd.BBBBBB.....",
    ".....TTt.TTt....",
    ".....LL...LL....",
    ".....LL...LL....",
    "....FFF..FFF....",
]

RAT_COLORS = {
    ".": None,
    "F": "8A6C56",  # fur
    "f": "634B3B",  # fur's shade
    "W": "B49A84",  # belly
    "E": "D99A9A",  # ears, nose, tail
    "R": "FF3A30",  # eye
}
RAT = [
    "................",
    "................",
    "................",
    "................",
    "................",
    "................",
    "................",
    "........E.E.....",
    ".......FFFFF....",
    ".....FFFFFRFF...",
    "...FFFFFFFFFFE..",
    ".E.fFFFFFFWW....",
    "..EfFFFFFWWW....",
    "...ffFFFWWW.....",
    "....F.F.F.F.....",
    "................",
]

GOBLIN_COLORS = {
    ".": None,
    "G": "6EA43E",  # skin
    "g": "4A7A2A",  # skin's shade
    "R": "FF4030",  # eyes
    "k": "F2EEDC",  # teeth
    "V": "7A4E2C",  # leather
    "v": "55361E",  # leather's shade
    "D": "CED6E2",  # blade
    "h": "4A3020",  # hilt
    "L": "3E3428",  # legs
}
GOBLIN = [
    "................",
    "................",
    "................",
    "..g........g....",
    "..gGGGGGGGgg....",
    "...GGGRGGRG.....",
    "...GGGGGGGG.....",
    "....GkkkkG...D..",
    "...vVVVVVVv..D..",
    "..GvVVVVVVvG.D..",
    "..G.VVVVVV.Ghh..",
    "....VVvVVV......",
    "....VV..VV......",
    "....LL..LL......",
    "....LL..LL......",
    "...gg..gg.......",
]

BAT_COLORS = {
    ".": None,
    "W": "5E3E86",  # wing
    "w": "3E2860",  # wing's shade
    "B": "7A5A9E",  # body
    "E": "9C7CC0",  # ears
    "Y": "FFE040",  # eyes
    "k": "F4F0F8",  # fangs
}
# Not in the game yet: ready for README idea 3, a bat that flutters.
BAT = [
    "................",
    "................",
    "................",
    "................",
    ".W...........W..",
    ".WW..E...E..WW..",
    ".WWW.BBBBB.WWW..",
    ".WwWWBYBYBWWwW..",
    "..wWWBBBBBWWw...",
    "..w.WBBBBBW.w...",
    "..w...kBk...w...",
    "................",
    "................",
    "................",
    "................",
    "................",
]

# ----- PNGs ------------------------------------------------------------------

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets")
CLEAR = (0, 0, 0, 0)


def rgba(hexcode, a=255):
    if isinstance(hexcode, tuple):
        hexcode, a = hexcode
    return (int(hexcode[0:2], 16), int(hexcode[2:4], 16), int(hexcode[4:6], 16), a)


def mix(a, b, t):
    """Colour a pulled toward colour b by t (0 to 1)."""
    return tuple(int(round(a[i] + (b[i] - a[i]) * t)) for i in range(3)) + (a[3],)


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

    def blit(self, other, ox, oy):
        for y in range(other.h):
            for x in range(other.w):
                if other.px[y][x][3]:
                    self.set(ox + x, oy + y, other.px[y][x])


def save_png(name, img):
    raw = b"".join(b"\x00" + b"".join(bytes(px) for px in row) for row in img.px)

    def chunk(kind, data):
        c = struct.pack(">I", len(data)) + kind + data
        return c + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    data = b"\x89PNG\r\n\x1a\n"
    data += chunk(b"IHDR", struct.pack(">IIBBBBB", img.w, img.h, 8, 6, 0, 0, 0))
    data += chunk(b"IDAT", zlib.compress(raw, 9))
    data += chunk(b"IEND", b"")
    path = os.path.join(ROOT, name + ".png")
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as f:
        f.write(data)


def save_sheet(name, frames, cols, doc):
    """Frames of one size, cols to a row, as one PNG, and the sidecar that
    cuts it (grid = [cols, rows]) with doc as its comment."""
    fw, fh = frames[0].w, frames[0].h
    rows = (len(frames) + cols - 1) // cols
    sheet = Img(fw * cols, fh * rows)
    # The importer pads other sizes (and says so), and warns of an empty frame.
    assert sheet.w & (sheet.w - 1) == 0 and sheet.h & (sheet.h - 1) == 0, (name, sheet.w, sheet.h)
    assert len(frames) == cols * rows and all(any(c[3] for r in f.px for c in r) for f in frames), name
    for i, f in enumerate(frames):
        sheet.blit(f, (i % cols) * fw, (i // cols) * fh)
    save_png(name, sheet)
    lines = ["# " + line if line else "#" for line in doc.strip().splitlines()]
    lines.append("# Written by tools/art.py with the picture: to change either, change art.py and run it again.")
    lines += ["grid = [%d, %d]" % (cols, rows)]
    with open(os.path.join(ROOT, name + ".toml"), "w", newline="\n") as f:
        f.write("\n".join(lines) + "\n")


# ----- the dungeon's tiles ---------------------------------------------------

rng = random.Random(SEED)


def stones(rects, extra=None):
    """A floor tile of flagstones: mortar along its top row and left column
    (the neighbours close the other two sides), each stone a shade of its
    own with a lit top-left edge, a dark bottom-right one, chipped corners,
    and a few specks; extra lays something on it."""
    t = Img(CELL, CELL, rgba(MORTAR))
    for (x0, y0, x1, y1) in rects:
        base = rgba(rng.choice(STONE))
        for y in range(y0, y1 + 1):
            for x in range(x0, x1 + 1):
                c = base
                if y == y0 or x == x0:
                    c = mix(base, rgba(STONE_HI), 0.55)
                if y == y1 or x == x1:
                    c = mix(base, rgba(STONE_LO), 0.7)
                t.set(x, y, c)
        for (cx, cy) in ((x0, y0), (x1, y0), (x0, y1), (x1, y1)):
            if rng.random() < 0.6:
                t.set(cx, cy, rgba(MORTAR))
        area = (x1 - x0 + 1) * (y1 - y0 + 1)
        for _ in range(max(1, area // 16)):
            x = rng.randint(x0 + 1, max(x0 + 1, x1 - 1))
            y = rng.randint(y0 + 1, max(y0 + 1, y1 - 1))
            dark = rng.random() < 0.5
            t.set(x, y, mix(base, rgba(STONE_LO if dark else STONE_HI), 0.45))
    if extra == "crack":
        x, y = rng.randint(5, 9), 2
        while y < 15:
            t.set(x, y, rgba(MORTAR))
            y += 1
            x = min(13, max(3, x + rng.choice((-1, 0, 0, 1))))
    elif extra == "moss":
        for (x, y, k) in ((2, 11, 1), (3, 11, 2), (3, 12, 1), (4, 12, 0), (2, 12, 0), (4, 11, 1),
                          (5, 12, 1), (3, 13, 0), (11, 4, 1), (12, 4, 0), (11, 5, 0), (12, 3, 2)):
            t.set(x, y, rgba(MOSS[k]))
    elif extra == "bones":
        for (x, y) in ((5, 10), (6, 9), (7, 8)):
            t.set(x, y, rgba(BONE))
        for (x, y) in ((4, 10), (5, 11), (7, 7), (8, 8)):
            t.set(x, y, rgba(BONE_LO))
        t.set(11, 12, rgba(BONE_LO))
    return t


def floors():
    """Frames 0-7: the floor. 0 is one plain slab (the floor layer's tile,
    under the look); systems/look.luau picks among all eight."""
    whole = [(1, 1, 15, 15)]
    two = [(1, 1, 7, 15), (9, 1, 15, 15)]
    level = [(1, 1, 15, 7), (1, 9, 15, 15)]
    three = [(1, 1, 8, 7), (10, 1, 15, 7), (1, 9, 15, 15)]
    four = [(1, 1, 7, 7), (9, 1, 15, 7), (1, 9, 7, 15), (9, 9, 15, 15)]
    return [
        stones(whole),
        stones(two),
        stones(level),
        stones(three),
        stones(four),
        stones(whole, "crack"),
        stones(three, "moss"),
        stones(four, "bones"),
    ]


def rock(base=ROCK, speck=ROCK_SPECK, specks=10):
    """Rock from above: a dark mass with specks and a few darker pits."""
    t = Img(CELL, CELL, rgba(base))
    for _ in range(specks):
        t.set(rng.randrange(CELL), rng.randrange(CELL), rgba(speck))
    for _ in range(specks // 3):
        t.set(rng.randrange(CELL), rng.randrange(CELL), mix(rgba(base), rgba(INK), 0.6))
    return t


def top(mask):
    """Frames 8-15: a wall from above beside a room, a lit rim on each side
    that meets open floor: 8 + mask, mask 1 the cell above, 2 right, 4 left."""
    t = rock()
    for i in range(CELL):
        if mask & 1:
            t.set(i, 0, rgba(RIM))
            t.set(i, 1, rgba(RIM_LO))
        if mask & 2:
            t.set(CELL - 1, i, rgba(RIM))
            t.set(CELL - 2, i, rgba(RIM_LO))
        if mask & 4:
            t.set(0, i, rgba(RIM))
            t.set(1, i, rgba(RIM_LO))
    return t


def face(sides, extra=None):
    """Frames 16-20: a wall's face, the wall cell just above open floor: a
    lit cap, three courses of bricks, a dark foot. 16 + sides, sides 1 when
    its left is open too and 2 its right (a lit corner edge); 20 mossy."""
    t = Img(CELL, CELL, rgba(BRICK_MORTAR))
    for x in range(CELL):
        t.set(x, 0, rgba(CAP))
        t.set(x, 1, rgba(CAP_LO))
        t.set(x, 14, rgba(FOOT))
        t.set(x, 15, rgba(FOOT))
    for (y0, y1, shift) in ((2, 4, 0), (6, 8, 4), (10, 12, 0)):
        x = -shift
        while x < CELL:
            base = rgba(rng.choice(BRICK))
            for y in range(y0, y1 + 1):
                for xx in range(x, x + 7):
                    c = base
                    if y == y0:
                        c = mix(base, rgba(BRICK_HI), 0.6)
                    elif y == y1:
                        c = mix(base, rgba(BRICK_LO), 0.6)
                    t.set(xx % CELL, y, c)
            x += 8
    if sides & 1:
        for y in range(14):
            t.set(0, y, rgba(CAP))
    if sides & 2:
        for y in range(14):
            t.set(CELL - 1, y, rgba(CAP))
    if extra == "moss":
        for (x, y, k) in ((1, 12, 0), (2, 12, 1), (2, 13, 0), (3, 13, 1), (3, 12, 2), (4, 13, 0),
                          (10, 13, 0), (11, 12, 2), (11, 13, 1), (12, 13, 0), (12, 12, 1)):
            t.set(x, y, rgba(MOSS[k]))
    return t


def glow():
    """Frame 21: a torch's warm light on the floor below it (a layer over
    the floor, in place of the wall's shadow there)."""
    t = Img(CELL, CELL)
    for y in range(CELL):
        for x in range(CELL):
            # An oval brightest at the wall, gone before the cell's edges.
            d = (((x - 7.5) / 7.5) ** 2 + ((y + 1.0) / 14.0) ** 2) ** 0.5
            a = int(85 * max(0.0, 1 - d) ** 1.3)
            if a:
                t.set(x, y, rgba(GLOW, a))
    return t


def torch():
    """Frame 23: a wall's face with a torch in an iron bracket, the bricks
    round it warmed by its glow (drawn, not lit: the game has no lights)."""
    t = face(0)
    cx, cy, reach = 7.5, 4.0, 6.0
    for y in range(14):
        for x in range(CELL):
            d = ((x - cx) ** 2 + (y - cy) ** 2) ** 0.5
            if d < reach:
                t.set(x, y, mix(t.get(x, y), rgba(GLOW), 0.6 * (1 - d / reach)))
    flame = [
        "...o....",
        "...oy...",
        "..oyYo..",
        "..oYYo..",
        "...yy...",
    ]
    colors = {"o": FLAME_OUT, "y": FLAME, "Y": FLAME_CORE}
    for y, line in enumerate(flame):
        for x, ch in enumerate(line):
            if ch in colors:
                t.set(4 + x, 1 + y, rgba(colors[ch]))
    for (x, y) in ((7, 6), (8, 6), (7, 7), (8, 7)):
        t.set(x, y, rgba(TORCH_WOOD))
    for (x, y) in ((6, 8), (7, 8), (8, 8), (9, 8), (6, 7), (9, 7)):
        t.set(x, y, rgba(IRON_LO))
    return t


def shade(kind):
    """Frames 24-27: the shadow a wall casts on the floor beside it (a layer
    over the floor): N from the wall above, W from the left, NW both, C a
    wall only up and to the left."""
    t = Img(CELL, CELL)
    top_alpha = (130, 90, 55, 25)
    left_alpha = (90, 50, 20)
    for y in range(CELL):
        for x in range(CELL):
            a = 0
            if kind in ("N", "NW") and y < len(top_alpha):
                a = max(a, top_alpha[y])
            if kind in ("W", "NW") and x < len(left_alpha):
                a = max(a, left_alpha[x])
            if kind == "C" and x < 3 and y < 3:
                a = max(a, (90, 50, 20)[max(x, y)])
            if a:
                t.set(x, y, (0, 0, 0, a))
    return t


def remains(kind):
    """Frames 28-31: a floor slab with what a fight left on it: a rat's, a
    goblin's, a bat's, and plain blood (where the hero fell)."""
    t = stones([(1, 1, 15, 15)])
    blood = [(6, 7), (7, 7), (8, 7), (7, 8), (8, 8), (6, 8), (5, 8), (9, 8), (7, 6), (8, 9), (7, 9)]
    if kind == "goblin" or kind == "blood":
        blood += [(4, 7), (5, 7), (9, 7), (10, 8), (6, 9), (9, 9), (5, 9), (8, 10), (6, 6), (11, 9), (4, 9)]
    if kind == "bat":
        blood = blood[:6]
    for (x, y) in blood:
        t.set(x, y, rgba(BLOOD))
    for (x, y) in ((7, 7), (8, 8)):
        t.set(x, y, rgba(BLOOD_HI))
    for (x, y) in ((3, 11), (12, 5), (11, 12)):
        t.set(x, y, rgba(BLOOD))
    if kind == "bat":
        # A torn wing.
        for (x, y) in ((9, 9), (10, 9), (10, 10), (11, 10), (12, 10), (11, 11)):
            t.set(x, y, rgba("3E2860"))
    elif kind == "rat":
        # A tuft of fur and the tail.
        for (x, y) in ((10, 10), (11, 10), (10, 11)):
            t.set(x, y, rgba("634B3B"))
        for (x, y) in ((12, 11), (13, 12), (13, 13)):
            t.set(x, y, rgba("D99A9A"))
    elif kind == "goblin":
        # The dropped knife.
        for (x, y) in ((10, 12), (11, 12), (12, 12), (13, 12)):
            t.set(x, y, rgba("CED6E2"))
        t.set(9, 12, rgba("4A3020"))
        t.set(8, 12, rgba("4A3020"))
    return t


def dungeon():
    tiles = floors()  # 0-7
    tiles += [top(m) for m in range(8)]  # 8-15
    tiles += [face(s) for s in range(4)]  # 16-19
    tiles += [face(0, "moss"), glow()]  # 20-21
    tiles += [rock(VOID, VOID_SPECK, 3), torch()]  # 22-23
    tiles += [shade(k) for k in ("N", "W", "NW", "C")]  # 24-27
    tiles += [remains(k) for k in ("rat", "goblin", "bat", "blood")]  # 28-31
    return tiles


# ----- the creatures, by rule ------------------------------------------------


def picture(rows, colors):
    assert len(rows) == CELL, len(rows)
    img = Img(CELL, CELL)
    for y, line in enumerate(rows):
        assert len(line) == CELL, (line, len(line))
        for x, ch in enumerate(line):
            if colors[ch]:
                img.px[y][x] = rgba(colors[ch])
    return img


def outlined(img):
    """A line of INK round the picture's shape: every clear pixel beside a
    drawn one, so a creature reads against any floor."""
    out = Img(img.w, img.h)
    out.px = [row[:] for row in img.px]
    for y in range(img.h):
        for x in range(img.w):
            if not img.px[y][x][3] and any(img.get(x + dx, y + dy)[3] for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1))):
                out.px[y][x] = rgba(INK)
    return out


def bob(img, feet):
    """The second idle frame: everything above row feet one pixel lower,
    the feet where they were (a breath; look.luau swaps the two each turn)."""
    out = Img(img.w, img.h)
    for y in range(img.h):
        src = y if y > feet else y - 1
        out.px[y] = img.px[src][:] if src >= 0 else [CLEAR] * img.w
    return out


def flash(img, toward, k, line):
    """The frame a blow lands on: every colour pulled toward toward by k,
    the outline in line."""
    out = Img(img.w, img.h)
    for y in range(img.h):
        for x in range(img.w):
            c = img.px[y][x]
            if c[3]:
                out.px[y][x] = rgba(line) if c == rgba(INK) else mix(c, rgba(toward), k)
    return out


def fallen(img):
    """The hero fallen: the picture turned a quarter round, lying on the
    floor of its cell."""
    turned = Img(img.w, img.h)
    for y in range(img.h):
        for x in range(img.w):
            turned.px[x][img.w - 1 - y] = img.px[y][x]
    rows = [r for r in turned.px if any(c[3] for c in r)]
    out = Img(img.w, img.h)
    for i, r in enumerate(rows):
        out.px[img.h - len(rows) + i - 1] = r
    return out


def lunge(img):
    """A monster biting: the picture a pixel forward and a pixel up, a jump
    at what it bites."""
    out = Img(img.w, img.h)
    for y in range(img.h):
        for x in range(img.w):
            out.set(x + 1, y - 1, img.px[y][x])
    return out


def creature(rows, colors, feet, hurt, hurt_line, k):
    """A row of the sheet: idle, idle a pixel lower, and struck."""
    base = picture(rows, colors)
    idle = outlined(base)
    return [idle, outlined(bob(base, feet)), flash(idle, hurt, k, hurt_line)]


def monster(rows, colors, feet):
    """A monster's row: idle, idle, struck (a white flash), and biting."""
    row = creature(rows, colors, feet, "FFF6EC", "E02828", 0.7)
    return row + [outlined(lunge(picture(rows, colors)))]


def creatures():
    hero = creature(HERO, HERO_COLORS, 12, "FF4A3A", "FF2A2A", 0.3)
    hero.append(outlined(fallen(picture(HERO, HERO_COLORS))))
    return hero + monster(RAT, RAT_COLORS, 13) + monster(GOBLIN, GOBLIN_COLORS, 12) + monster(BAT, BAT_COLORS, 10)


# ----- the UI's plates -------------------------------------------------------


def frame_ring(img, inset, c, cut=0):
    """The ring inset pixels in from the edge, in c (its corners cut by cut)."""
    n = img.w
    for k in range(inset + cut, n - inset - cut):
        for (x, y) in ((k, inset), (k, n - 1 - inset), (inset, k), (n - 1 - inset, k)):
            img.set(x, y, c)


def panel():
    """16 x 16, nine-sliced at 4 pixels: dark stone in an iron frame, lit
    along its top and left, with a brass rivet in each corner."""
    n = 16
    img = Img(n, n, rgba(PLATE_FILL))
    frame_ring(img, 0, rgba(IRON_LO), cut=1)
    frame_ring(img, 1, rgba(IRON))
    for k in range(2, n - 2):
        img.set(k, 1, rgba(IRON_HI))
        img.set(1, k, rgba(IRON_HI))
    frame_ring(img, 2, rgba(IRON_LO))
    for (x, y) in ((1, 1), (n - 2, 1), (1, n - 2), (n - 2, n - 2)):
        img.set(x, y, rgba(BRASS))
    return img


def plate():
    """16 x 16, nine-sliced at 3 pixels: the same dark, a thin cold edge."""
    n = 16
    img = Img(n, n, rgba(PLATE_FILL[0], 205))
    frame_ring(img, 0, rgba(IRON_LO, 240), cut=1)
    for k in range(2, n - 2):
        img.set(k, 1, rgba("2A2533", 230))
    return img


# ----- the files ---------------------------------------------------------------

TILES_DOC = """
The dungeon's tiles: 32 frames of 16 x 16, eight to a row, drawn by
tools/art.py. The floor layer of maps/dungeon.tilemap.toml draws frame 0
under everything; systems/look.luau paints the rest on the look layers (tile
n is frame n - 1). Nothing here collides: the walls layer does that.

  0-7    the floor: 0 one slab, 1-4 two to four stones, 5 cracked, 6 mossy, 7 bones
  8-15   a wall from above beside a room, 8 + mask (1 floor above, 2 right, 4 left)
  16-19  a wall's face (floor below it), 16 + sides (1 open left, 2 open right)
  20     a wall's face with moss
  21     a torch's light on the floor below it
  22     rock far from any room
  23     a wall's face with a torch
  24-27  a wall's shadow on the floor: from above, from the left, both, the corner
  28-31  what a fight leaves: a rat's, a goblin's, a bat's, blood
"""

CREATURES_DOC = """
The creatures: 16 x 16 frames, four to a row, facing right, drawn by
tools/art.py. A row each: the hero (0 and 1 idle, 2 hurt, 3 fallen), then
the rat (4), the goblin (8), and the bat (12, not in the game yet), each
idle, idle, struck, and biting. systems/look.luau picks the frame each
turn and mirrors it to face the way the creature looks. A new kind of
monster is a new row here and a line in look.luau's ROWS.
"""

if __name__ == "__main__":
    save_sheet("tiles/dungeon", dungeon(), 8, TILES_DOC)
    save_sheet("sprites/creatures", creatures(), 4, CREATURES_DOC)
    save_sheet("ui/panel", [panel()], 1, "The plate the HUD and the pause screen sit on: dark stone in an iron frame.\nNine-sliced at 4 pixels and drawn at twice its size (UiPanel slice and scale).")
    save_sheet("ui/plate", [plate()], 1, "The message line's plate: the HUD's dark with a thin cold edge.\nNine-sliced at 3 pixels and drawn at twice its size (UiPanel slice and scale).")
    print("wrote assets/tiles/dungeon, sprites/creatures, ui/panel, ui/plate")
