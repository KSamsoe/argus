"""The vigil's tiles (the lanterns mod, phase 3 item 8).

A closed room, 24 x 14, to the right of the hall with a gap between them:
the door in the hall (scenes/hollow/rooms/c/lanterns.door.toml) is the
way in. Written in argus-tiles' format as Hollow's tools/deep.py writes
the deep's rooms, so running it again gives the same files:

    python tools/vigil.py          (from mods/lanterns)

A mod's paths are spelled from the game's root, so the room's tilemap
entity names mods/lanterns/scenes/... A mod's tools/ is not in its pack.
"""

import os
import struct

HERE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
ROOM = os.path.join(HERE, "scenes", "hollow", "rooms", "lanterns-vigil")
X0, Y0 = 80, 0
W, H = 24, 14
CHUNK = 32


def fnv1a64(data):
    h = 0xCBF29CE484222325
    for b in data:
        h ^= b
        h = (h * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h


def main():
    back = [[0] * W for _ in range(H)]
    ground = [[0] * W for _ in range(H)]
    for y in range(H):
        for x in range(W):
            if x in (0, W - 1) or y in (0, H - 1):
                ground[y][x] = 1
            else:
                back[y][x] = 1
    # A ledge the keeper stands below, and a step up to it.
    for x in range(15, 22):
        ground[8][x] = 1
    layers = [
        ("back", "tiles/back", -2.0, False, back),
        ("ground", "tiles/rock", -1.0, True, ground),
    ]
    chunks = os.path.join(ROOM, "tiles.chunks")
    manifest = "# argus tilemap chunks: layer\tcx\tcy\tfnv1a64\n"
    for name, _, _, _, cells in layers:
        data = bytearray(CHUNK * CHUNK * 2)
        for y in range(H):
            for x in range(W):
                struct.pack_into("<H", data, (y * CHUNK + x) * 2, cells[y][x])
        os.makedirs(os.path.join(chunks, name), exist_ok=True)
        with open(os.path.join(chunks, name, "0_0.bin"), "wb") as f:
            f.write(data)
        manifest += f"{name}\t0\t0\t{fnv1a64(data):016x}\n"
    with open(os.path.join(chunks, "manifest.txt"), "w", newline="\n") as f:
        f.write(manifest)
    index = (
        "# The vigil's tiles: this index is text, the tiles are in tiles.chunks/\n"
        "# (see argus-tiles). Written by tools/vigil.py.\n"
        'kind = "tilemap"\n'
        f"width = {W}\nheight = {H}\ntile_size = 1.0\nchunk = {CHUNK}\ncell = 2\n"
        'chunks = "tiles.chunks"\n'
        f'hash = "fnv1a64:{fnv1a64(manifest.encode()):016x}"\n'
    )
    for name, tileset, z, collision, _ in layers:
        index += (
            f'\n[[layers]]\nname = "{name}"\ntileset = "{tileset}"\n'
            f"z = {z}\ncollision = {'true' if collision else 'false'}\n"
        )
    with open(os.path.join(ROOM, "tiles.tilemap.toml"), "w", newline="\n") as f:
        f.write(index)
    rel = "mods/lanterns/scenes/hollow/rooms/lanterns-vigil/tiles.tilemap.toml"
    with open(os.path.join(ROOM, "tiles.toml"), "w", newline="\n") as f:
        f.write(
            "# The vigil's tiles; tile (0, 0) at its top-left corner. Written by\n"
            "# tools/vigil.py. A mod's paths are spelled from the game's root.\n"
            f"[Transform]\npos = [{X0:.1f}, {Y0 + H:.1f}]\n\n[Tilemap]\nmap = \"{rel}\"\n"
        )


if __name__ == "__main__":
    main()
