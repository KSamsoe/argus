"""The blank game's art, the player's sprite sheet and the floor, drawn as
text (one character a pixel) or by rule and written as PNGs into assets/,
where the engine picks them up, even while the game runs. Standard library
only, and the same files every run:

    python tools/art.py

To recolour it, change the palette below and run it again. To use your own
art, draw assets/player.png (five 18 x 18 frames in a row, facing right)
and assets/floor.png in any paint program instead of running this.
"""

import os
import random
import struct
import zlib

ASSETS = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets")

# Sizes, in the art's pixels. The camera shows 15 world units on 540 rows of
# screen, 36 a unit, so at 9 pixels a unit each pixel is a 4 x 4 square.
PX = 9
FRAME = 18  # a player frame: 2 x 2 units (the Sprite's size in player.toml)
FLOOR = 30 * PX  # the floor: 30 x 30 units (floor.toml)
SLAB = 3 * PX  # the floor's grid of flagstones, some cut in two
SEED = 1  # change it for another floor

# The palette, as RRGGBB.
INK = "1B1A22"  # outlines, the eye, the boots
HAIR, HAIR_DARK = "6B4630", "4A2F22"
SKIN, BELT = "F5CDA4", "3A2E3A"
SHIRT, SHIRT_DARK = "F2C14E", "C98B32"
LEG, LEG_FAR = "7A5236", "553826"  # the near leg, and the far one
STONES = ["2B3A42", "2E3E47", "283740", "314450"]  # one picked per stone
KERB = ["3B4D57", "384953"]  # the border of stones round the floor
LIGHT, SHADE, JOINT = "36484F", "243138", "182126"  # a stone's lit and shaded edges, the gaps
MOSS = ["3E5A3C", "4F7044"]
RIM = "11171B"  # the floor's outermost pixels

# The player, facing right. The head and shirt are the same in every frame;
# the near arm and the legs change. Frame 0 stands; 1 to 4 walk.
TOP = [
    "......ooooo.......",
    ".....oHHHHHo......",
    "....oHHHHHHHo.....",
    "....oHHHHHHHHo....",
    "....ohHHHSSSSo....",
    "....ohHHSSSESo....",
    "....ohhHSSSESo....",
    ".....ohhSSSSo.....",
    ".....oYYooooYo....",
    "....oyYYYYYYYo....",
    "....oyYYYYYYYo....",
    "....oyYYYYYYYo....",
    ".....oBBBBBBo.....",
]
ARMS = {  # the near arm's pixels, (x, y, colour)
    "mid": [(9, 9, "y"), (9, 10, "y"), (9, 11, "S")],
    "fwd": [(10, 9, "y"), (11, 10, "y"), (12, 11, "S")],
    "back": [(8, 9, "y"), (7, 10, "y"), (6, 11, "S")],
}
LEGS = {  # three rows under the shirt
    "stand": [".......ff.LL......", ".......ff.LL......", ".......oo.oo......"],
    "stride": [".......ff.LL......", "......ff...LL.....", ".....oo.....oo...."],
    "pass": ["........fLL.......", "........fLL.......", "........ooo......."],
    "stride2": [".......LL.ff......", "......LL...ff.....", ".....oo.....oo...."],
}
# Each frame: its legs, its arm (swung against the near leg), how far the body sinks.
FRAMES = [("stand", "mid", 0), ("stride", "back", 1), ("pass", "mid", 0), ("stride2", "fwd", 1), ("pass", "mid", 0)]
PLAYER = {"o": INK, "E": INK, "H": HAIR, "h": HAIR_DARK, "S": SKIN, "Y": SHIRT, "y": SHIRT_DARK,
          "B": BELT, "L": LEG, "f": LEG_FAR}


def rgba(hexc):
    return (int(hexc[0:2], 16), int(hexc[2:4], 16), int(hexc[4:6], 16), 255)


def png(path, rows):
    """Write rows (lists of (r, g, b, a)) as an RGBA PNG under assets/."""
    h, w = len(rows), len(rows[0])
    raw = b"".join(b"\x00" + bytes(v for px in row for v in px) for row in rows)

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    data = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
    data += chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")
    with open(os.path.join(ASSETS, path), "wb") as f:
        f.write(data)


def player_frame(legs, arm, sink):
    px = [[(0, 0, 0, 0)] * FRAME for _ in range(FRAME)]
    for x in range(3, 15):  # a soft shadow under the feet
        px[16][x] = (0, 0, 0, 110 if 4 <= x <= 13 else 50)
    marks = [(x, 13 + y, ch) for y, line in enumerate(LEGS[legs]) for x, ch in enumerate(line)]
    marks += [(x, y + sink, ch) for y, line in enumerate(TOP) for x, ch in enumerate(line)]
    marks += [(x, y + sink, ch) for x, y, ch in ARMS[arm]]
    for x, y, ch in marks:
        if ch != ".":
            px[y][x] = rgba(PLAYER[ch])
    return px


def player_sheet():
    frames = [player_frame(*f) for f in FRAMES]
    return [sum((f[y] for f in frames), []) for y in range(FRAME)]


def floor():
    """Flagstones in rows, some cut in two, inside a kerb of stones: lit
    and shaded edges, a little grain, and moss in some of the joints."""
    rng = random.Random(SEED)
    px = [[JOINT] * FLOOR for _ in range(FLOOR)]

    def stone(x0, y0, x1, y1, colours=STONES):  # its last row and column stay joint
        base = rng.choice(colours)
        for y in range(y0, y1 - 1):
            for x in range(max(x0, 0), min(x1 - 1, FLOOR)):
                c = base
                if y == y0 or x == x0:
                    c = LIGHT
                elif y == y1 - 2 or x == x1 - 2:
                    c = SHADE
                elif rng.random() < 0.06:
                    c = rng.choice(colours)
                px[y][x] = c

    for y0 in range(0, FLOOR, SLAB):  # each row set over by 0 to 2 units
        for x0 in range(-rng.choice((0, 1, 2)) * PX, FLOOR, SLAB):
            x1, y1, cut, r = x0 + SLAB, y0 + SLAB, rng.choice((1, 2)) * PX, rng.random()
            if r < 0.5:
                stone(x0, y0, x1, y1)
            elif r < 0.75:
                stone(x0, y0, x0 + cut, y1)
                stone(x0 + cut, y0, x1, y1)
            else:
                stone(x0, y0, x1, y0 + cut)
                stone(x0, y0 + cut, x1, y1)
    for a in range(0, FLOOR, SLAB):  # the kerb, a unit wide
        for x0, y0, x1, y1 in ((a, 0, a + SLAB, PX), (a, FLOOR - PX, a + SLAB, FLOOR),
                               (0, a, PX, a + SLAB), (FLOOR - PX, a, FLOOR, a + SLAB)):
            stone(x0, y0, x1, y1, KERB)
    joints = [(x, y) for y in range(1, FLOOR - 1) for x in range(1, FLOOR - 1) if px[y][x] == JOINT]
    for _ in range(40):  # moss, in patches along the joints
        cx, cy, r = rng.randrange(FLOOR), rng.randrange(FLOOR), rng.randrange(5, 18)
        for x, y in joints:
            if (x - cx) ** 2 + (y - cy) ** 2 < r * r and rng.random() < 0.25:
                for dx, dy in ((0, 0), (1, 0), (0, -1), (-1, 0), (0, 1)):
                    if rng.random() < 0.6:
                        px[y + dy][x + dx] = rng.choice(MOSS)
    for i in range(FLOOR):  # a dark rim all round
        px[0][i] = px[FLOOR - 1][i] = px[i][0] = px[i][FLOOR - 1] = RIM
    return [[rgba(c) for c in line] for line in px]


if __name__ == "__main__":
    png("player.png", player_sheet())
    png("floor.png", floor())
    print("wrote assets/player.png and assets/floor.png")
