# argus-index: spec trace

Every statement in the design document (v0.3) that constrains the index (§4 "The index", the index lines of §3 and §5, §17), with where it lives and the test that pins it. Statuses as in the other traces: **Done**, **Partial**, **Deferred**.

## §4 The index

| Spec | Status | Where | Test |
|---|---|---|---|
| On load and on every file change, the daemon updates `.argus/index.db` (SQLite). | Done | `Index::open` creates `.argus/index.db`; the daemon calls `sync_project` at `session.open`, `scene.load`, `script.reload`, and `asset.move`, `sync_entities` / `remove_entity` after every commit that touched an authored entity, and `refresh_index` every 10 s while idle so files edited outside the daemon are picked up; `scene.index` forces it. Changes are detected from file size and mtime recorded per file (`files` table), with a prefab or def change re-indexing every scene. | `index::file_changes_are_picked_up_and_entities_update_incrementally`, `index_shapes::the_index_follows_edits_reloads_and_moves` |
| Entities by scene. | Done | `entities(scene, id, root, room, file, parent, prefab, chain, tags, overrides)`. | `index::sync_indexes_every_scene_prefab_def_schema_and_system` |
| Components by entity. | Done | `components(scene, entity, component, provenance)` and `fields(scene, entity, path, value, provenance, at)`, values as JSON in the protocol's spelling (entity refs as `@full/id`). | `index::queries_have_the_protocol_shape` |
| References between assets, prefabs, and entities. | Done | `refs(scene, from_kind, from_id, path, kind, target, at)` for entity refs, def refs, prefab chains, and path refs, from entities, prefabs, and def rows. `Index::references_to` / `world.refs`; `Index::instances_of` / `world.instances`. | `index::references_instances_and_full_text_search` |
| A full-text column over doc strings. | Done | FTS5 table `docs(kind, name, doc)` with the porter tokenizer over component, field, and system docs. `Index::search` / `world.search`. | `index::references_instances_and_full_text_search` |
| Every `world.*` and `scene.*` query is served from this index when the world is not loaded, and from the live world when it is, with identical result shapes. The agent does not need to know which. | Done | Identical by construction: `argus_index::query::run` is the one evaluator, over a `Source` that is the daemon's `WorldSource` or the index's `SceneSource`; `shapes::{Summary, resolved_json, overrides_json}` build `world.summary` and `world.get` for both. The daemon routes to the index for another scene (`scene` param), on request (`source: "index"`), or when the loaded scene could not be instantiated. A `source` field says which answered, for the human. | `index_shapes::live_and_index_reads_have_identical_shapes` |
| The index is a cache and is never committed. | Done | Lives under `.argus/` (gitignored by `Project::init`); a schema-version mismatch drops and rebuilds; `synchronous = OFF` because a torn cache costs a rebuild, not data. | `index::file_changes_are_picked_up_and_entities_update_incrementally` (version bump) |
| Prefab and def catalogs. | Done | `prefabs(path, file, base, chain, components, tags)`, `def_tables`, `defs(table, key, row)`; `Index::defs` serves `world.defs`. | `index::get_summary_rooms_describe_and_defs` |
| Component schemas and systems (for `world.describe`, `world.who_writes`). | Done | `schemas`, `systems` tables; `Index::describe`, `who_writes`. | `index::get_summary_rooms_describe_and_defs` |

## §3 Prefabs

| Spec | Status | Where | Test |
|---|---|---|---|
| The index lists every instance of a prefab. | Done | `instances_of`: entities whose chain includes the prefab, top-level or with their own prefab (the same rule as `Scene::instances_of`). | `index::references_instances_and_full_text_search` |

## §5 Protocol

| Spec | Status | Where | Test |
|---|---|---|---|
| Read contract (limit 50, total, cursor, row arrays). | Done | `query::page` / `query::paged`, shared with the daemon. | `index::queries_have_the_protocol_shape` |

## §17 Performance

| Target | Spec | Measured (release, development laptop) |
|---|---|---|
| Delve: floor generation plus index | < 20 ms | Runtime rooms are never in SQLite: they are queried from the live world the tick they exist. The authored half, re-indexing one floor's worth (200 entity files, 300 entities with children), is 15–19 ms in `perf::full_sync_of_ten_thousand_entities_and_incremental_room_resync`. |
| Hollow: full index of 10,000 entities across 200 rooms | (no target) | 660–840 ms for 15,000 entities including children; ~165,000 rows. |
| Query over an indexed 15,000-entity scene | (no target) | 33 ms for a filtered query returning 1,000 |
| Staleness check of a 10,200-file scene | (idle refresh) | 21 ms, from directory-listing metadata |

## §12, §17 The broadphase in the shared evaluator (phase 1, item 0)

| Spec | Status | Where | Test |
|---|---|---|---|
| Spatial predicates hit the broadphase hash, not a scan. | Done | `Source::near(center, r)` lets a source narrow the candidates of a `within` query; the live world answers from its hash, the index returns `None` and scans as before. The evaluator still measures every distance itself, so both sources give identical rows. | `argus-daemon/tests/index_shapes.rs::live_and_index_reads_have_identical_shapes` |

## Known limitations, stated so they are decisions

- File change detection is by size and mtime from a periodic scan (10 s idle) plus the daemon's own writes; a file watcher is a later refinement. An outside edit to the *loaded* scene updates the index when its scene is re-indexed from disk, but the live world keeps its state until `scene.load`.
- Deleting entity rows by the `root` secondary index measured 100x slower than by primary key on a `WITHOUT ROWID` table, so trees are deleted by key; the comment in `delete_entity_tree` records it.
- The index holds authored content only. Runtime entities exist in the live world and never here.
- Values are stored as JSON text; numeric comparisons parse on the fly. Fine at Hollow's scale; a typed column is the next step if queries over unloaded scenes become hot.
