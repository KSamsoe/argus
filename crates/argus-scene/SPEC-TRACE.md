# argus-scene: spec trace

Every statement in the design document (v0.3) that constrains authored content (§3 "Prefabs and overrides", §4 "Project & scene storage", and the related lines of §5, §6, §17), with where it lives and the test that pins it. Statuses as in the other traces: **Done**, **Partial**, **Deferred**.

## §3 Data model

| Spec | Status | Where | Test |
|---|---|---|---|
| Authored entities carry a stable string id, unique within the scene, stable across sessions, saves, and renames of everything else. | Done | Filename is the id (`door.east.toml` → `door.east`); in a room scene the full id is `room/id`. Renaming an entity is `asset.move`, which rewrites the refs; nothing else changes it. | `scenes::a_flat_scene_spawns_authored_entities_with_ids`, `moves_edits::moving_an_entity_or_a_def_row_rewrites_refs_in_place` |
| Runtime-spawned entities never get string ids and are never written to scene files. | Done | Only ids that resolve to a file can be edited; anything else is error 4609 `NOT_AUTHORED`. `World::is_authored` is the runtime side. | `scenes::a_flat_scene_spawns_authored_entities_with_ids` |
| A prefab is a named bundle of components with values, and may itself be built from another prefab (`door/locked` from `door/base`). | Done | `prefab.rs`: `prefabs/<path>.toml` is an entity document whose `prefab` key names its base; `PrefabLibrary::chain` walks it. | `docs_prefabs::prefab_chain_gives_every_field_a_provenance` |
| An entity that references a prefab stores only the fields it overrides; everything else is inherited at load and re-inherited when the prefab changes. | Done | `doc.rs` stores what the file says; `resolve.rs` merges; `Scene::reapply_prefab` re-resolves and rewrites loaded instances. | `docs_prefabs::the_spec_entity_file_parses_into_overrides_only`, `moves_edits::a_prefab_change_touches_its_instances` |
| Inheritance is per field, never per component. Overriding `Door.locked` does not detach the rest of `Door`. | Done | `resolve_layers` applies layers base-first, field by field. | `docs_prefabs::inheritance_is_per_field_and_removal_is_explicit` |
| Adding a component the prefab lacks is an override; removing one the prefab has is an explicit `-Component` entry, so a reader sees the removal in the file. | Done | `[Component]` (even empty) adds; `-Component = true` removes. `Scene::remove_component` writes the `-` entry when the component is inherited, and just drops the table when it is the entity's own. Tags and children follow the same rule (`-tags`, `-children`). | `docs_prefabs::removals_children_and_bad_keys`, `moves_edits::overrides_are_written_back_without_disturbing_the_file` |
| A prefab may contain child entities; children are addressed by path (`@door.east/light`) and overridden the same way. | Done | `[children.<name>.<Component>]` in prefab or entity files; the child spawns as `parent/name` with a `Parent` component; overrides in the entity file merge layer by layer. | `scenes::rooms_stream_by_graph_distance_and_refs_follow`, `docs_prefabs::prefab_chain_gives_every_field_a_provenance` |
| `world.get {resolved: false}` returns only the overrides; `resolved: true` returns the merged result with each field's provenance (`own`, `door/locked`, `door/base`). | Done | `Scene::overrides` (the `EntityDoc`) and `Scene::resolved` (a `ResolvedEntity` with `Provenance` per field, component, and tag). Fields nobody set carry `default`, which the document does not list and this crate adds. | `docs_prefabs::prefab_chain_gives_every_field_a_provenance` |
| Changing a prefab is a transaction like any other; the index lists every instance, and the change feed reports how many entities the change touched. | Done (data side) | `Scene::instances_of` lists instances; `Scene::reapply_prefab` returns every entity touched (children that inherit through the parent's prefab included). The transaction and feed are the daemon (piece 4); the index listing is piece 6. | `moves_edits::a_prefab_change_touches_its_instances` |
| Tags are zero-size components. | Done | `tags = [...]` in files; a tag no one registered is registered on first use with the file as its source. | `scenes::a_flat_scene_spawns_authored_entities_with_ids` |
| Relations are components holding entity references. | Done | `@id`, `@room/id`, `@id/child` strings in files resolve to live handles at spawn; empty is null. | `scenes::rooms_stream_by_graph_distance_and_refs_follow` |

## §4 Project and scene storage

| Spec | Status | Where | Test |
|---|---|---|---|
| Files are the source of truth, read by git, humans, and agents in that order. The index is a cache, never committed. | Done | Everything here reads and writes TOML; `Project::init` writes `.argus/` into `.gitignore`. The index is piece 6. | `scenes::*` |
| `argus.toml`: project name, clock config, plugin list, default scene. | Done | `ProjectConfig` (`name`, `clock`, `default_scene`, `plugins`); the clock spec is validated with `argus_sim::Clock::parse`. Phase 2 adds `[ring] every` and `budget_mb` (item 0) and `[marks] key`, the key that drops a mark, F8 unless set (item 2). Phase 3 adds `[time] day`, the sim seconds in one of the game's days (86,400 unless set; Outpost's is 60, decided with Kevin), which the harness's `days()` and scripts' `q.day` count in. | `scenes::a_flat_scene_spawns_authored_entities_with_ids`, `argus-daemon/tests/window.rs` |
| Layout: `components/`, `systems/`, `defs/`, `prefabs/`, `scenes/<name>/scene.toml`, one authored entity per file, `rooms/<room>/room.toml`, `assets/`, `tests/`, `plugins/`, `.argus/` gitignored. | Done | `Project::init` creates it; `Project::scenes` lists `scenes/*/scene.toml`; `Scene::open` reads flat or room layouts. `components/` and `systems/` are read by `argus-luau`. | `tests/common/mod.rs` is the §4 layout; `scenes::*` |
| One authored entity per TOML file: small, flat, diffable, never a reason to read a neighbour's file. | Done | `EntityDoc`; the spec's `door.east.toml` parses verbatim. Edits rewrite only the changed key and keep comments and spacing, and write a vector's f32s by their shortest text (`21.2`, not `21.200000762939453`, found when the phase 2 gate's proposal moved Hollow's gem). | `docs_prefabs::the_spec_entity_file_parses_into_overrides_only`, `moves_edits::overrides_are_written_back_without_disturbing_the_file` |
| Entity refs by string id, room-qualified (`@cavern-04/door.west`); def refs like `item:key.brass`, not GUIDs. | Done | `@x` resolves in the writer's room first, then scene-wide; `@room/x` is explicit. Def refs are validated against the loaded tables. | `scenes::rooms_stream_by_graph_distance_and_refs_follow`, `scenes::graph_and_reference_problems_are_issues_with_locations` |
| `scene.toml`: bounds, enabled system groups, seed policy, clock; for room scenes the room graph, streaming radius, transitions. | Done | `SceneDoc { clock, seed, bounds, enable, disable, rooms: RoomGraph { start, radius, edges } }`. Every scene may declare its clock (§6); missing means the project default. Seed is a number or `"random"`. | `scenes::a_flat_scene_spawns_authored_entities_with_ids`, `scenes::rooms_stream_by_graph_distance_and_refs_follow` |
| `room.toml`: bounds, tileset, neighbours. | Done | `RoomDoc`. The scene file holds the graph; `neighbors` in a room file must agree with it or the scene reports the mismatch naming both files. | `scenes::graph_and_reference_problems_are_issues_with_locations` |
| Rooms are directories; the daemon streams rooms in and out around the player by graph distance, so a two-hundred-room game never has more than a handful live. | Done (mechanism) | `Scene::spawn_around`, `transition`, `load_rooms`, `unload_room`; distance is BFS over the graph; rooms within `radius` stay loaded. References into an unloaded room are null and remembered, and filled when it returns. Streaming at scale is phase 3. | `scenes::rooms_stream_by_graph_distance_and_refs_follow`, `scenes::unloaded_room_targets_are_pending_not_errors` |
| The index still answers queries about rooms that aren't loaded. | Partial | The scene keeps every room's resolved entities in memory (`Scene::resolved`, `instances_of`) whether or not they are spawned, which is what the index will serve. The SQLite index is piece 6. | `scenes::unloaded_room_targets_are_pending_not_errors` |
| Transitions between rooms are an engine event with a hook for the game. | Done | `room.transition {from, to}`, `room.loaded {room, entities}`, `room.unloaded {room, entities}` as `Event::Game`. | `scenes::rooms_stream_by_graph_distance_and_refs_follow` |
| Data tables in `defs/` as TOML with declared schemas, referenced as `item:key.brass`, queryable like everything else (`world.defs {table, where}`). | Done | `defs.rs`: rows are `["key"]` tables validated against a `DefSchema` (from `def "items" {...}` in script, or Rust). `DefTable::query` is the `where` half of `world.defs`; a table without a schema is an issue with the fix in the hint. | `defs_bulk::def_rows_are_keyed_typed_and_queryable`, `defs_bulk::def_problems_are_located_and_the_rest_loads` |
| Asset and prefab references are paths. Renames go through `asset.move`, which rewrites references and appends to `.argus/moves.log` so an out-of-date client can catch up. | Done | `moves.rs`: prefabs, entity files, def rows (`defs/items.toml#key`), and other project files; every string value in `prefabs/`, `scenes/`, `defs/` that spells the old reference is rewritten in place. | `moves_edits::moving_a_prefab_rewrites_every_reference_and_logs_it`, `moves_edits::moving_an_entity_or_a_def_row_rewrites_refs_in_place` |
| A rename done outside the tool leaves a clear "missing reference" error with the old path in it. | Done | Error 4603 names the kind and the old path with the referencing `file:line`; if the moves log knows the new path, the hint says so. | `moves_edits::an_outside_rename_leaves_a_missing_reference_naming_the_old_path` |
| Bulk data: a text index (dimensions, tileset, layer names, a content hash) plus a binary chunk; the agent queries the chunk through the protocol (`scene.tiles.get {rect}`) and never sees a 40,000-line array. | Done | `bulk.rs`: `*.map.toml` next to a `.bin`; `Bulk::get(layer, rect)` and `set`; size and hash are checked on open. Tilemap semantics (auto-tiling, collision) are phase 1. | `defs_bulk::bulk_chunks_round_trip_and_check_their_hash` |
| Data indexes sit beside entity files without being read as entities. | Done | `bulk::is_data_index`: `*.map.toml`, since phase 1 item 5 tilemaps (`*.tilemap.toml`), and since phase 3 item 2 baked navigation grids (`*.navgrid.toml`) are listed with the room's bulk files, skipped by `asset.move`'s entity rule, and left out of the index's staleness check. `bulk::reserved_stem` refuses the entity ids `scene` and `room` and ids ending `.map` or `.tilemap`, in `create_entity` and in an entity move, since such a file would be written and then skipped on load. | `defs_bulk::bulk_chunks_round_trip_and_check_their_hash`, `defs_bulk::reserved_names_are_not_entity_ids`, `argus-daemon/tests/tiles.rs` |
| Open: TOML versus KDL. | Decided | TOML, as the document leans; `toml_edit` keeps formatting and comments on writes, which is what makes file-per-entity diffs clean. | |

## §5 Protocol (the parts this crate serves)

| Spec | Status | Where | Test |
|---|---|---|---|
| `edit.commit` on an authored entity writes its file (`scenes/arena/spawner.north.toml`). | Done (data side) | `Scene::set_override`, `clear_override`, `add_component`, `remove_component`, `create_entity`, `delete_entity` write files; `Scene::sync_entity` brings the live entity in line. Transactions, authors, and the feed are the daemon. | `moves_edits::overrides_are_written_back_without_disturbing_the_file` |
| Errors carry code, message, hint, `file:line`. | Done | Every error here has a ref into the TOML; range and type errors from the schema keep the schema's ref and add the TOML location to the message. | `docs_prefabs::schema_errors_carry_both_locations` |
| Identical result shapes from the live world or the index. | Deferred | Piece 6. `ResolvedEntity` is the shape both will produce. | |

## §6 Determinism

| Spec | Status | Where | Test |
|---|---|---|---|
| Given the same scene, seed, input log, and code version, the world at tick N is bit-identical. | Done (scene half) | Entities spawn in sorted id order, rooms in sorted name order, children after their parent; the same files give the same handles and the same values. A scene with any open issue refuses to spawn (4611), so a run never starts from a half-loaded world. | `scenes::loading_is_deterministic`, `docs_prefabs::schema_errors_carry_both_locations` |
| Every scene declares which clock drives its sim stage. | Done | `SceneDoc::clock`, validated; falls back to `argus.toml`. | `scenes::a_flat_scene_spawns_authored_entities_with_ids` |
| (Phase 2 item 2.) The rooms loaded, in a keyframe on disk. | Done | `StreamState::encode` and `decode`. | `argus-daemon/tests/marks.rs` |

## §17 Performance

| Target | Spec | Measured (release, development laptop) |
|---|---|---|
| Hollow authored entities per scene | 10,000 across 200 rooms | `perf::cold_open_of_ten_thousand_entities_in_two_hundred_rooms` builds exactly that |
| Cold scene open (index warm) | < 300 ms | 213–245 ms for open plus resolve with no index at all; files are read and parsed on every core because the file system is the cost (read alone is 270–520 ms serial on NTFS) |
| Room stream-in | < 4 ms (phase 1) | 0.7 ms worst for one 75-entity room in and one out |
| Spawn everything | (no target) | 15,000 entities in 62–66 ms |

## §18 Build plan

Phase 0 item 5, "TOML scenes, rooms, defs, and prefab overrides": this crate.

## Phase 3, item 4

| Spec | Status | Where | Test |
|---|---|---|---|
| (§12) A game is written in one language. | Done | `ProjectConfig::strings_base`, argus.toml `[strings] base` ("en" when it says nothing): the string table every other language falls back to, and the one a release build keeps in the game's pack. | The games' string tables; `argus-player` `packs::hollow_from_its_packs_plays_as_hollow_from_its_folder` |

## Known limitations, stated so they are decisions

- A child entity that exists only because a prefab declares it is not an "instance" of that prefab (`instances_of`), but it is counted among the entities a prefab change touched (`reapply_prefab`).
- Provenance `default` is added for fields no layer set; the document lists only `own` and prefab paths.
- Tags named in a file but registered nowhere are registered on first use (zero-size, documented as declared by use). A typo in a tag is therefore a new tag, visible in `world.describe`, not an error.
- Bare path references are checked against the prefab library and the project tree; `prefix:key` references with an unknown prefix pass through (asset kinds arrive with the importer in phase 1).
- The moves log records unix seconds and does not rotate.
- Streaming keeps every room's resolved documents in memory; at 10,000 entities that is a few megabytes. The index (piece 6) is the place to move them if a project outgrows that.
