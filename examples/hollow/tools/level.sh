#!/usr/bin/env bash
# Hollow's level, built through the protocol the way an agent builds it:
# create the tilemap, then paint it in one transaction. Rows count down from
# the top of the map (row 0 is y 23 to 24; row 23 is the floor, y 0 to 1).
#
#   ./tools/level.sh            (from examples/hollow, with argus on PATH)
set -euo pipefail
cd "$(dirname "$0")/.."
A="${ARGUS:-argus}"
MAP=scenes/hollow/level.tilemap.toml

$A scene tiles create "$MAP" width=72 height=24 chunk=32 \
  layers='[{name: back, tileset: tiles/back, z: -2}, {name: ground, tileset: tiles/rock, z: -1, collision: true}, {name: planks, tileset: tiles/plank, z: -1, collision: true}]'

TX=$($A edit begin agent:claude note="Hollow: three rooms, a shaft of planks, a pit" --json | sed 's/.*"tx":\([0-9]*\).*/\1/')
paint() { # layer x y w h
  $A edit tiles tx="$TX" file="$MAP" layer="$1" rect="{x: $2, y: $3, w: $4, h: $5}" tile=1 > /dev/null
}

# The back wall: the cave, the shaft, the hall, and the two doorways.
paint back 1 11 23 12
paint back 25 1 18 22
paint back 44 11 27 12
paint back 24 20 1 3
paint back 43 20 1 3

# Rock. The floor, broken by the hall's pit (x 56 to 60).
paint ground 0 23 56 1
paint ground 60 23 12 1
# The cave: its left wall, its ceiling, and a step.
paint ground 0 10 1 13
paint ground 0 10 25 1
paint ground 16 21 2 2
# The shaft: the wall with the cave (a doorway at the bottom), its top, and
# the wall with the hall.
paint ground 24 0 1 20
paint ground 24 0 20 1
paint ground 43 0 1 20
# The hall: its ceiling and its right wall.
paint ground 43 10 29 1
paint ground 71 10 1 13

# Planks, three apart so each is a jump from the one below.
paint planks 8 20 4 1
paint planks 26 20 5 1
paint planks 33 17 5 1
paint planks 26 14 5 1
paint planks 33 11 5 1
paint planks 26 8 5 1
paint planks 30 5 9 1
# A way over the pit.
paint planks 54 20 9 1

$A edit commit tx="$TX"

# Added after the first pictures (docs/phase1-gate.md): the pit had nothing
# behind it, so the clear colour showed through and it read as more floor;
# and the space above the cave and the hall was unpainted, so the view of
# the narrow shaft showed grey slivers at its top corners.
TX=$($A edit begin agent:claude note="The pit read as floor: paint the back wall into it, and fill the empty rock above the rooms" --json | sed 's/.*"tx":\([0-9]*\).*/\1/')
paint back 56 23 4 1
paint ground 0 0 24 10
paint ground 44 0 28 10
$A edit commit tx="$TX"
$A scene tiles info "$MAP"
