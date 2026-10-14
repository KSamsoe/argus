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

Built (2026-10-14): `q:draw_line`, `q:draw_arrow`, `q:draw_box`, `q:draw_circle`, `q:draw_text`, with `color`, `width`, `fill`, and `size`. "For this frame" became "until the system runs again": a turn-based game's shapes would otherwise vanish the frame after its turn. The list is `argus_sim::Draws`, kept by the script host (each system's emptied as it starts a run, forgotten when it is declared again or removed), off unless the host turns it on, which only the daemon does. The renderer draws it on the UI canvas under the UI (`argus-render` `debug.rs`), and `SemanticFrame::with_debug` lists it under the frame's `debug` in pixels. Tests: argus-sim `draw.rs` units, argus-daemon `tests/draws.rs` (pixels and the semantic list, a system that stops drawing, a bad colour, a release player keeping none).

## 2. Polygon and capsule colliders

- `Collider` takes a capsule (`radius` with a `height`, as the queries' capsule) and a convex polygon (`points`, a list field): the overlap tier, rigid bodies, and the queries meet them. A kinematic body stays a box, said so.

Built (2026-10-14): `Collider.height` and `Collider.points`, a list of vec2 through argus-ecs's new `Points` field type (a Rust component's list, stored as the list's interned text, as a script's are). `Collider::kind`, `round`, and `boxed` say what it is, its shape for the geometry core, and the box a kinematic body moves as. The overlap pass keeps its box and circle tests and uses the geometry core when either shape is a capsule or a polygon, a polygon's shape made once for its corners (checked by their text, since an id can name another list after a seek back). Rigid bodies take Rapier capsules and convex hulls; a shape changed in place now replaces the collider, because Rapier 0.36's `set_shape` leaves a resting body's contacts as the old shape made them (a box resized in place had the same bug), and a kinematic body's proxy is now the box around its shape (a circle's was its unused `size`). Tests: argus-physics `tests/shapes.rs`, argus-rigid `a_capsule_and_a_polygon_land_on_their_shapes`, argus-daemon `tests/shapes.rs`.

## 3. Parallax

- A camera-relative scroll factor on sprites and tile layers (a background moves at a share of the camera's speed), and Tiled's own parallax factors taken instead of dropped.

Built (2026-10-14): a `Parallax` component (`factor`, `origin`) on a sprite, and a tile layer's `parallax` with its map's `parallax_origin` (argus-tiles `LayerDef`, `MapIndex`). The renderer shifts each by `argus_render::parallax_shift`, `(camera - origin) * (1 - factor)`, with the frame's interpolated camera, so a far layer does not step between ticks; `argus_tiles::visible` takes a parallax layer's cells around the view as the layer sees it, with a margin for those frames. The Tiled importer carries a layer's factors (its groups' multiplied in) and the map's parallax origin instead of warning. The semantic frame places a parallax sprite where it is drawn. Drawing only: collision and overlaps stay where things are. Tests: argus-tiles `a_parallax_layer_is_saved_and_shows_the_tiles_where_it_is_drawn`, argus-assets `tests/tiled.rs` (both map forms), argus-daemon `tests/parallax.rs` (the picture and the semantic frame).

## 4. Save slots

- Scripts list the saves there are, with what each holds (when it was made, the scene, a game's own summary fields), and delete one; a "Continue" button and a three-slot menu can be built.

Built (2026-10-14): `q:saves()`, `q:delete_save(name)`, and `q:save(name, summary)`. The saves on disk are outside the run, as a key is, so a tick's read is input: the first time a tick runs, the host's list is recorded in the input journal on a reserved channel (`argus_input::SAVES_CHANNEL`) for that tick, and a re-run (a seek, a replay, a report) reads the recorded list. Recording at the read, not when the list changes, keeps saves made after a seek back from branching the run, and puts nothing in the journal of a game that never asks. Each host keeps the list: the daemon from `.argus/saves/` (empty while `test.run` runs scenarios), a release build through `Player::list_saves`, `store_save`, and `delete_save` (disk and cloud), the web page through `note_save`, `forget_save`, and `take_deletes` over its IndexedDB store. A save's summary is in its identity (`meta.summary`). Tests: argus-daemon `tests/save_slots.rs` (summaries, newest first, a delete from the disk, a re-run reading the recorded list, a release player's list).

## 5. Animation between turns

- In a turn-based game, animators, particles, and tweens move while the game waits for the next turn, not only on turns. Decided with Kevin before it is built: what this does to determinism, replays, and digests.

Kevin, 2026-10-14: idle time is in the run. Between turns the continuous clock ticks for what moves between turns, the sim frozen, and those ticks are recorded with the input, one entry a gap, so replays, seeks, and reports match and digests stay whole; a headless run sees no idle time unless it asks. (The other choices were the renderer animating on wall time outside the run, or leaving it frozen.)

Built (2026-10-14): `SystemDesc::between_turns` (and a script's `between_turns = true`) on the animators, particles, tweens, the camera, skeletons, timelines, and the post effects. `Scheduler::idle` runs idle ticks: only those systems, on the continuous clock, the world's frame and the turn count kept; on a turn they get dt 0. The input journal records each gap (`Command::Idle`, coalesced); before a turn the scheduler asks how many recorded idle ticks are ahead of the journal's place and runs them, so every path that re-runs turns (a seek, a keyframe check, a replay, `run_until`) runs them. The daemon runs one a frame while a turn-based scene plays (`wants_frames`), and answers `sim.idle {ticks | seconds}`; a release build and the web page run them from wall time; a scenario has `t:idle(seconds)`. Tests: argus-daemon `tests/between_turns.rs` (a turn takes no time, a slide plays between turns, a script's system counts idle ticks, a seek back runs the recorded half second again).

## Docs

- The gameplay chapter (queries, colliders, saves), the presentation chapter (debug drawing, parallax), the references, and the file formats.

Done (2026-10-14): gameplay (rays and shapes, the four collider shapes, save slots, between turns), presentation (debug drawing, parallax), testing (`t:idle`), scripting (`between_turns`, the saves row), the Luau and scenario references, `obs.frame`'s `debug`, `sim.idle`, and the tilemap format's `parallax` and `parallax_origin`.

## Gate

- Tests for each part, and one example using each the way a game would.

Met (2026-10-14). Tests: each item's above. The examples, spread over the games as Kevin chose:

- Toybox (e4de7b07): a wedge with a polygon collider the balls roll down, a pill with a capsule collider standing on the floor, and a tripwire laser along the floor, a raycast each tick that ends on the pill's rounded foot unless a ball rolls through, its beam debug-drawn. `tests/staples.luau`.
- Hollow (032454f2): the back wall is a parallax tile layer at half speed, so its columns pass behind the level in the shaft and the hall. The hall's picture golden is updated for it.
- Delve (df977bef): monsters see the hero only when a ray from their cell to the hero's meets no wall (seen, they come every turn; else every other turn, on the trail); `DebugView.sight` draws each sight line; the menu's Saves page saves three slots with what they hold, loads them, and deletes them. Its sparks fly between turns. `tests/sight.luau`, `tests/saves.luau`; the fight golden is updated for the new pace.
