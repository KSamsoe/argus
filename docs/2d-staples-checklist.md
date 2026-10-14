# 2D staples: checklist

Kevin, 2026-10-14: after 0.0.8, the next work is the staples a 2D game reaches for that Argus lacks or has thinly, found by comparing it with Godot, GameMaker, Defold, and Unity 2D, and by the design's own open questions. In order:

## 0. Raycasts and shape queries from scripts

- `q:raycast(from, to, opts)`: the first thing a line from `from` to `to` meets, or nil: the entity (or the tile cell), the point, the surface's normal, the distance, and the fraction of the way. `q:raycast_all` gives every hit, nearest first.
- `q:shape_cast(shape, from, to, opts)`: the first thing a shape (`{ radius = r }`, `{ size = v }`, and the collider shapes item 2 adds) moving from `from` to `to` would touch, with where it stops.
- `q:overlap(shape, at, opts)`: everything a shape placed at a point touches; a point query is a shape of radius 0.
- What they meet: every `Collider` (by its layer, against `opts.mask`) and every Tilemap's collision cells (solid, slopes, and one-way platforms only for a line going down through their top, unless `opts.one_way` says otherwise), skipping `opts.ignore`. Inactive entities are not there.
- One geometry core in argus-physics: everything is a rounded convex polygon (a point grown by a radius is a circle, a segment a capsule), and a moving shape against a target is a ray against their Minkowski sum. Exact, deterministic float math (no trig), the same on every platform.

Built (2026-10-14): `argus_physics::geom` (`Round`, the ray, distance, and Minkowski sum) and `argus_physics::query` (`raycast`, `shape_cast`, `overlap` over colliders and tile cells, a grid walk per map for rays). Scripts reach them through argus-luau's `Casts` service, which argus-player fills; a query names `Collider` and `Tilemap` in the system's reads or leaves one out with its option. A capsule shape is `{ radius, height }`, `height` the whole of it standing up. Tests: argus-physics `tests/query.rs` (a room of solid, slope, and one-way cells with colliders), argus-daemon `tests/casts.rs` (through a script, and the undeclared read).

## 1. Debug drawing from scripts

- `q:draw` lines, boxes, circles, arrows, and text in world space, for this frame: seen in the window, the editor, and a picture frame (`obs.frame`), never in a release build, and not part of the world.

## 2. Polygon and capsule colliders

- `Collider` takes a capsule (`radius` with a `height`, as the queries' capsule) and a convex polygon (`points`, a list field): the overlap tier, rigid bodies, and the queries meet them. A kinematic body stays a box, said so.

## 3. Parallax

- A camera-relative scroll factor on sprites and tile layers (a background moves at a share of the camera's speed), and Tiled's own parallax factors taken instead of dropped.

## 4. Save slots

- Scripts list the saves there are, with what each holds (when it was made, the scene, a game's own summary fields), and delete one; a "Continue" button and a three-slot menu can be built.

## 5. Animation between turns

- In a turn-based game, animators, particles, and tweens move while the game waits for the next turn, not only on turns. Decided with Kevin before it is built: what this does to determinism, replays, and digests.

## Docs

- The gameplay chapter (queries, colliders, saves), the presentation chapter (debug drawing, parallax), the references, and the file formats.

## Gate

- Tests for each part, and one example using each the way a game would.
