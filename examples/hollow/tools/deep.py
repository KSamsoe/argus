"""The deep: 197 generated rooms under and behind the cave (phase 3 item 6).

Hollow reaches 200 rooms for the phase 3 gate ("Hollow reaches 200 rooms
with the open and stream-in numbers green"); Kevin said generated rooms are
fine. This writes them, from a fixed seed, so running it again gives the
same files:

    python tools/deep.py          (from examples/hollow)

The rooms sit on a grid to the left of the cave (room a), 24 x 14 each,
row 0 level with the cave and the rest below it. A maze joins them (a
spanning tree from a seeded walk, plus a few loops): a passage sideways is
a doorway at floor level, a passage down is a hole in the floor covered by
planks (drop through them, or jump up through them) with a stack of planks
below it in the lower room. The first room opens into the cave through its
left wall, which tools/level.sh leaves open.

Each room is a directory with its room.toml (bounds, neighbours), its own
tilemap (back wall, rock, planks) and the entity that draws it, and fifty
decorations: crystals, drips, and two glowing crystals the semantic frame
describes. About 10,000 authored files: what the index is for (section 4).
Since phase 5 item 3 the hollow is dark: a glowing crystal shows at its own
colour (Emissive) and lights the rock around it in that colour (a Light,
which the room's walls stop), so the deep is a dark maze with beacons in it.

Tiles are written in argus-tiles' format (chunks of little-endian u16
cells, a manifest of FNV-1a hashes, the index's hash of the manifest); the
engine checks every hash when it reads them.
"""

import os
import random
import shutil
import struct

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
SCENE = os.path.join(ROOT, "scenes", "hollow")
ROOMS = os.path.join(SCENE, "rooms")

COLS, ROWS = 20, 10
SKIP = {(17, 9), (18, 9), (19, 9)}  # 200 - 3: with a, b, and c, 200 rooms
W, H = 24, 14
CHUNK = 32
SEED = 7
LOOPS = 20


def name(c, r):
    return f"deep-{c}-{r}"


def origin(c, r):
    """Bottom-left corner of a cell, in world units."""
    return -(c + 1) * W, -r * H


def fnv1a64(data):
    h = 0xCBF29CE484222325
    for b in data:
        h ^= b
        h = (h * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h


def maze(rng):
    cells = [(c, r) for r in range(ROWS) for c in range(COLS) if (c, r) not in SKIP]
    allowed = set(cells)

    def around(cell):
        c, r = cell
        for n in ((c + 1, r), (c - 1, r), (c, r + 1), (c, r - 1)):
            if n in allowed:
                yield n

    edges = set()
    seen = {(0, 0)}
    stack = [(0, 0)]
    while stack:
        cur = stack[-1]
        options = [n for n in around(cur) if n not in seen]
        if not options:
            stack.pop()
            continue
        n = rng.choice(options)
        edges.add(tuple(sorted((cur, n))))
        seen.add(n)
        stack.append(n)
    assert seen == allowed
    spare = sorted(
        {tuple(sorted((a, b))) for a in cells for b in around(a)} - edges
    )
    for e in rng.sample(spare, LOOPS):
        edges.add(e)
    return cells, sorted(edges)


def tiles(room_dir, open_left, open_right, open_up, open_down):
    """The room's tilemap: rows go down, row 0 at the top."""
    back = [[0] * W for _ in range(H)]
    ground = [[0] * W for _ in range(H)]
    planks = [[0] * W for _ in range(H)]
    for y in range(H):
        for x in range(W):
            if x in (0, W - 1) or y in (0, H - 1):
                ground[y][x] = 1
            else:
                back[y][x] = 1
    door_rows = range(9, 13)
    hole_cols = range(10, 14)
    if open_left:
        for y in door_rows:
            ground[y][0], back[y][0] = 0, 1
    if open_right:
        for y in door_rows:
            ground[y][W - 1], back[y][W - 1] = 0, 1
    if open_up:
        # A hole in the ceiling, and planks up to it, each a jump above the
        # one below.
        for x in hole_cols:
            ground[0][x], back[0][x] = 0, 1
            for y in (10, 7, 4, 1):
                planks[y][x] = 1
    if open_down:
        # A hole in the floor, covered by planks: stand on them, drop
        # through them, or come up through them from below.
        for x in hole_cols:
            ground[H - 1][x], back[H - 1][x] = 0, 1
            planks[H - 1][x] = 1
    layers = [
        ("back", "tiles/back", -2.0, False, back),
        ("ground", "tiles/rock", -1.0, True, ground),
        ("planks", "tiles/plank", -1.0, True, planks),
    ]
    chunks = os.path.join(room_dir, "tiles.chunks")
    manifest = "# argus tilemap chunks: layer\tcx\tcy\tfnv1a64\n"
    for lname, _, _, _, cells in layers:
        if not any(any(row) for row in cells):
            continue
        data = bytearray(CHUNK * CHUNK * 2)
        for y in range(H):
            for x in range(W):
                struct.pack_into("<H", data, (y * CHUNK + x) * 2, cells[y][x])
        os.makedirs(os.path.join(chunks, lname), exist_ok=True)
        with open(os.path.join(chunks, lname, "0_0.bin"), "wb") as f:
            f.write(data)
        manifest += f"{lname}\t0\t0\t{fnv1a64(data):016x}\n"
    with open(os.path.join(chunks, "manifest.txt"), "w", newline="\n") as f:
        f.write(manifest)
    index = (
        "# The room's tiles: this index is text, the tiles are in tiles.chunks/\n"
        "# (see argus-tiles). Generated by tools/deep.py.\n"
        'kind = "tilemap"\n'
        f"width = {W}\nheight = {H}\ntile_size = 1.0\nchunk = {CHUNK}\ncell = 2\n"
        'chunks = "tiles.chunks"\n'
        f'hash = "fnv1a64:{fnv1a64(manifest.encode()):016x}"\n'
    )
    for lname, tileset, z, collision, _ in layers:
        index += (
            f'\n[[layers]]\nname = "{lname}"\ntileset = "{tileset}"\n'
            f"z = {z}\ncollision = {'true' if collision else 'false'}\n"
        )
    with open(os.path.join(room_dir, "tiles.tilemap.toml"), "w", newline="\n") as f:
        f.write(index)


PALETTES = [
    ["#6FD3FF", "#4FA8E8", "#9FE6FF"],  # near the surface: cold blue
    ["#7FF0C8", "#4FD0A0", "#B0FFE0"],  # green
    ["#C9A0FF", "#9F70F0", "#E0C8FF"],  # violet
    ["#FF9F7F", "#F07050", "#FFC8A8"],  # deep: ember
]


def write(path, text):
    with open(path, "w", newline="\n") as f:
        f.write(text)


def decorate(rng, room_dir, room, c, r, open_up, open_down):
    x0, y0 = origin(c, r)
    palette = PALETTES[min(3, r * 4 // ROWS)]
    rel = f"scenes/hollow/rooms/{room}/tiles.tilemap.toml"
    write(
        os.path.join(room_dir, "tiles.toml"),
        f"# The room's tiles; tile (0, 0) at its top-left corner.\n"
        f"[Transform]\npos = [{x0:.1f}, {y0 + H:.1f}]\n\n[Tilemap]\nmap = \"{rel}\"\n",
    )

    def spot(lo_y, hi_y):
        # Clear of the plank stack under a hole in the ceiling.
        while True:
            x = rng.uniform(1.6, W - 1.6)
            if open_up and 9.0 <= x <= 15.0:
                continue
            return round(x0 + x, 2), round(y0 + rng.uniform(lo_y, hi_y), 2)

    for i in range(36):
        px, py = spot(1.4, H - 2.5)
        size = round(rng.uniform(0.25, 0.55), 2)
        kind = rng.choice(["circle", "rect"])
        write(
            os.path.join(room_dir, f"crystal.{i:02d}.toml"),
            f'tags = ["crystal"]\n\n[Transform]\npos = [{px}, {py}]\n\n'
            f'[Sprite]\nkind = "{kind}"\nsize = [{size}, {size}]\n'
            f'color = "{rng.choice(palette)}"\nz = -1.5\n',
        )
    for i in range(12):
        px, _ = spot(0, 0)
        length = round(rng.uniform(0.5, 1.1), 2)
        write(
            os.path.join(room_dir, f"drip.{i:02d}.toml"),
            f'tags = ["drip"]\n\n[Transform]\npos = [{px}, {round(y0 + H - 1 - length / 2, 2)}]\n\n'
            f'[Sprite]\nkind = "rect"\nsize = [0.18, {length}]\ncolor = "#5A6478"\nz = -1.5\n',
        )
    colors = {0: "blue", 1: "green", 2: "violet", 3: "red"}
    where = "by the cave" if r == 0 else f"{r} room{'s' if r > 1 else ''} down"
    for i in range(2):
        px, py = spot(2.0, H - 4.0)
        tone = colors[min(3, r * 4 // ROWS)]
        write(
            os.path.join(room_dir, f"glow.{i}.toml"),
            f'tags = ["crystal", "glow"]\n\n[Transform]\npos = [{px}, {py}]\n\n'
            f'[Sprite]\nkind = "circle"\nsize = [0.9, 0.9]\ncolor = "{palette[2]}"\nz = -1.4\n\n'
            f'[Describe]\nname = "glowing crystal"\ntext = "A crystal glows {tone} in the dark, {where}."\n\n'
            f'[Emissive]\namount = 1.0\n\n'
            f'[Light]\ncolor = "{palette[2]}"\nintensity = 0.8\nradius = 3.5\nfalloff = 1.5\n',
        )


def main():
    rng = random.Random(SEED)
    cells, edges = maze(rng)
    for d in os.listdir(ROOMS):
        if d.startswith("deep-"):
            shutil.rmtree(os.path.join(ROOMS, d))
    near = {cell: [] for cell in cells}
    for a, b in edges:
        near[a].append(b)
        near[b].append(a)
    for (c, r) in cells:
        room = name(c, r)
        room_dir = os.path.join(ROOMS, room)
        os.makedirs(room_dir)
        ns = near[(c, r)]
        # Left in the world is one column further along the grid.
        open_left = (c + 1, r) in ns
        open_right = (c - 1, r) in ns or (c, r) == (0, 0)
        open_up = (c, r - 1) in ns
        open_down = (c, r + 1) in ns
        x0, y0 = origin(c, r)
        neighbours = sorted(name(*n) for n in ns) + (["a"] if (c, r) == (0, 0) else [])
        write(
            os.path.join(room_dir, "room.toml"),
            f"# Generated by tools/deep.py: change the tool, not this file.\n"
            f"bounds = [{x0:.1f}, {y0:.1f}, {x0 + W:.1f}, {y0 + H:.1f}]\n"
            f"neighbors = [{', '.join(chr(34) + n + chr(34) for n in sorted(neighbours))}]\n",
        )
        tiles(room_dir, open_left, open_right, open_up, open_down)
        decorate(rng, room_dir, room, c, r, open_up, open_down)

    # The scene's room graph: the three first rooms, the way down from the
    # cave, and the maze.
    path = os.path.join(SCENE, "scene.toml")
    text = open(path).read()
    head = text[: text.index("[rooms.graph]")]
    graph = ["[rooms.graph]", '"a" = ["b", "deep-0-0"]', '"b" = ["a", "c"]', '"c" = ["b"]']
    graph.append("# The deep (tools/deep.py): each passage once.")
    by_room = {}
    for a, b in edges:
        by_room.setdefault(name(*a), []).append(name(*b))
    for room in sorted(by_room, key=lambda n: tuple(int(p) for p in n.split("-")[1:])):
        graph.append(f'"{room}" = [{", ".join(chr(34) + n + chr(34) for n in by_room[room])}]')
    write(path, head + "\n".join(graph) + "\n")
    # The cave's neighbours mirror the graph.
    a_room = os.path.join(ROOMS, "a", "room.toml")
    text = open(a_room).read()
    text = text.replace('neighbors = ["b"]', 'neighbors = ["b", "deep-0-0"]')
    write(a_room, text)
    files = sum(len(os.listdir(os.path.join(ROOMS, name(*cell)))) for cell in cells)
    print(f"{len(cells)} rooms, {len(edges)} passages, {files} files and folders")


main()
