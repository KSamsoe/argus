# Scenes

This chapter covers the authored world: a scene's folder and `scene.toml`, one TOML file per entity, ids and tags, components in a file, children, prefabs and overriding them, rooms that stream in and out for big worlds, tilemaps, how a scene loads, editing with `edit` versus editing files, and how `argus summary` and `ARGUS.md` describe a scene. It is for anyone building levels, by hand or with an agent.

## A scene is a folder

Every folder under `scenes/` with a `scene.toml` in it is a scene, named after the folder. `argus.toml`'s `default_scene` is the one the daemon opens first.

```
scenes/
  level/
    scene.toml          the scene's settings
    player.toml         @player
    camera.toml         @camera
    ground.start.toml   @ground.start
    coin.1.toml         @coin.1
    ...
```

That is the platformer template's `scenes/level/`. List and switch scenes with:

```sh
argus scene list
argus scene load scene=level   # switch to the scene in scenes/level/
argus scene load seed=7        # the same scene again, with another seed
```

`scene.toml` holds settings, not entities:

| key | default | what it is |
|---|---|---|
| `clock` | `argus.toml`'s | `"continuous@60"` (any rate) or `"discrete"` (turns). |
| `seed` | `"random"` | The run's seed. A number replays exactly; `"random"` lets the runner pick one and record it. |
| `bounds` | none | `[x0, y0, x1, y1]`: a following camera with `bounded = true` keeps its view inside. |
| `[systems] enable`, `disable` | every system runs | Patterns (`sim.gems`, `sim.*`) of systems to run or switch off in this scene. |
| `[rooms]`, `[rooms.graph]` | none | Makes it a room scene (below). |

From `templates/platformer/scenes/level/scene.toml`:

```toml
clock = "continuous@60"
seed = 1

# The camera keeps its view inside this rect [x0, y0, x1, y1]: widen it
# when the level grows.
bounds = [0.0, -4.0, 49.0, 12.0]
```

A system switched off with `[systems] disable` shows why in `argus script systems`: "listed in scene.toml systems.disable".

## One file per entity

Each other `.toml` file in the folder is one authored entity, and its name without `.toml` is the entity's id: `player.toml` is `@player`, `door.east.toml` is `@door.east`. (Files named `scene.toml` or `room.toml`, and files ending `.tilemap.toml`, `.map.toml`, or `.navgrid.toml`, are not entities.)

Every top-level table in the file is a component, and its keys are field values. From `examples/delve/scenes/delve/player.toml`:

```toml
tags = ["player"]

[PlayerSlot]
slot = 1

[Actions]

[Transform]
pos = [0.0, 0.0]

[Health]
cur = 10
max = 10

[Sprite]
atlas = "sprites/hero"
size = [1.0, 1.0]
z = 5.0
```

- A field left out keeps its default. An empty table (`[Actions]`) adds the component at its defaults.
- A component the file does not name is not on the entity.
- Fields are flat: a field's value is never a table.
- `argus describe component=Sprite` lists a component's fields, types, defaults, and ranges. `ARGUS.md` lists every component the game has.

How each field type is written:

| type | written as | example |
|---|---|---|
| `bool` | `true` or `false` | `visible = false` |
| integers (`u8` ... `i64`) | a whole number | `slot = 1` |
| `f32`, `f64` | a number | `z = 5.0` |
| `vec2` | two numbers | `pos = [3.0, 1.0]` |
| `str` | a string | `color = "#F2C14E"`, `map = "maps/floor.tilemap.toml"` |
| `ref` | a def row, or a project path; `""` for none | `crop = "item:crop"` (Outpost's `Field`) |
| `entity` | another entity's id; `""` for none | `target = "@player"` |

A value of the wrong type, out of its field's range, or naming nothing is a scene issue (below), reported at its file and line.

## Ids and references

| written | means |
|---|---|
| `@player` | the authored entity `player.toml` |
| `@west/gem` | the entity `gem.toml` in room `west` (room scenes) |
| `@spikes/tooth1` | the child `tooth1` of `@spikes` |
| `e:12` | a runtime entity, by its number (never written in a file) |

In a room scene, a short id like `@player` works when only one loaded entity has it. When several do, the answer says so:

```
error 4203: @gem could be any of @east/gem, @west/gem
  hint: In a room scene an id is room/id; name the room.
```

Runtime entities, the ones systems spawn, have no string id and never get a file. Only authored entities are saved to the scene.

## Tags

```toml
tags = ["player"]
```

A tag is a component with no fields. Using one in `tags` is enough to make it exist; `q:each("player")` in a script and `where.has=[player]` in a query find it. `-tags = ["big"]` drops a tag a prefab gave.

## Children

An entity can carry child entities in its own file, as `[children.<name>.<Component>]` tables. Each child gets the id `<parent>/<name>`, a `Parent` pointing at the entity, and a `Transform` relative to it. From `templates/platformer/prefabs/spikes.toml`, one of four teeth:

```toml
[Collider]
size = [1.8, 0.4]
offset = [0.0, 0.2]

[Hazard]

[children.tooth1.Transform]
pos = [-0.75, 0.0]
rot = 0.7853982

[children.tooth1.Sprite]
kind = "rect"
size = [0.5, 0.5]
color = "#E04848"
z = -1.0
```

The engine adds the child's `GlobalTransform` (where it is in the world) by itself. Children nest: `[children.panel.children.title.UiText]`.

An entity in a file of its own can hang under another with `[Parent]`. From `templates/platformer/scenes/level/goal.flag.toml`, the flag's cloth under its pole:

```toml
[Parent]
entity = "@goal"

[Transform]
pos = [0.55, 1.1]
```

Move the pole and the cloth goes with it.

## Prefabs

A prefab is an entity file kept under `prefabs/`, named by its path: `prefabs/platform.toml` is `"platform"`, `prefabs/door/locked.toml` is `"door/locked"`. A prefab spawns nothing by itself. An entity starts from one with `prefab = "..."` and then holds only what it changes, and a system makes one while the game runs with `q:spawn_prefab` (see [scripting](scripting.md#spawning-from-a-prefab)). From the platformer template, `prefabs/platform.toml`:

```toml
[Sprite]
kind = "rect"
color = "#3F5A3A"
z = 0.0

[Collider]

[KinematicBody]
solid = true

[Describe]
name = "ground"
```

and `scenes/level/ground.start.toml`, one block of the level:

```toml
prefab = "platform"

[Transform]
pos = [8.5, -2.0]

[Sprite]
size = [15.0, 4.0]

[Collider]
size = [15.0, 4.0]
```

The rules are few:

- **Inheritance is field by field.** `[Sprite] size = [...]` here keeps the prefab's `kind`, `color`, and `z`.
- **A prefab can start from another.** The chain can be as long as you like.
- **Adding is just writing it.** A component or tag the prefab lacks is added by naming it.
- **Removing is written out.** `-Describe = true` removes a component the prefab has, `-tags = ["big"]` a tag, `-children = ["tooth4"]` a child.
- **Children are overridden by path.** `[children.tooth1.Sprite] color = "#FFFFFF"` changes one tooth.

A big coin made from the platformer's coin, and one of them placed without its tag or its `Describe`:

```toml
# prefabs/coin_kinds/big.toml: "coin_kinds/big"
prefab = "coin"
tags = ["big"]

[Sprite]
size = [1.2, 1.2]

[Collider]
radius = 0.6
```

```toml
# scenes/level/coin.big.toml: @coin.big
prefab = "coin_kinds/big"
-tags = ["big"]
-Describe = true

[Transform]
pos = [10.0, 3.0]
```

And a second row of spikes with three teeth, the first one white:

```toml
# scenes/level/spikes.2.toml: @spikes.2
prefab = "spikes"
-children = ["tooth4"]

[Transform]
pos = [30.0, 0.0]

[children.tooth1.Sprite]
color = "#FFFFFF"
```

To see where each value of an entity comes from:

```sh
argus world get entity=@coin.1 resolved=true
argus world prefabs                  # every prefab, its chain, and how many instances
argus world instances prefab=coin    # every entity made from it, in every scene
```

With `resolved=true` each field says whether it came from the entity's own file, a prefab (with the file and line), or the default. The editor's inspector shows the same thing as a colour.

Changing a prefab is one edit to the prefab's file. Every instance that does not override that field changes with it in the running game, and the feed lists the instances it touched:

```sh
argus edit author=human note="bigger coins" 'ops=[{op: prefab, prefab: coin, path: Sprite.size, value: [0.8, 0.8]}]'
```

## Rooms: big worlds that stream

A flat scene keeps every entity loaded. A big world is better split into **rooms**: the daemon keeps only the rooms near the player loaded and remembers what happened in the rest. `examples/hollow` has 201 rooms and about ten thousand entities, and keeps only the handful of rooms around the hero loaded.

A room scene's entities live in `scenes/<scene>/rooms/<room>/`, each room with a `room.toml`:

```
scenes/caves/
  scene.toml
  rooms/
    persistent/   room.toml  player.toml
    west/         room.toml  gem.toml
    east/         room.toml  gem.toml
    far/          room.toml  gem.toml
```

The scene's `scene.toml` says how they join and what to follow:

```toml
clock = "continuous@60"
seed = 1

[rooms]
start = "west"
radius = 1
follow = "@player"
always = ["persistent"]

[rooms.graph]
"west" = ["east"]
"east" = ["far"]
```

Each `room.toml` has the room's bounds in the world and repeats its neighbours (the engine checks the two agree):

```toml
bounds = [10.0, -10.0, 30.0, 10.0]
neighbors = ["west", "far"]
```

| `[rooms]` key | what it is |
|---|---|
| `start` | The first room, where loading begins. |
| `radius` | Rooms within this many steps of the current room stay loaded (default 1). |
| `follow` | The entity rooms stream around. The current room is the one whose bounds hold it. |
| `always` | Rooms that never unload: the followed entity's, the camera, the HUD, the menus. A room here may have no bounds. |
| `transition` | A timeline played at every room change (`"timeline:rooms.fade"` in Hollow). |

The followed entity must live in an `always` room. When it crosses into another room, `rooms.stream` loads the rooms within the radius, unloads the rest, and emits `room.transition {from, to}`. An unloaded room remembers what changed in it: in the caves above, a gem taken in `far` stays taken when the player comes back.

```sh
argus scene rooms              # every room: loaded or not, neighbours, bounds, entities
argus scene rooms room=far     # what an unloaded room remembers
argus query 'select=[id, room]'
argus query 'select=[id]' 'where.room=west'
```

A query about a room that is not loaded is answered from the files, through the index, and says `source: index`. A query of the live world says how many rooms it could not see (`unloaded_rooms`).

## Tilemaps

A tilemap is a grid of tiles the engine draws, collides with, and finds paths on. It is bulk data: a small text index (`*.tilemap.toml`) beside a folder of binary chunks. You make and change it through the daemon, never by hand.

**A tileset** is a sprite asset whose frames are the tiles. A tile's frame properties say how it collides: `solid`, `one_way` (a plank to jump up through), or `slope`. With no image yet, a sidecar can describe a placeholder. `assets/tiles/stone.toml`:

```toml
# A placeholder tile: a grey square, solid from every side.
kind = "rect"
size = [16, 16]
color = "#6B7280"

[[frames]]
index = 0
properties = { solid = true }
```

**Make the map**, 20 by 10 tiles with one layer:

```sh
argus scene tiles.create file=scenes/level/blocks.tilemap.toml width=20 height=10 'layers=[{name: ground, tileset: tiles/stone, collision: true}]'
```

This writes `blocks.tilemap.toml` (the index: size, layers, a hash) and a `blocks.chunks/` folder.

**Place it** with an entity. Its `Transform` is the top-left corner of tile (0, 0); rows go down. `scenes/level/blocks.toml`:

```toml
[Transform]
pos = [50.0, 10.0]

[Tilemap]
map = "scenes/level/blocks.tilemap.toml"
```

**Paint it** with an edit. Tile `n` is frame `n - 1` of the layer's tileset, and 0 is empty. This fills the bottom row:

```sh
argus edit author=human note="a floor of stone" 'ops=[{op: tiles, file: scenes/level/blocks.tilemap.toml, layer: ground, rect: {x: 0, y: 9, w: 20, h: 1}, tile: 1}]'
argus scene tiles.collision file=scenes/level/blocks.tilemap.toml 'rect={x: 0, y: 6, w: 20, h: 4}'
```

```
....................
....................
....................
####################
```

The editor's Tiles tab paints the same way. A tiles op can also `erase`, give every cell (`cells`), or paint a `terrain` that picks tiles by their neighbours (`[autotile]` in the tileset).

Two other kinds of map:

- **A runtime map** (`runtime = true` in the index) has no chunks: its cells start empty each run and a script fills them with `q:tiles(map)`. `examples/delve/maps/floor.tilemap.toml` is one, painted by `systems/floor.luau` from the seed. The cells are the map file's for the run, not an entity's: `q:tiles(e)` takes an entity that places the map and reaches its file's cells, so two entities placing one runtime map share them. Saves, seeks, and replays keep them, and the `scene.tiles.get` method reads them as the run has them.
- **A Tiled map** (`.tmx` or `.tmj` under `assets/`) is read as Tiled saved it and placed the same way, `[Tilemap] map = "assets/maps/level1.tmx"`. Edit it in Tiled; the engine reads it again when Tiled saves.

The "Files" section of `argus docs` has every key of a tilemap, a tileset sidecar, and a Tiled map.

## How a scene loads

When a scene loads (when the daemon opens the project, on `argus scene load`, and at the start of every test run):

1. The scripts in `components/`, `defs/`, and `systems/` load, so every component is declared.
2. Prefabs and data tables are read.
3. The scene's files are read, every prefab chain resolved, and every reference checked.
4. If anything is wrong, nothing spawns. Otherwise the entities spawn (in a room scene, the rooms around the start).
5. Systems with `on_load = true` run once, before the first tick.

A scene with a problem never half loads. `argus scene load` lists the issues, and so does `argus scene issues`:

```
total: 2
code  data                                                          message
----  ------------------------------------------------------------  -------------------------------------
4001  {"hint":"world.describe lists registered components; script components must load before the scene.","ref":"scenes/level/bad2.toml:4"}  no component named "Hazrd"
4003  {"ref":"scenes/level/coin.bad.toml:7"}                        expected a bool value; got an integer
```

Each issue names its file and line. One file can hold more than one mistake and the list may show only the first, so load again after fixing.

## Editing: through the daemon, or the files

There are two ways to change a scene, and both are fine.

| | `argus edit` (or the editor, or an agent) | a text editor |
|---|---|---|
| the files | only the changed keys are written; comments and layout kept | whatever you type |
| the running game | changes at once | unchanged until `argus scene load` |
| the change feed | a row with author and note | nothing |
| undo | `argus edit undo=true author=...` | git |
| checking | refused before it lands if a value is wrong ("Hero.fall_y must be in range [-100, 0]; got 5") | found at the next load |

The edit ops for scenes are `set`, `add`, `remove`, `spawn` (with an `id` it makes a new entity file), `despawn` (deletes an authored entity's file), `prefab`, and `tiles`:

```sh
argus edit author=human note="a red coin" 'ops=[{op: set, entity: "@coin.2", path: Sprite.color, value: "#FF5050"}]'
argus edit author=human note="a second gem" 'ops=[{op: spawn, id: gem.2, components: {Transform: {pos: [6.0, 2.0]}, Gem: {value: 2}}}]'
```

Setting a field on an entity made from a prefab writes just that override into its file:

```toml
prefab = "coin"

[Transform]
pos = [26.0, 3.6]

[Sprite]
color = "#FF5050"
```

After editing files by hand, run `argus scene load`. `argus reload` takes in scripts and assets but leaves the live world's entities alone.

**Renaming** an entity file, a prefab, or an asset goes through the daemon, so references follow:

```sh
argus asset move from=scenes/level/goal.toml to=scenes/level/flag.toml
```

It rewrites every reference in the project's TOML files (here `goal.flag.toml`'s `entity = "@flag"`) and logs the move in `.argus/moves.log`, so an old name in an error points to the new one. It does not rewrite ids written in Luau, such as `q:entity("@goal")` in a system or `"@goal"` in a test: search for those yourself. The loaded world keeps the old id until `argus scene load`.

## How a scene is described

**`argus summary`** is the one-screen answer for the loaded scene: entity counts grouped by their components (`archetypes`), the named entities (the first 50, with `named_total`), the rooms loaded, the systems in run order and any `disabled`, the frame, the seed, and the clock. With `scene=` it answers for another scene from the index, without loading it.

**`ARGUS.md`**, written by `argus docs --for-agent`, has a Scenes section with each scene's clock, seed, entity count, and rooms, and a table of its named entities and their components. From the tutorial game in [Your first game](first-game.md), with the caves scene above added:

```
### caves

Clock `continuous@60`, seed 1, 4 entities (4 authored), rooms east, far, persistent, west.

| entity | components |
|---|---|
| `@east/gem` | Transform, Sprite, Gem |
| `@far/gem` | Transform, Sprite, Gem |
| `@persistent/player` | Transform, PlayerSlot, Actions, Sprite, Camera, Player, Score, player |
| `@west/gem` | Transform, Sprite, Gem |

### main (loaded)

Clock `continuous@60`, seed 1, 3 entities (3 authored).
```

`ARGUS.md` does not update itself: write it again after adding entities, components, or scenes.

For a room scene that is loaded, both describe the rooms loaded at that moment, not the whole scene. In a copy of Hollow, `ARGUS.md` and `argus summary` said 136 entities in 6 rooms, while the scene has 201 rooms and 10,081 entities. Ask the index for the whole of it:

```sh
argus world summary source=index
```

## Where to go next

- [Scripting](scripting.md): the systems that work on these entities.
- [Gameplay](gameplay.md): physics, colliders, navigation grids, and saves.
- [Presentation](presentation.md): sprites, cameras, lighting, and UI trees.
- [Content](content.md): data tables, dialogue, timelines, and strings.
- [Playtesting](playtesting.md): the editor's hierarchy, inspector, and tile painting, and the change feed.
