"""Outpost's art: pixel grids and small procedures, written as PNGs and sidecars.

Run it once and the files land in assets/, where the importer picks them up.
Standard library only, and the same files every run (the noise is seeded).

    python tools/art.py                  # writes assets/
    python tools/art.py --preview DIR    # and enlarged previews of each sheet in DIR

Everything is drawn at 8 pixels to a world unit (a grid cell), the scale of
Hollow's tiles; the default view (26 units tall on a 540 px canvas) shows a
pixel at about 2.6 screen pixels. The frame numbers each sheet holds are the
ones systems/art.luau picks, so a change of layout here is a change there.

What it draws (3/4 top-down; light from the upper left):

- ground/valley: the valley's 40 x 24 cells and a forest margin round them,
  one image (paths, the yard, the tilled plot, the bunk deck, a stream);
  ground/grass: a tile for a valley of another size (the stress scene),
  drawn by the materials/ground material.
- trees: three kinds of tree, each standing, swaying under the axe, young,
  a sapling, a shoot from a stump, and a stump.
- colonist: six colonists (the founders' looks) in 21 poses.
- items: crops, logs, and stone, on the ground and carried overhead.
- field: a field's soil from tilled to ripe, and half cut.
- bed: a cot in its owner's colours, empty or slept in; stockpile (64
  frames: wood, crops, and meals at 4 levels each); stove; campfire; grave.
- ui/panel, ui/button, ui/hud: nine-sliced panels for the menus and HUD.
- fx/smoke, fx/bits (particle effects), materials/art (a frame without its
  tint) and materials/ground (the grass tile repeated): text, not pixels.
"""

import math
import os
import random
import struct
import sys
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.join(HERE, "..", "assets")
PPU = 8

CLEAR = (0, 0, 0, 0)


def hexc(s, a=255):
    s = s.lstrip("#")
    if len(s) == 8:
        a = int(s[6:8], 16)
    return (int(s[0:2], 16), int(s[2:4], 16), int(s[4:6], 16), a)


def mix(c1, c2, k):
    return tuple(int(round(c1[i] + (c2[i] - c1[i]) * k)) for i in range(4))


# --- PNG ---------------------------------------------------------------------


def _chunk(kind, data):
    c = struct.pack(">I", len(data)) + kind + data
    return c + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)


def png_bytes(w, h, rows):
    """rows: h lists of w (r, g, b, a). Indexed when it has 256 colours or
    fewer (smaller files), else RGBA."""
    colours = {}
    for row in rows:
        for px in row:
            if px[3] == 0:
                px = CLEAR
            if px not in colours:
                colours[px] = len(colours)
                if len(colours) > 256:
                    break
    sig = b"\x89PNG\r\n\x1a\n"
    if len(colours) <= 256:
        pal = sorted(colours, key=lambda c: colours[c])
        index = {c: i for i, c in enumerate(pal)}
        raw = b"".join(
            b"\x00" + bytes(index[CLEAR if px[3] == 0 else px] for px in row) for row in rows
        )
        data = sig + _chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 3, 0, 0, 0))
        data += _chunk(b"PLTE", b"".join(bytes(c[:3]) for c in pal))
        data += _chunk(b"tRNS", bytes(c[3] for c in pal))
    else:
        raw = b"".join(b"\x00" + b"".join(bytes(px) for px in row) for row in rows)
        data = sig + _chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
    data += _chunk(b"IDAT", zlib.compress(raw, 9))
    data += _chunk(b"IEND", b"")
    return data


WRITTEN = []


def write(path, data):
    full = os.path.join(ROOT, path)
    os.makedirs(os.path.dirname(full), exist_ok=True)
    if isinstance(data, str):
        data = data.encode("utf-8")
    with open(full, "wb") as f:
        f.write(data)
    WRITTEN.append((path, len(data)))


# --- Drawing -------------------------------------------------------------------


class Canvas:
    def __init__(self, w, h, fill=CLEAR):
        self.w, self.h = w, h
        self.px = [[fill] * w for _ in range(h)]

    def get(self, x, y):
        if 0 <= x < self.w and 0 <= y < self.h:
            return self.px[y][x]
        return CLEAR

    def set(self, x, y, c):
        if c is None:
            return
        if 0 <= x < self.w and 0 <= y < self.h:
            self.px[y][x] = c

    def over(self, x, y, c):
        """c drawn over what is there, by its alpha."""
        if c is None or not (0 <= x < self.w and 0 <= y < self.h):
            return
        a = c[3] / 255.0
        if a >= 1:
            self.px[y][x] = c
            return
        b = self.px[y][x]
        ba = b[3] / 255.0
        oa = a + ba * (1 - a)
        if oa <= 0:
            return
        rgb = [int(round((c[i] * a + b[i] * ba * (1 - a)) / oa)) for i in range(3)]
        self.px[y][x] = (rgb[0], rgb[1], rgb[2], int(round(oa * 255)))

    def grid(self, rows, pal, ox=0, oy=0):
        for y, line in enumerate(rows):
            for x, ch in enumerate(line):
                if ch in pal and pal[ch] is not None:
                    self.over(ox + x, oy + y, pal[ch])

    def paste(self, other, ox, oy):
        for y in range(other.h):
            for x in range(other.w):
                c = other.px[y][x]
                if c[3]:
                    self.over(ox + x, oy + y, c)

    def ellipse(self, cx, cy, rx, ry, c):
        for y in range(int(cy - ry - 1), int(cy + ry + 2)):
            for x in range(int(cx - rx - 1), int(cx + rx + 2)):
                dx, dy = (x + 0.5 - cx) / rx, (y + 0.5 - cy) / ry
                if dx * dx + dy * dy <= 1.0:
                    self.over(x, y, c)

    def rect(self, x0, y0, w, h, c):
        for y in range(y0, y0 + h):
            for x in range(x0, x0 + w):
                self.over(x, y, c)

    def outline(self, c, solid=200):
        """A one-pixel outline round everything at least `solid` opaque."""
        body = [[self.px[y][x][3] >= solid for x in range(self.w)] for y in range(self.h)]
        out = []
        for y in range(self.h):
            for x in range(self.w):
                if body[y][x]:
                    continue
                for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                    nx, ny = x + dx, y + dy
                    if 0 <= nx < self.w and 0 <= ny < self.h and body[ny][nx]:
                        out.append((x, y))
                        break
        for x, y in out:
            self.over(x, y, c)

    def shifted(self, dx, dy, rows=None):
        o = Canvas(self.w, self.h)
        for y in range(self.h):
            for x in range(self.w):
                ry = rows is None or y in rows
                sx, sy = (x - dx, y - dy) if ry else (x, y)
                o.px[y][x] = self.get(sx, sy)
        return o


def sheet(frames, cols):
    """Frames of one size in rows of cols."""
    fw, fh = frames[0].w, frames[0].h
    rows = (len(frames) + cols - 1) // cols
    s = Canvas(fw * cols, fh * rows)
    for i, f in enumerate(frames):
        s.paste(f, (i % cols) * fw, (i // cols) * fh)
    return s


def save(path, canvas):
    write(path, png_bytes(canvas.w, canvas.h, canvas.px))
    PREVIEWS.append((path, canvas))


PREVIEWS = []

SHADOW = (12, 20, 8, 70)
INK = hexc("1E1610", 235)  # outlines

# --- Colonists -------------------------------------------------------------------
#
# Frames of 14 x 18; the feet stand on the pivot, 3 px above the bottom. A
# row a colonist: Ada, Bram, Cleo, Dov, Esme, Finn (defs/founders.toml).

LOOKS = [
    # shirt, shirt shade, hair, hair shade, skin, skin shade, trousers, trousers shade, long hair
    ("D0493E", "922F2A", "5A3420", "3C2214", "F2C9A0", "D9A57A", "45405C", "302C44", True),  # Ada
    ("3F72BA", "2A4E88", "221C1C", "100C0C", "D9A274", "B8804F", "5C4A38", "403226", False),  # Bram
    ("E4B03C", "A9802A", "B5522C", "7E3820", "F6D4B2", "DDB08A", "45405C", "302C44", True),  # Cleo
    ("2E9C88", "1E6C5E", "E2C25E", "B8963C", "A86E48", "8A5434", "5C4A38", "403226", False),  # Dov
    ("9058AE", "643C7E", "2C201C", "140E0C", "7C4C32", "5E3622", "3A3A4C", "28283A", True),  # Esme
    ("E27E32", "A85A20", "7C5232", "5A3A22", "F0C8A0", "D4A47A", "4C5C3C", "36432A", False),  # Finn
]
BOOT = hexc("3A2A20")
AXE_WOOD, AXE_HEAD, AXE_EDGE = hexc("9A6A3E"), hexc("C9CDD5"), hexc("8A8F98")
LADLE = hexc("B98C55")
BOWL, BOWL_DARK, STEW = hexc("A06E40"), hexc("74502E"), hexc("F0B858")
GRAIN = hexc("E8C860")

# Body grids: 12 x 13, pasted at (1, 2) in the frame.
# h hair, H hair shade, s skin, S skin shade, t shirt, T shirt shade,
# p trousers, P trousers shade, b boots, L long hair (else clear), M long
# hair (else shirt).
HEAD_FRONT = ["............", "....hhhh....", "...hhhhhh...", "...hssssh...", "...LssssL..."]
HEAD_BACK = ["............", "....hhhh....", "...hhhhhh...", "...hhhhhh...", "...LHHHHL..."]
HEAD_SIDE = ["............", "....hhhh....", "...hhhhhh...", "...hhhsss...", "...LHssss..."]

TORSO_FRONT = ["...MttttM...", "..TttttttT..", "..TttttttT..", "..stttttts.."]
TORSO_BACK = ["...tMMMMt...", "..TttMMttT..", "..TttttttT..", "..stttttts.."]
TORSO_UP = ["...MttttM...", "...tttttt...", "...tttttt...", "...tttttt..."]  # arms raised

LEGS_STAND = ["...pppppp...", "...pP..Pp...", "...pP..Pp...", "...bb..bb..."]
LEGS_F1 = ["...pppppp...", "...pP..Pp...", "...bb..Pp...", ".......bb..."]
LEGS_F2 = ["...pppppp...", "...pP..Pp...", "...pP..bb...", "...bb......."]


def side_torso(arm):
    """Side view (facing right): a narrow torso and the near arm forward,
    down, or back."""
    t = ["...Ltttt....", "....tttt....", "....tttt....", "....tttt...."]
    t = [list(r) for r in t]
    if arm == "forward":
        for x, y, ch in ((6, 1, "T"), (7, 2, "T"), (8, 3, "s")):
            t[y][x] = ch
    elif arm == "down":
        for x, y, ch in ((5, 1, "T"), (5, 2, "T"), (5, 3, "s")):
            t[y][x] = ch
    elif arm == "back":
        for x, y, ch in ((5, 1, "T"), (4, 2, "T"), (3, 3, "s")):
            t[y][x] = ch
    return ["".join(r) for r in t]


LEGS_S1 = ["....pppp....", "...PP..pp...", "..PP....pp..", "..bb....bb.."]
LEGS_S2 = ["....pppp....", ".....pP.....", ".....pP.....", ".....bbb...."]
LEGS_S3 = ["....pppp....", "...pp..PP...", "..pp....PP..", "..bb....bb.."]
LEGS_S4 = ["....pppp....", ".....Pp.....", ".....Pp.....", ".....bbb...."]

# Crouched, side view: the body 2 px lower, knee forward.
CROUCH = [
    "............",
    "............",
    "............",
    "....hhhh....",
    "...hhhhhh...",
    "...hhhsss...",
    "...LHssss...",
    "...Ltttt....",
    "....tttt....",
    "....tttt....",
    "...pppppp...",
    "...pP..ppp..",
    "...bb...bb..",
]


def body(head, torso, legs):
    return head + torso + legs


def colonist_frames(look):
    shirt, shirt2, hair, hair2, skin, skin2, pants, pants2, long_hair = look
    pal = {
        "h": hexc(hair),
        "H": hexc(hair2),
        "s": hexc(skin),
        "S": hexc(skin2),
        "t": hexc(shirt),
        "T": hexc(shirt2),
        "p": hexc(pants),
        "P": hexc(pants2),
        "b": BOOT,
        "L": hexc(hair) if long_hair else None,
        "M": hexc(hair) if long_hair else hexc(shirt),
    }

    def frame(rows, tools=(), dy=0, shadow=True):
        c = Canvas(14, 18)
        figure = Canvas(14, 18)
        figure.grid(rows, pal, 1, 2 + dy)
        for x, y, col in tools:
            figure.set(x, y, col)
        figure.outline(INK)
        if shadow:
            c.ellipse(7, 15.2, 3.6, 1.3, SHADOW)
        c.paste(figure, 0, 0)
        return c

    stand = body(HEAD_FRONT, TORSO_FRONT, LEGS_STAND)
    back = body(HEAD_BACK, TORSO_BACK, LEGS_STAND)
    f = []
    f.append(frame(stand))  # 0 stand
    f.append(frame(body(HEAD_FRONT, TORSO_FRONT, LEGS_F1)))  # 1 walk down
    f.append(frame(body(HEAD_FRONT, TORSO_FRONT, LEGS_F2)))  # 2
    f.append(frame(body(HEAD_BACK, TORSO_BACK, LEGS_F1)))  # 3 walk up
    f.append(frame(body(HEAD_BACK, TORSO_BACK, LEGS_F2)))  # 4
    f.append(frame(body(HEAD_SIDE, side_torso("forward"), LEGS_S1)))  # 5 walk right
    f.append(frame(body(HEAD_SIDE, side_torso("down"), LEGS_S2), dy=-1))  # 6
    f.append(frame(body(HEAD_SIDE, side_torso("back"), LEGS_S3)))  # 7
    f.append(frame(body(HEAD_SIDE, side_torso("down"), LEGS_S4), dy=-1))  # 8
    # 9, 10: carrying overhead (the load is the item's own sprite above).
    up = ["............", "..s.hhhh.s..", "..ThhhhhhT..", "..ThsssshT..", "..TLssssLT.."]
    f.append(frame(up + TORSO_UP + LEGS_F1))  # 9
    f.append(frame(up + TORSO_UP + LEGS_F2))  # 10
    # 11, 12: chopping, facing right: the axe up behind, then struck forward.
    raised = body(HEAD_SIDE, side_torso("none"), LEGS_S1)
    raised = [list(r) for r in raised]
    for x, y in ((5, 5), (4, 4)):
        raised[y][x] = "T"
    raised[3][4] = "s"
    raised = ["".join(r) for r in raised]
    f.append(frame(raised, tools=[(4, 4, AXE_WOOD), (4, 3, AXE_WOOD), (3, 2, AXE_WOOD), (2, 1, AXE_HEAD), (3, 1, AXE_HEAD), (2, 2, AXE_EDGE), (1, 1, AXE_EDGE)]))
    struck = body(HEAD_SIDE, side_torso("forward"), LEGS_S1)
    f.append(frame(struck, tools=[(10, 10, AXE_WOOD), (11, 10, AXE_WOOD), (12, 11, AXE_HEAD), (12, 10, AXE_HEAD), (13, 11, AXE_EDGE), (13, 10, AXE_EDGE)]))
    # 13, 14: harvesting: crouched, reaching into the crop and pulling.
    reach = [list(r) for r in CROUCH]
    for x, y, ch in ((7, 8, "T"), (8, 9, "T"), (9, 10, "s")):
        reach[y][x] = ch
    f.append(frame(["".join(r) for r in reach]))
    pull = [list(r) for r in CROUCH]
    for x, y, ch in ((7, 8, "T"), (8, 8, "s")):
        pull[y][x] = ch
    f.append(frame(["".join(r) for r in pull], tools=[(10, 9, GRAIN), (10, 8, GRAIN), (11, 8, GRAIN), (9, 10, GRAIN)]))
    # 15, 16: cooking, facing right: the ladle stirs the pot.
    stir = body(HEAD_SIDE, side_torso("forward"), LEGS_STAND[:1] + LEGS_S2[1:])
    f.append(frame(stir, tools=[(10, 9, LADLE), (11, 10, LADLE), (12, 11, LADLE)]))
    f.append(frame(stir, tools=[(10, 9, LADLE), (11, 9, LADLE), (12, 10, LADLE)]))
    # 17, 18: eating from a bowl, facing out.
    hold = [list(r) for r in stand]
    hold[8] = list("...tsssst...")
    f.append(frame(["".join(r) for r in hold], tools=[(5, 9, BOWL), (6, 9, BOWL), (7, 9, BOWL), (8, 9, BOWL), (6, 10, BOWL_DARK), (7, 10, BOWL_DARK), (6, 8, STEW), (7, 8, STEW)]))
    lift = [list(r) for r in stand]
    lift[7] = list("..TttttttT..")
    lift[8] = list("..sttttttT..")
    lift[6][8] = "s"
    f.append(frame(["".join(r) for r in lift], tools=[(8, 6, BOWL), (9, 6, BOWL), (9, 7, BOWL_DARK), (8, 5, STEW)]))
    # 19: bending to pick something up, facing right.
    pick = [list(r) for r in CROUCH]
    for x, y, ch in ((7, 9, "T"), (8, 10, "T"), (9, 11, "s")):
        pick[y][x] = ch
    f.append(frame(["".join(r) for r in pick]))
    # 20: standing, facing away.
    f.append(frame(back))
    return f


def colonists():
    frames = []
    for look in LOOKS:
        frames.extend(colonist_frames(look))
    save("colonist.png", sheet(frames, 21))
    write(
        "colonist.toml",
        """# Outpost's colonists, drawn by tools/art.py: 21 poses of 14 x 18 px in a
# row, a row each for Ada, Bram, Cleo, Dov, Esme, and Finn. The feet stand
# on the pivot. systems/art.luau picks the frame (row * 21 + pose):
# 0 stand, 1-2 walk down, 3-4 walk up, 5-8 walk right (flipped for left),
# 9-10 carry overhead, 11-12 chop, 13-14 harvest, 15-16 cook, 17-18 eat,
# 19 pick up, 20 stand facing away.
grid = [21, 6]
pivot = [0.5, 0.1667]
""",
    )


# --- Trees ---------------------------------------------------------------------
#
# Frames of 16 x 24; the trunk's foot on the pivot, 3 px above the bottom.
# Three kinds, seven frames each: 0 standing, 1-2 swaying under the axe,
# 3 young, 4 sapling, 5 a shoot from a stump, 6 a stump.

TREE_KINDS = {
    "oak": {
        "leaf": ["24461F", "2F5A27", "3D6F31", "4F853C", "68A04C"],
        "bark": ["4A3020", "6A4630", "8A6040"],
    },
    "pine": {
        "leaf": ["17362A", "1F4733", "2A5A40", "37704E", "4C8A60"],
        "bark": ["42291C", "5E3C28", "7A5236"],
    },
    "birch": {
        "leaf": ["3A5A22", "4C7228", "618A32", "7AA43E", "9CC058"],
        "bark": ["8C8476", "D8D2C2", "F2EEE2"],
    },
}


def shade_of(x, y, cx, cy, r, rng):
    s = ((x - cx) * -0.62 + (y - cy) * -0.78) / max(r, 1)
    s += rng.uniform(-0.18, 0.18)
    if s > 0.5:
        return 4
    if s > 0.15:
        return 3
    if s > -0.25:
        return 2
    if s > -0.6:
        return 1
    return 0


def canopy(c, blobs, leaf, rng, cx, cy, r):
    """Leaves: the union of blobs, shaded by the light from the upper left."""
    inside = set()
    for bx, by, br in blobs:
        for y in range(int(by - br - 1), int(by + br + 2)):
            for x in range(int(bx - br - 1), int(bx + br + 2)):
                if (x + 0.5 - bx) ** 2 + (y + 0.5 - by) ** 2 <= br * br:
                    inside.add((x, y))
    for x, y in inside:
        c.set(x, y, hexc(leaf[shade_of(x + 0.5, y + 0.5, cx, cy, r, rng)]))
    # A few clumps of light on the lit side, and dark under the leaves.
    for _ in range(int(r * 1.5)):
        x, y = rng.randint(int(cx - r), int(cx)), rng.randint(int(cy - r), int(cy))
        if (x, y) in inside and (x + 1, y) in inside:
            c.set(x, y, hexc(leaf[4]))
            c.set(x + 1, y, hexc(leaf[3]))
    for x, y in inside:
        if (x, y + 1) not in inside:
            c.set(x, y, hexc(leaf[0]))
    return inside


def trunk(c, bark, x0, top, bottom, w=2):
    for y in range(top, bottom + 1):
        for i in range(w):
            k = 2 if i == 0 else (1 if i < w - 1 else 0)
            c.set(x0 + i, y, hexc(bark[k]))


def tree_frame(kind, stage, seed):
    rng = random.Random(seed)
    k = TREE_KINDS[kind]
    leaf, bark = k["leaf"], k["bark"]
    c = Canvas(16, 24)
    body = Canvas(16, 24)
    foot = 20  # the trunk's last row
    if stage in (0, 3):
        big = stage == 0
        if kind == "pine":
            h = 18 if big else 12
            top = foot - h
            trunk(body, bark, 7, foot - 3, foot)
            tiers = 3 if big else 2
            inside = set()
            for t in range(tiers):
                y0 = top + t * (h - 3) // tiers
                y1 = y0 + (h - 3) // tiers + 3
                for y in range(y0, min(y1, foot - 2)):
                    half = 1 + (y - y0) * (6.5 if big else 4.5) / max(y1 - y0, 1) + t * 0.6
                    for x in range(int(8 - half), int(8 + half + 0.99)):
                        inside.add((x, y))
                        side = (x + 0.5 - 8) / max(half, 1)
                        s = 3 if side < -0.35 else (2 if side < 0.25 else 1)
                        if y == y1 - 1 or y == min(y1, foot - 2) - 1:
                            s = max(0, s - 1)
                        if rng.random() < 0.08:
                            s = min(4, s + 1)
                        body.set(x, y, hexc(leaf[s]))
                # jagged lower edge
                for x in range(16):
                    if (x, y1 - 2) in inside and rng.random() < 0.3:
                        body.set(x, y1 - 1, CLEAR)
            body.set(8, top - 1, hexc(leaf[3]))
        else:
            r = (6.0 if kind == "oak" else 5.0) * (1.0 if big else 0.7)
            cy = foot - 5 - r * (1.05 if big else 0.95)
            if kind == "birch":
                trunk(body, bark, 7, int(cy), foot)
                for y in range(int(cy) + 2, foot, 3):
                    body.set(7 + (y % 2), y, hexc(bark[0]))
            else:
                trunk(body, bark, 7, int(cy + 2), foot)
                body.set(6, foot, hexc(bark[0]))
                body.set(9, foot, hexc(bark[0]))
            blobs = [(8, cy, r)]
            for _ in range(5 if big else 3):
                a = rng.uniform(0, math.tau)
                d = rng.uniform(0.4, 0.75) * r
                blobs.append((8 + math.cos(a) * d, cy + math.sin(a) * d * 0.8, r * rng.uniform(0.45, 0.65)))
            canopy(body, blobs, leaf, rng, 8, cy, r)
    elif stage == 4:  # sapling
        trunk(body, bark, 8, foot - 7, foot, w=1)
        canopy(body, [(8.5, foot - 9, 3.0), (7, foot - 7.5, 1.8), (10, foot - 7.5, 1.8)], leaf, rng, 8.5, foot - 9, 3)
    elif stage in (5, 6):  # a stump, with a shoot from it
        wood = TREE_KINDS["oak"]["bark"] if kind != "birch" else bark
        for y in range(foot - 2, foot + 1):
            for x in range(6, 10):
                body.set(x, y, hexc(wood[2] if x == 6 else (wood[1] if x < 9 else wood[0])))
        ring = hexc("C8A070") if kind != "birch" else hexc("E8DCC0")
        for x in range(6, 10):
            body.set(x, foot - 3, ring)
        body.set(7, foot - 3, hexc("A07850"))
        body.set(8, foot - 3, hexc("D8B488"))
        if stage == 5:
            for x, y, s in ((8, foot - 4, 2), (8, foot - 5, 3), (9, foot - 6, 3), (7, foot - 6, 4), (10, foot - 6, 2)):
                body.set(x, y, hexc(leaf[s]))
        else:
            c.set(4, foot + 1, hexc("C8A070"))
            c.set(11, foot, hexc("B08850"))
    body.outline(INK)
    w = {0: 5.5, 3: 4.0, 4: 2.5, 5: 2.5, 6: 2.5}[stage]
    c.ellipse(8, foot + 1.3, w, 1.4, SHADOW)
    c.paste(body, 0, 0)
    return c


def trees():
    frames = []
    for n, kind in enumerate(("oak", "pine", "birch")):
        full = tree_frame(kind, 0, 11 + n)
        frames.append(full)
        rows = set(range(0, 16 if kind != "birch" else 15))
        frames.append(full.shifted(-1, 0, rows))
        frames.append(full.shifted(1, 0, rows))
        for stage in (3, 4, 5, 6):
            frames.append(tree_frame(kind, stage, 31 + n * 7 + stage))
    save("trees.png", sheet(frames, 7))
    write(
        "trees.toml",
        """# The valley's trees, drawn by tools/art.py: frames of 16 x 24 px, a row
# a kind (oak, pine, birch) of 7: 0 standing, 1-2 swaying under the axe,
# 3 young, 4 a sapling, 5 a shoot from a stump, 6 a stump. The trunk's
# foot is on the pivot. systems/art.luau picks the frame by Tree.growth.
grid = [7, 3]
pivot = [0.5, 0.125]
""",
    )


# --- Items -----------------------------------------------------------------------

SHEAF = ["...g.g..", "..gGgGg.", "..GgGgG.", "...gGg..", "...bBb..", "...gGg..", "..gG.Gg.", "........"]
LOGS = ["........", "........", "..LLLLLe", ".LLLLLLE", "LLLLLeLL", "LLLLLELL", ".lllll..", "........"]
STONE = ["........", "........", "...ss...", "..sSSs..", ".sSSSsS.", ".ssSsss.", "........", "........"]
ITEM_PAL = {
    "g": hexc("E6C65E"),
    "G": hexc("B8922E"),
    "b": hexc("8A5A2A"),
    "B": hexc("6A4220"),
    "L": hexc("8A5E36"),
    "l": hexc("5E3E22"),
    "e": hexc("D8B07A"),
    "E": hexc("B88A58"),
    "s": hexc("8E9098"),
    "S": hexc("B4B6BE"),
}


def item_canvas(rows, w, h, oy, shadow=True):
    c = Canvas(w, h)
    art = Canvas(w, h)
    art.grid(rows, ITEM_PAL, 0, oy)
    art.outline(INK)
    if shadow:
        c.ellipse(4, oy + 6.8, 3.2, 1.1, SHADOW)
    c.paste(art, 0, 0)
    return c


def items():
    s = Canvas(48, 20)
    for i, rows in enumerate((SHEAF, LOGS, STONE)):
        s.paste(item_canvas(rows, 8, 8, 0), i * 8, 0)
        s.paste(item_canvas(rows, 8, 20, 0, shadow=False), 24 + i * 8, 0)
    save("items.png", s)
    write(
        "items.toml",
        """# Goods, drawn by tools/art.py: 0 crops, 1 logs, 2 stone, lying on the
# ground (8 x 8 px); 3, 4, 5 the same carried overhead (8 x 20 px, the load
# at the top, so it rides above its carrier's hands).
[[frames]]
rect = [0, 0, 8, 8]
pivot = [0.5, 0.3]

[[frames]]
rect = [8, 0, 8, 8]
pivot = [0.5, 0.3]

[[frames]]
rect = [16, 0, 8, 8]
pivot = [0.5, 0.3]

[[frames]]
rect = [24, 0, 8, 20]
pivot = [0.5, 0.0]

[[frames]]
rect = [32, 0, 8, 20]
pivot = [0.5, 0.0]

[[frames]]
rect = [40, 0, 8, 20]
pivot = [0.5, 0.0]
""",
    )


# --- Fields ----------------------------------------------------------------------
#
# Frames of 8 x 12: the soil is the bottom 8 x 8 (one cell; fields side by
# side make one plot), the crop grows up out of it.

SOIL = [hexc("4A3120"), hexc("5C3D27"), hexc("6E4A2E"), hexc("7E5734")]


def field_frame(stage, seed):
    rng = random.Random(seed)
    c = Canvas(8, 12)
    for y in range(4, 12):
        for x in range(8):
            r = (y - 4) % 4
            k = [2, 1, 0, 1][r]
            if rng.random() < 0.12:
                k = min(3, k + 1)
            c.set(x, y, SOIL[k])
    if stage == 0:
        for x in (1, 4, 6):
            c.set(x, 5 + (x % 2) * 4, SOIL[3])
        return c
    green, green2, tip = hexc("5E9A3A"), hexc("3F7428"), hexc("8CC452")
    gold, gold2, head = hexc("D8B44E"), hexc("A8862E"), hexc("F2DA7A")
    tall = {1: 1, 2: 2, 3: 4, 4: 6, 5: 7, 6: 7}[stage]
    for row_base, xs in ((7, (0, 2, 4, 6)), (11, (1, 3, 5, 7))):
        for x in xs:
            cut = stage == 6 and (x + row_base) % 3 != 0
            h = 1 if cut else tall
            for i in range(h):
                y = row_base - i
                if stage <= 3:
                    col = tip if i == h - 1 else (green if i % 2 == 0 else green2)
                elif stage == 4:
                    col = hexc("B4B84C") if i >= h - 2 else (hexc("8AA840") if i % 2 == 0 else green2)
                else:
                    col = gold2 if cut else (head if i >= h - 2 else (gold if i % 2 == 0 else gold2))
                c.set(x, y, col)
            if stage >= 3 and not cut:
                c.set(x + 1, row_base - h // 2, green2 if stage == 3 else gold2)
    return c


def fields():
    frames = [field_frame(s, 50 + s) for s in range(7)]
    save("field.png", sheet(frames, 7))
    write(
        "field.toml",
        """# A field, drawn by tools/art.py: frames of 8 x 12 px, the soil the bottom
# 8 x 8 (one cell). 0 tilled, 1 seedlings, 2 sprouts, 3 green, 4 ripening,
# 5 ripe, 6 half cut (being harvested). systems/art.luau picks it by
# Field.growth.
grid = [7, 1]
pivot = [0.5, 0.3333]
""",
    )


# --- Beds --------------------------------------------------------------------------

WOOD = [hexc("4E3220"), hexc("6E4A30"), hexc("8E6640"), hexc("B08452")]


def bed_frame(look, asleep):
    """A cot seen from above, its head to the north: a wooden frame, a
    pillow, a sheet, and its owner's quilt (in their shirt's colour, so a
    cot reads as someone's), folded at the foot or pulled up over them."""
    c = Canvas(8, 12)
    c.ellipse(4, 10.9, 3.9, 1.2, SHADOW)
    b = Canvas(8, 12)
    for y in range(1, 11):
        for x in range(0, 8):
            b.set(x, y, WOOD[1])
    for x in range(0, 8):
        b.set(x, 1, WOOD[3])
        b.set(x, 10, WOOD[0])
    for y in range(1, 11):
        b.set(0, y, WOOD[2])
        b.set(7, y, WOOD[0])
    for y in range(2, 10):
        for x in range(1, 7):
            b.set(x, y, hexc("E8E0CC") if y < 4 else hexc("D2C8B2"))
    for x in range(2, 6):
        b.set(x, 2, hexc("F6F0E2"))
        b.set(x, 3, hexc("E2D8C2"))
    q1, q2 = (hexc(look[0]), hexc(look[1])) if look else (hexc("7A8494"), hexc("5C6574"))
    top = 4 if asleep else 7
    for y in range(top, 10):
        for x in range(1, 7):
            b.set(x, y, q2 if (x == 6 or y == 9) else q1)
    for x in range(1, 7):
        b.set(x, top, mix(q1, (255, 255, 255, 255), 0.35))
    if asleep:
        hair, hair2, skin = hexc(look[2]), hexc(look[3]), hexc(look[4])
        for x in range(2, 6):
            b.set(x, 2, hair)
        b.set(2, 3, hair2)
        b.set(5, 3, hair2)
        b.set(3, 3, skin)
        b.set(4, 3, skin)
        # the shape under the quilt
        for y in range(5, 9):
            b.set(4, y, q2)
    b.outline(INK)
    c.paste(b, 0, 0)
    return c


def beds():
    frames = [bed_frame(look, False) for look in LOOKS]
    frames += [bed_frame(look, True) for look in LOOKS]
    frames.append(bed_frame(None, False))
    save("bed.png", sheet(frames, 13))
    write(
        "bed.toml",
        """# A cot, drawn by tools/art.py: 8 x 12 px frames, the quilt in its
# owner's colour. 0-5 empty, its quilt folded (Ada, Bram, Cleo, Dov, Esme,
# Finn); 6-11 its owner asleep in it; 12 empty, nobody's.
grid = [13, 1]
""",
    )


# --- The stockpile -------------------------------------------------------------------
#
# 24 x 28 px: a plank deck of 3 x 3 cells (the bottom 24 rows) with what it
# holds stacked on it. Frame wood * 16 + crops * 4 + meals, each 0 to 3.

PLANK = [hexc("5E5446"), hexc("7E725E"), hexc("968A74"), hexc("ACA08A")]


def deck():
    c = Canvas(24, 28)
    rng = random.Random(7)
    c.ellipse(12, 26.5, 12.5, 2, SHADOW)
    for y in range(5, 26):
        for x in range(1, 23):
            seam = (y - 5) % 4 == 3
            k = 0 if seam else (2 if (y - 5) % 4 == 0 else 1)
            if not seam and rng.random() < 0.06:
                k = 3
            c.set(x, y, PLANK[k])
        if (y - 5) % 4 != 3:
            gap = 5 + ((y - 5) // 4 * 7) % 13
            c.set(gap, y, PLANK[0])
    for x in range(1, 23):
        c.set(x, 25, PLANK[0])
        c.set(x, 26, hexc("3A3228"))
    for px, py in ((0, 3), (22, 3), (0, 23), (22, 23)):
        for y in range(py, py + 4):
            c.set(px, y, WOOD[2])
            c.set(px + 1, y, WOOD[0])
        c.set(px, py, WOOD[3])
    c.outline(INK)
    return c


def log(c, x, y, n=8):
    """A log lying east-west, its cut end to the east: n x 3 px."""
    for i in range(n - 2):
        c.set(x + i, y, hexc("A86E3C"))
        c.set(x + i, y + 1, hexc("84542C"))
        c.set(x + i, y + 2, hexc("5C381C"))
    for dy, row in enumerate(("eE", "Ec", "eE")):
        for dx, ch in enumerate(row):
            c.set(x + n - 2 + dx, y + dy, {"e": hexc("E8C890"), "E": hexc("C8A068"), "c": hexc("8A6038")}[ch])


def sheaf(c, x, y):
    for dy, row in enumerate([".g.g.", "gGgGg", "gGgGg", ".rRr.", ".gGg.", "gG.Gg", "g...g"]):
        for dx, ch in enumerate(row):
            col = {"g": hexc("F2D66A"), "G": hexc("C49A34"), "r": hexc("A8402C"), "R": hexc("7A2C1E")}.get(ch)
            if col:
                c.set(x + dx, y + dy, col)


def basket(c, x, y):
    """A basket of loaves: 7 x 5 px."""
    for dy in range(2, 5):
        for dx in range(7):
            weave = (dx + dy) % 2 == 0
            col = hexc("C08A4A") if weave else hexc("8E5E2E")
            if dy == 4:
                col = hexc("6A4422")
            c.set(x + dx, y + dy, col)
    for dx, col in ((1, "E8A04C"), (2, "F6C878"), (3, "E8A04C"), (4, "F6C878"), (5, "D08840")):
        c.set(x + dx, y + 1, hexc(col))
    for dx, col in ((2, "FAD898"), (4, "FAD898")):
        c.set(x + dx, y, hexc(col))


def stockpile():
    base = deck()
    wood_at = [
        [],
        [(2, 13), (3, 10)],
        [(2, 14), (2, 11), (6, 14), (4, 8)],
        [(1, 15), (1, 12), (5, 15), (5, 12), (2, 9), (6, 9), (4, 6)],
    ]
    crop_at = [
        [],
        [(15, 8)],
        [(13, 8), (18, 8), (15, 11)],
        [(12, 7), (17, 7), (14, 10), (19, 10), (12, 13), (17, 13)],
    ]
    meal_at = [[], [(9, 18)], [(5, 18), (13, 18)], [(2, 18), (9, 18), (16, 18), (13, 15)]]
    frames = []
    for w in range(4):
        for cr in range(4):
            for m in range(4):
                c = Canvas(24, 28)
                c.paste(base, 0, 0)
                goods = Canvas(24, 28)
                for x, y in wood_at[w]:
                    log(goods, x, y)
                for x, y in crop_at[cr]:
                    sheaf(goods, x, y)
                for x, y in meal_at[m]:
                    basket(goods, x, y)
                goods.outline(INK)
                c.paste(goods, 0, 0)
                frames.append(c)
    save("stockpile.png", sheet(frames, 8))
    write(
        "stockpile.toml",
        """# The stockpile, drawn by tools/art.py: a plank deck of 3 x 3 cells with
# what it holds on it, 24 x 28 px. Frame wood * 16 + crops * 4 + meals,
# each 0 (none) to 3 (plenty); systems/art.luau reads Stockpile.wood, crop,
# and meal.
grid = [8, 8]
pivot = [0.5, 0.4286]
""",
    )


# --- The stove, the campfire, a grave ----------------------------------------------

STONE_C = [hexc("4C4A50"), hexc("66646A"), hexc("85838A"), hexc("A4A2A8")]
FIRE = [hexc("A8301E"), hexc("E0562A"), hexc("FF9238"), hexc("FFD25A"), hexc("FFF4C0")]


def flame(c, cx, base, h, rng, w=2.2):
    for y in range(base - h, base + 1):
        k = (base - y) / max(h, 1)
        half = w * (1 - k * 0.8) + rng.uniform(-0.3, 0.3)
        for x in range(int(cx - half - 0.5), int(cx + half + 0.5) + 1):
            d = abs(x + 0.5 - cx) / max(half, 0.5)
            lvl = 4 - int(d * 2.2 + k * 2.2)
            c.set(x, y, FIRE[max(0, min(4, lvl))])


def stove_frame(state, seed):
    rng = random.Random(seed)
    c = Canvas(12, 20)
    c.ellipse(6, 17.3, 5.5, 1.5, SHADOW)
    b = Canvas(12, 20)
    # chimney at the back
    for y in range(1, 8):
        for x in (8, 9):
            b.set(x, y, STONE_C[2] if x == 8 else STONE_C[1])
    b.set(8, 1, STONE_C[3])
    b.set(9, 1, STONE_C[2])
    # the oven's dome
    b.ellipse(6, 10.5, 5.2, 4.6, STONE_C[1])
    b.ellipse(5.2, 9.6, 3.6, 3.0, STONE_C[2])
    b.ellipse(4.6, 8.8, 1.6, 1.2, STONE_C[3])
    for y in range(11, 17):
        for x in range(1, 11):
            b.set(x, y, STONE_C[1] if (x + y) % 5 else STONE_C[0])
    for x in range(1, 11):
        b.set(x, 16, STONE_C[0])
    # mouth
    for y in range(11, 16):
        for x in range(4, 8):
            if y > 11 or x in (5, 6):
                b.set(x, y, hexc("1A1210"))
    if state > 0:
        flame(b, 6.0, 15, 3 if state in (1, 3) else 2, rng, 1.6)
        for x in range(4, 8):
            b.set(x, 15, FIRE[3])
    # a pot on top while cooking
    if state >= 3:
        for x in range(3, 7):
            b.set(x, 5, hexc("2A2A30"))
            b.set(x, 6, hexc("3A3A42"))
        b.set(3, 4, hexc("50505A"))
        b.set(6, 4, hexc("50505A"))
        b.set(4, 4, hexc("E8E0D0", 200) if state == 3 else hexc("50505A"))
        b.set(5, 3 if state == 3 else 4, hexc("F0ECE4", 160))
    b.outline(INK)
    c.paste(b, 0, 0)
    return c


def stove():
    frames = [stove_frame(s, 70 + s) for s in range(5)]
    save("stove.png", sheet(frames, 5))
    write(
        "stove.toml",
        """# The stove, drawn by tools/art.py: a stone oven with a chimney, 12 x 20
# px. 0 cold, 1-2 lit, 3-4 cooking (a pot on top). Its foot is on the pivot.
grid = [5, 1]
pivot = [0.5, 0.15]
""",
    )


def campfire_frame(state, seed):
    rng = random.Random(seed)
    c = Canvas(12, 14)
    c.ellipse(6, 11, 5.8, 2.4, SHADOW)
    b = Canvas(12, 14)
    for i in range(10):
        a = i / 10 * math.tau
        x, y = 6 + math.cos(a) * 4.4, 10.2 + math.sin(a) * 2.2
        b.set(int(x), int(y), STONE_C[2 if math.sin(a) < 0 else 1])
        b.set(int(x) + 1, int(y), STONE_C[3 if math.sin(a) < 0 else 2])
    # logs crossed
    for i in range(6):
        b.set(3 + i, 11 - i // 2, hexc("6A4428"))
        b.set(8 - i, 11 - i // 2, hexc("8E6038"))
    if state == 0:
        for x in (5, 6, 7):
            b.set(x, 10, hexc("6A6460"))
        b.set(6, 9, hexc("8A8480"))
    else:
        flame(b, 6.0, 10, 6 + (state % 2), rng, 2.4)
        flame(b, 4.2, 10, 3, rng, 1.0)
        flame(b, 7.8, 10, 3 + (state == 2), rng, 1.0)
    b.outline(INK)
    c.paste(b, 0, 0)
    return c


def campfire():
    frames = [campfire_frame(s, 80 + s) for s in range(4)]
    save("campfire.png", sheet(frames, 4))
    write(
        "campfire.toml",
        """# The campfire, drawn by tools/art.py: a ring of stones round crossed
# logs, 12 x 14 px. 0 out (by day), 1-3 burning.
grid = [4, 1]
pivot = [0.5, 0.2857]
""",
    )


def grave():
    c = Canvas(8, 12)
    c.ellipse(4, 10, 3.8, 1.6, SHADOW)
    b = Canvas(8, 12)
    for y in range(7, 10):
        for x in range(1, 7):
            if not (y == 7 and x in (1, 6)):
                b.set(x, y, hexc("6E4A2E") if y > 7 else hexc("8A6440"))
    for y in range(1, 9):
        b.set(4, y, hexc("8E6640"))
        b.set(3, y, hexc("B08452") if y < 8 else hexc("8E6640"))
    for x in range(1, 7):
        b.set(x, 3, hexc("B08452") if x < 6 else hexc("8E6640"))
    b.set(6, 8, hexc("F0E070"))
    b.set(1, 9, hexc("E07A9A"))
    b.outline(INK)
    c.paste(b, 0, 0)
    save("grave.png", c)
    write(
        "grave.toml",
        """# A grave, drawn by tools/art.py: a wooden cross on a mound, 8 x 12 px.
pivot = [0.5, 0.25]
""",
    )


# --- The ground --------------------------------------------------------------------
#
# ground/valley: the valley's cells (x 0 to 39, y 0 to 23, a cell centred on
# whole numbers) and 8 cells of forest round them, 56 x 40 cells, image row
# 0 at the top (y 31.5). The places are found.luau's: the stockpile (20, 12),
# the stove (24, 12), the fields (5-9, 6-7), the beds (15-20, 18), the
# campfire (17.5, 15.5). Trees stand only outside x 3-27, y 3-19.

GW, GH = 40, 24
MARGIN = 8
GRASS = [hexc("3F6E30"), hexc("487A36"), hexc("51853B"), hexc("5C9142"), hexc("6CA04B"), hexc("80B456")]
FLOWERS = [hexc("F2D45A"), hexc("F4EEDC"), hexc("B48CD8"), hexc("E8786A")]
DIRT = [hexc("6A4A2E"), hexc("806040"), hexc("98744C"), hexc("AE8A5C"), hexc("C0A070")]
FOREST = [hexc("15281A"), hexc("1C3420"), hexc("244226"), hexc("2E522C"), hexc("3C6434")]
WATER = [hexc("2C5A76"), hexc("3A7290"), hexc("4E8AA8"), hexc("8CC4DA")]


def world_px(x, y):
    """A world point to the valley image's pixel (fractional)."""
    return (x + 0.5 + MARGIN) * PPU, (GH - 0.5 - y + MARGIN) * PPU


def value_noise(w, h, cell, seed):
    rng = random.Random(seed)
    gw, gh = w // cell + 2, h // cell + 2
    g = [[rng.random() for _ in range(gw)] for _ in range(gh)]

    def at(x, y):
        fx, fy = x / cell, y / cell
        ix, iy = int(fx), int(fy)
        tx, ty = fx - ix, fy - iy
        tx, ty = tx * tx * (3 - 2 * tx), ty * ty * (3 - 2 * ty)
        a = g[iy][ix] + (g[iy][ix + 1] - g[iy][ix]) * tx
        b = g[iy + 1][ix] + (g[iy + 1][ix + 1] - g[iy + 1][ix]) * tx
        return a + (b - a) * ty

    return at


def seg_dist(px, py, ax, ay, bx, by):
    dx, dy = bx - ax, by - ay
    t = max(0, min(1, ((px - ax) * dx + (py - ay) * dy) / max(dx * dx + dy * dy, 1e-9)))
    qx, qy = ax + dx * t, ay + dy * t
    return math.hypot(px - qx, py - qy)


def path_dist(px, py, pts):
    return min(seg_dist(px, py, *pts[i], *pts[i + 1]) for i in range(len(pts) - 1))


def grass_tone(n1, n2, rng):
    v = n1 * 0.7 + n2 * 0.3 + rng.uniform(-0.13, 0.13)
    return GRASS[max(0, min(4, int(v * 5)))]


def valley_ground():
    W, H = (GW + 2 * MARGIN) * PPU, (GH + 2 * MARGIN) * PPU
    c = Canvas(W, H)
    rng = random.Random(2026)
    n1 = value_noise(W, H, 24, 1)
    n2 = value_noise(W, H, 6, 2)
    n3 = value_noise(W, H, 40, 3)
    # Paths in world units, as polylines.
    paths = [
        [(18.0, 12.0), (14.0, 10.0), (11.0, 8.4), (9.8, 7.6)],  # the yard to the fields
        [(21.5, 12.0), (24.0, 12.0)],  # to the stove
        [(19.5, 13.0), (18.0, 15.0), (17.5, 17.0)],  # to the campfire and the bunks
        [(25.0, 12.0), (30.0, 11.5), (36.0, 12.5), (41.0, 12.0)],  # east, to the woods
        [(14.0, 10.0), (8.0, 11.0), (2.0, 10.5), (-3.0, 11.0)],  # west, to the stream
        [(11.0, 14.0), (15.0, 13.0), (18.0, 12.5)],  # where they stand at first
    ]
    widths = [0.75, 0.9, 0.7, 0.55, 0.55, 0.55]
    yard = (21.5, 12.0, 4.6, 3.0)  # an oval of trodden earth round the stockpile and stove
    hearth = (17.5, 15.5, 1.9)
    plot = (4.5, 5.5, 9.5, 7.5)  # the fields' cells' edges
    deck_r = (14.5, 17.45, 20.5, 18.55)  # under the beds
    for iy in range(H):
        wy = GH - 0.5 + MARGIN - (iy + 0.5) / PPU
        for ix in range(W):
            wx = (ix + 0.5) / PPU - 0.5 - MARGIN
            a, b = n1(ix, iy), n2(ix, iy)
            # How far outside the valley (cells); the forest past its edge.
            ox = max(-0.5 - wx, wx - (GW - 0.5), 0)
            oy = max(-0.5 - wy, wy - (GH - 0.5), 0)
            out = math.hypot(ox, oy)
            stream_x = -3.2 + math.sin(wy * 0.35) * 0.9 + math.sin(wy * 0.9 + 1) * 0.25
            sd = abs(wx - stream_x)
            edge = out - (n3(ix, iy) - 0.5) * 2.4
            if sd < 0.85 and wx < 0:
                k = 0 if sd > 0.65 else (1 if sd > 0.35 else 2)
                if rng.random() < 0.05:
                    k = 3
                col = WATER[k]
            elif sd < 1.2 and wx < 0:
                col = DIRT[1] if rng.random() < 0.6 else DIRT[2]
            elif edge > 1.4 and not (wx < 0 and sd < 2.2 and out < 3):
                v = a * 0.6 + b * 0.4 + rng.uniform(-0.1, 0.1)
                col = FOREST[max(0, min(4, int(v * 5)))]
            else:
                col = grass_tone(a, b, rng)
                if edge > 0.3:
                    col = mix(col, FOREST[2], min(0.7, (edge - 0.3) * 0.6))
                pd = min(path_dist(wx, wy, p) - w for p, w in zip(paths, widths))
                yd = math.hypot((wx - yard[0]) / yard[2], (wy - yard[1]) / yard[3])
                hd = math.hypot(wx - hearth[0], wy - hearth[1]) / hearth[2]
                rough = (b - 0.5) * 0.35
                if plot[0] - 0.12 <= wx <= plot[2] + 0.12 and plot[1] - 0.12 <= wy <= plot[3] + 0.12:
                    col = SOIL[0]
                elif deck_r[0] <= wx <= deck_r[2] and deck_r[1] <= wy <= deck_r[3]:
                    row = int((deck_r[3] - wy) * PPU)
                    col = PLANK[0] if row % 3 == 2 else PLANK[1 + (int(wx * 2) % 2)]
                elif yd + rough < 1.0 or hd + rough < 1.0 or pd + rough * 0.6 < 0:
                    inner = min(1.0 - yd, 1.0 - hd, -pd)
                    k = 2 if inner > 0.12 else 1
                    if rng.random() < 0.1:
                        k = 3 if rng.random() < 0.6 else 0
                    col = DIRT[k]
                elif yd + rough < 1.12 or hd + rough < 1.12 or pd + rough * 0.6 < 0.12:
                    col = mix(col, DIRT[1], 0.55)
            c.px[iy][ix] = col
    # Blades, tufts, flowers, and stones on the grass.
    for _ in range(W * H // 14):
        ix, iy = rng.randrange(W), rng.randrange(1, H - 1)
        here = c.px[iy][ix]
        if here in GRASS[:5]:
            k = GRASS.index(here)
            c.set(ix, iy - 1, GRASS[min(5, k + 2)])
            c.set(ix, iy, GRASS[min(5, k + 1)])
            c.set(ix, iy + 1, GRASS[max(0, k - 1)])
    for _ in range(W * H // 55):
        ix, iy = rng.randrange(W), rng.randrange(H - 1)
        here = c.px[iy][ix]
        if here in GRASS:
            if rng.random() < 0.82:
                c.set(ix, iy, GRASS[5] if rng.random() < 0.5 else GRASS[4])
                if rng.random() < 0.5:
                    c.set(ix + 1, iy + 1, GRASS[1])
            else:
                f = rng.choice(FLOWERS)
                c.set(ix, iy, f)
    for _ in range(W * H // 900):
        ix, iy = rng.randrange(W - 2), rng.randrange(H - 2)
        if c.px[iy][ix] in GRASS or c.px[iy][ix] in DIRT:
            c.set(ix, iy, STONE_C[2])
            c.set(ix + 1, iy, STONE_C[1])
            c.set(ix, iy + 1, STONE_C[0])
    # The forest: crowns over the margin, a shaded rim where it meets the grass.
    for _ in range(W * H // 70):
        ix, iy = rng.randrange(W), rng.randrange(H)
        if c.px[iy][ix] in FOREST:
            r = rng.uniform(1.5, 3.2)
            for yy in range(int(iy - r), int(iy + r) + 1):
                for xx in range(int(ix - r), int(ix + r) + 1):
                    d = math.hypot(xx - ix, yy - iy)
                    if d <= r and 0 <= xx < W and 0 <= yy < H and c.px[yy][xx] in FOREST:
                        s = ((xx - ix) * -0.6 + (yy - iy) * -0.8) / r
                        c.set(xx, yy, FOREST[4] if s > 0.55 else (FOREST[3] if s > 0.0 else FOREST[2]))
    return c


def grass_tile():
    """A 64 x 64 tile that wraps, for a valley of any size."""
    c = Canvas(64, 64)
    rng = random.Random(5)
    for y in range(64):
        for x in range(64):
            a = 0.5 + 0.25 * math.sin(x / 64 * math.tau * 2 + 1.3) * math.cos(y / 64 * math.tau + 0.4)
            b = 0.5 + 0.25 * math.sin((x + y) / 64 * math.tau * 3)
            c.px[y][x] = grass_tone(a, b, rng)
    for _ in range(80):
        x, y = rng.randrange(64), rng.randrange(64)
        c.set(x, y, GRASS[5] if rng.random() < 0.5 else GRASS[4])
        if rng.random() < 0.2:
            c.set(x, y, rng.choice(FLOWERS))
    return c


def ground():
    save("ground/valley.png", valley_ground())
    write(
        "ground/valley.toml",
        """# The valley's ground, drawn by tools/art.py: its 40 x 24 cells and 8
# cells of forest round them, 8 px a cell. One frame, pivot in the middle.
""",
    )
    save("ground/grass.png", grass_tile())
    write(
        "ground/grass.toml",
        """# A tile of grass that wraps (64 x 64 px, 8 cells), for a valley that is
# not the 40 x 24 one: materials/ground repeats it across the ground.
""",
    )


# --- UI ------------------------------------------------------------------------------


def panel_image(alpha, size=16):
    c = Canvas(size, size)
    edge, light, inner, stud = hexc("120C08"), hexc("A07A4C"), hexc("231A13", alpha), hexc("E0BC7A")
    for y in range(size):
        for x in range(size):
            corner = (x in (0, size - 1)) and (y in (0, size - 1))
            if corner:
                continue
            if x in (0, size - 1) or y in (0, size - 1):
                c.set(x, y, edge)
            elif x in (1, size - 2) or y in (1, size - 2):
                c.set(x, y, light if (x == 1 or y == 1) else hexc("6E5232"))
            else:
                c.set(x, y, inner)
    for x, y in ((2, 2), (size - 3, 2), (2, size - 3), (size - 3, size - 3)):
        c.set(x, y, stud)
    return c


def button_image():
    c = Canvas(12, 12)
    for y in range(12):
        for x in range(12):
            if (x in (0, 11)) and (y in (0, 11)):
                continue
            if x in (0, 11) or y in (0, 11):
                c.set(x, y, hexc("5A5A5A"))
            elif y == 1:
                c.set(x, y, hexc("FFFFFF"))
            elif y == 10:
                c.set(x, y, hexc("A8A8A8"))
            else:
                c.set(x, y, hexc("E2E2E2"))
    return c


def ui():
    save("ui/panel.png", panel_image(238))
    save("ui/hud.png", panel_image(200))
    save("ui/button.png", button_image())


# --- Effects and materials -------------------------------------------------------------


def fx_and_materials():
    write(
        "fx/smoke.toml",
        """# Smoke from the campfire and the stove (systems/art.luau plays it while
# they burn): it starts clear, so it is seen first above the fire, then
# drifts up and to the east and thins away.
type = "emitter"
particle = "circle"
shape = "circle"
radius = 0.15
rate = 5
max = 200
lifetime = [2.2, 3.2]
speed = [1.1, 1.5]
direction = 90
spread = 25
gravity = [0.25, 0.1]
drag = 0.9
size = [[0, 0.2], [0.4, 0.5], [1, 0.9]]
color = [[0, "#FFB04800"], [0.08, "#FFA04070"], [0.22, "#8E8A8460"], [1, "#B8B4AE00"]]
z = 6
""",
    )
    write(
        "fx/bits.toml",
        """# Chips from a chopped tree and chaff from a harvest: systems/art.luau
# bursts it where the work is (particles.burst with a pos).
type = "emitter"
particle = "rect"
shape = "circle"
radius = 0.2
rate = 0
max = 400
lifetime = [0.35, 0.7]
speed = [1.6, 3.2]
direction = 90
spread = 150
gravity = [0, -9]
drag = 1.2
size = [[0, 0.16], [1, 0.1]]
color = [[0, "#E2BE7CFF"], [0.7, "#C89A5AE0"], [1, "#A0784800"]]
z = 6
""",
    )
    write(
        "materials/art.wgsl",
        """// Outpost's art: the sprite's frame as it is drawn, without the tint.
// The colony's systems still give its entities their old shapes' colours
// (a tree's colour says standing or stump); the art, not the colour, is what
// is seen.
fn material(in: MaterialInput, p: Params) -> vec4<f32> {
    return sample_atlas(in, in.uv);
}
""",
    )
    write(
        "materials/art.toml",
        """type = "material"
doc = "A sprite's frame without its tint: Outpost's art over the colony's colours."
""",
    )
    write(
        "materials/ground.wgsl",
        """// Grass repeated across a valley of any size: the tile once every
// p.cells world units, however big the sprite is.
fn material(in: MaterialInput, p: Params) -> vec4<f32> {
    let at = in.local * p.size / p.cells;
    return sample_tile(vec2<f32>(at.x, 1.0 - at.y));
}
""",
    )
    write(
        "materials/ground.toml",
        """type = "material"
doc = "ground/grass repeated across the ground, one tile every cells world units."

[params.tile]
type = "texture"
default = "ground/grass"
doc = "The tile"

[params.size]
type = "vec2"
default = [40, 24]
from = "Sprite.size"
doc = "The ground's size in world units"

[params.cells]
type = "f32"
default = 8
range = [1, 64]
doc = "World units a tile covers"
""",
    )


# --- Previews --------------------------------------------------------------------------


def previews(out, scale=5):
    os.makedirs(out, exist_ok=True)
    bg = hexc("51853B")
    for path, cv in PREVIEWS:
        k = max(1, min(scale, 1500 // cv.w))
        w, h = cv.w * k, cv.h * k
        rows = []
        for y in range(h):
            row = []
            for x in range(w):
                c = cv.px[y // k][x // k]
                a = c[3] / 255
                checker = bg if ((x // (k * 4) + y // (k * 4)) % 2 == 0) else mix(bg, (0, 0, 0, 255), 0.15)
                row.append(tuple(int(c[i] * a + checker[i] * (1 - a)) for i in range(3)) + (255,))
            rows.append(row)
        name = path.replace("/", "_")
        with open(os.path.join(out, name), "wb") as f:
            f.write(png_bytes(w, h, rows))


if __name__ == "__main__":
    colonists()
    trees()
    items()
    fields()
    beds()
    stockpile()
    stove()
    campfire()
    grave()
    ground()
    ui()
    fx_and_materials()
    total = sum(n for _, n in WRITTEN)
    for path, n in WRITTEN:
        print(f"{n:8d}  {path}")
    print(f"{total:8d}  in all ({len(WRITTEN)} files)")
    if "--preview" in sys.argv:
        previews(sys.argv[sys.argv.index("--preview") + 1])
