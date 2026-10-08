# argus-tiles: spec trace

Every statement in the design document (v0.3) that constrains tilemaps as phase 1 item 5 scopes it (§12 "Tilemaps", §4 "Bulk authored data", §5 protocol and transactions, §17 the Hollow target, §18 phase 1), with where it lives and the test that pins it. Statuses as in the other traces: **Done**, **Partial**, **Deferred**.

## §12 Tilemaps

| Spec | Status | Where | Test |
|---|---|---|---|
| Layers of tiles over a grid. | Done | A map is a `*.tilemap.toml` index: `width`, `height`, `tile_size` (world units per tile), `cell` (bytes per cell: 1, 2, or 4), and `[[layers]]` with `name`, `tileset`, `z`, and `collision`. Tile `0` is empty; tile `n` is frame `n - 1` of the layer's tileset, which is a sprite asset from item 4. Rows go down, as a tile editor shows. | `chunks_are_sparse_lazy_saved_incrementally_and_checked` |
| Auto-tiling rules declared in TOML. | Done | `[autotile.<terrain>]` in the tileset's sidecar (`argus-assets` `sidecar.rs`): `mode = "single"` (one tile), `"4bit"` (16 tiles by the N=1, E=2, S=4, W=8 edge mask), or `"blob47"` (47 tiles, the eight-neighbour mask with each corner kept only when both its edges connect, in ascending mask order); `edges = "connect"` or `"open"` says whether the map's border counts as the same terrain. `paint` fills a rect with a terrain (or erases it) and re-picks every tile of that terrain in the rect grown by one, so the neighbours of the edit update too. The import checks the tile count for the mode and that every tile id is a frame of the sheet. | `four_bit_auto_tiling_picks_each_tile_from_its_neighbours`, `blob47_uses_the_eight_neighbours_with_corners_only_between_edges`, `argus-assets` `importer::the_sidecar_carries_what_aseprite_and_tiled_carry` |
| Collision derived from tile properties. | Done | A tile's properties are its frame's properties in the sidecar (`[[frames]] index = 15, properties = { solid = true }` or `one_way = true`, the Tiled shape). `collision(map, importer, rect)` combines every layer marked `collision = true` into one grid of `Empty`, `Solid`, or `OneWay`; solid beats one-way. A collision layer with no tileset is all solid wherever it has a tile, which is what a hand-made collision layer wants. `scene.tiles.collision` shows it as rows of `.`, `#`, and `-`. Item 6's kinematic controller is the customer. | `four_bit_auto_tiling_picks_each_tile_from_its_neighbours`, `argus-daemon/tests/tiles.rs` |
| Chunked storage (§4). | Done | Beside the index, a `<stem>.chunks/` folder holds `<layer>/<cx>_<cy>.bin` (little-endian cells, `chunk × chunk` of them) and `manifest.txt` (one line per chunk: layer, cx, cy, hash). Chunks load when first read or written; a chunk that is all empty is never written, and saving one that became empty deletes it. A save rewrites only the dirty chunks, then the manifest, then the index, whose `hash` is the manifest's. On open the manifest is checked against the index, and each chunk against its size and hash when it loads (`MAP_MISMATCH`). | `chunks_are_sparse_lazy_saved_incrementally_and_checked` |
| Editable through the protocol at the rect level, so an agent can carve a room without touching pixels. | Done | `edit.tiles {tx, file, layer, rect, ...}` with exactly one of `cells` (row-major, `w × h` of them), `tile` (fill), `terrain` (auto-tiled fill), or `erase`. It is a transaction op like `edit.set`: see §5 below. `scene.tiles.create` makes a map; `scene.tiles.info` shows its index, each layer's terrains, and whether the tileset was found. | `argus-daemon/tests/tiles.rs::a_room_is_carved_at_the_rect_level_undone_and_drawn` |
| A human can paint in the editor and both land in the same chunk. | Done | There is one write path, `edit.tiles`, and the editor (phase 4 item 7, its Tiles tab) is a protocol client. Every save rewrites the index's hash, so the daemon reopens a map when its index's modification time changes, and the next edit applies to what the other writer saved. A failed commit puts back the ops it already applied, on disk too. | `argus-daemon/tests/tiles.rs`; `argus-editor/tests/tiles.rs` (the agent's carving and a person's stroke in one chunk) |
| Hollow and Outpost are the customers. | Done | The phase-1 gate's three-room Hollow slice is the first real map. Maps sit in a flat scene's folder or in a room's folder; the scene loader lists them with the room's bulk files and never reads them as entities. Outpost's navigation grid derived from the tilemap is phase 3. (finished: argus-nav builds a grid over a tilemap, its trace's first row; Outpost keeps no tilemap, Kevin's decision of 2026-10-04 in docs/phase4-checklist.md, item 7) | `argus-scene` `defs_bulk::bulk_chunks_round_trip_and_check_their_hash` |

## §4 Bulk authored data

| Spec | Status | Where | Test |
|---|---|---|---|
| A text index file plus binary chunks; the index carries everything a reader needs to reason about the data (dimensions, tileset, layer names, a content hash). | Done | The `*.tilemap.toml` index is plain TOML with the dimensions, the layers with their tilesets, the chunk folder, and the hash. A save changes only the hash line, so a comment written in the index survives. Layer names and the chunk folder become paths, so each must be one plain folder name; an index edited to point elsewhere is refused on open. The phase-0 `*.map.toml` format stays for other bulk data and still answers `scene.tiles.get`. | `chunks_are_sparse_lazy_saved_incrementally_and_checked` |
| The agent reads the index and queries the chunk through the protocol (`scene.tiles.get {rect}`); it never sees a 40,000-line array. | Done | `scene.tiles.get` serves tilemaps and phase-0 bulk indexes alike: one layer or all layers, a rect of cells. `scene.tiles.info` never loads a chunk. | `argus-daemon/tests/tiles.rs` |
| Path references, not GUIDs. | Done | A `Tilemap` component names its map by project path (`map = "scenes/hollow/rooms/a/floor.tilemap.toml"`); a layer names its tileset by asset name (`tileset = "tiles/rock"`). | `argus-daemon/tests/tiles.rs` |

## §5 Protocol and transactions

| Spec | Status | Where | Test |
|---|---|---|---|
| Human and agent edit the same world through the same transactions. | Done | `edit.tiles` queues an `Op::Tiles` (cells) or `Op::Terrain` (a terrain or an erase) in the open transaction; `edit.commit` applies the ops in order, saves the touched maps, and reports `touched` as `file#layer` and `files` as the chunk files written. A bad op (an unknown terrain, a rect outside the map) fails the commit whole and names the terrains there are. | `argus-daemon/tests/tiles.rs` |
| Every committed transaction records its inverse; undo and redo are scoped per author. | Done | The inverse of a tile op is an `Op::Tiles` of the old cells over the area the op changed (the rect grown by one for a terrain paint). `edit.undo {author}` unwinds only that author's work. | `argus-daemon/tests/tiles.rs` |
| The change feed reports what changed. | Done | Commit records in the feed carry the ops as `["tiles", file, layer, rect]` and `["terrain", file, layer, rect, terrain]`. | `argus-daemon/tests/tiles.rs` |

## §17 Performance

| Spec | Status | Where | Test |
|---|---|---|---|
| Hollow tilemap: 4,096 × 4,096 tiles, 8 layers. Chunked storage, rect edits, and auto-tiling at that size without pauses. | Done | Measured in release on the development laptop (numbers below). The map is never read whole: an edit loads the chunks it touches, a save writes the chunks it changed, and drawing reads only the chunks in view. | `perf_4096_square_8_layers_without_pauses` (ignored; run with `--release -- --ignored`) |

| Operation, 4,096 × 4,096 × 8 layers, 64-tile chunks | Time |
|---|---|
| Create the empty map | 1.0 ms |
| A 64 × 64 auto-tiled brush stroke plus save, worst of a run of strokes | 4.8 ms |
| A 1,024 × 1,024 fill plus save (256 chunks) | 85 ms |
| The tiles in one screen's view (3,680 tiles) | 0.18 ms |
| Reopen with 324 chunks on disk | 3.8 ms |

## Phase 1, item 6: what the physics step reads

| Spec | Status | Where | Test |
|---|---|---|---|
| Collision derived from tile properties, with slopes (§12 "slopes" in the kinematic tier). | Done | `Collide::Slope { left, right }` from a tile's `slope = [left, right]` property (a malformed one is solid, and the importer warns); `Collide::rank` settles layers that disagree (solid, then slope, then one-way). `scene.tiles.collision` shows slopes as `/` and a backslash. | `the_store_reads_slopes_and_notices_a_map_saved_elsewhere` |
| The tick sees the chunk an edit wrote (§12 "both land in the same chunk"). | Done | `MapStore` holds the session's open maps; the daemon edits, draws, and answers `scene.tiles.*` through it, and the physics step reads collision from it, through per-tileset tables rebuilt when tilesets re-import. `open` reopens a map whose index another process saved. | same, `argus-daemon/tests/physics.rs` |

## §12 Rendering and the world

| Spec | Status | Where | Test |
|---|---|---|---|
| A tilemap is drawn with the sprites. | Done | A `Tilemap` component places a map; its entity's `Transform` is the top-left corner of tile (0, 0), and tile (c, r) is centred at (x + (c + 0.5)·size, y − (r + 0.5)·size). Each frame the daemon asks `visible` for the tiles the camera sees and passes them to the renderer as extra sprites (`Renderer::render_with`), drawn from the tileset's atlas with the layer's `z`. A missing tileset draws magenta and is listed in `missing_atlases`. `visible = false` hides a map. | `visible_tiles_are_the_ones_the_view_covers`, `argus-daemon/tests/tiles.rs` |

## Phase 1, item 12: tiles in the semantic frame

| Spec | Status | Where | Test |
|---|---|---|---|
| The semantic frame says what the player sees (§8 L4), and for a platformer that is mostly the ground. | Done | `MapStore::collision_rows` gives a map's collision inside a world rect, one character a cell (`Collide::ch`), top row first, taking every `step`th cell past a size. The daemon puts it in the frame for every tilemap in view. | `argus-daemon/tests/semantic.rs` |

## Phase 3, item 2: what a navigation grid reads

| Spec | Status | Where | Test |
|---|---|---|---|
| (§12) A navigation grid derived from the tilemap follows its edits. | Done | `MapFile::revision` moves whenever a cell changes (and when the map is read or made) and never repeats in a process; `MapStore::revision` adds the tileset tables'. A grid over the map is built again when they move. | `argus-nav` `nav::a_grid_covers_a_tilemap_and_follows_its_edits` |

## Phase 5, item 4: runtime maps

| Spec | Status | Where | Test |
|---|---|---|---|
| (§2 Delve, "procedural generation"; §8 "the semantic frame carries the map") A map a game makes each run. | Done | `runtime = true` in a tilemap's index: its layers, tileset, size, and collision are authored there; its cells are the run's, empty at the start, written by the game's scripts (`q:tiles`, argus-luau), drawn, collided with, navigated, and read by the semantic frame as any map's. Nothing writes them to disk: `save` refuses, and an edit is refused before a cell changes. `MapFile::in_memory` makes a map with no index file too (a Tiled map read into this form). `MapStore::runtime_cells` and `set_runtime_cells` are what a keyframe, a replay, and a save keep of them (`encode_cells`, `decode_cells`), by layer name, so a save outlives a layer moved in the index. | `argus-daemon` `runtime_map` (written each tick from the seed; a seek back, the replay on, and a save and load all give the same cells; no chunk folder; an edit refused, keeping nothing); Delve's floors (`examples/delve/tests/map.luau`, `argus-player` `release_delve_floor`) |

## Phase 5, item 4: Tiled maps

| Spec | Status | Where | Test |
|---|---|---|---|
| (§12) "A Tiled map becomes the engine's map and its layers." | Done | `tiled.rs`: `MapStore::open` reads a `.tmx` or `.tmj` with argus-assets' `tiled::read_map` (the reading the importer uses too) into a `MapFile::in_memory`. Each Tiled tile layer is a layer, named as in Tiled; a group's layers are flattened as `group/layer`, with the group's offset (in whole tiles, rounded with a warning otherwise), visibility, and custom properties. A layer drawing from several tilesets is split into one layer per tileset, `name (tileset)`, since an engine layer has one tileset. A layer's `z` property is its z, else its place among the map's tile layers (0, 1, 2, ...); `collision = true` makes it a collision layer. Its tileset is the sprite asset its tiles come from (an external `.tsx` by its path, an embedded tileset by the sprite the importer makes of it), and a cell is the tile's id in its own tileset (the GID less its firstgid, plus one), so tile `n` is frame `n - 1` as on every map. Flip and rotation flags are dropped (the engine draws tiles unflipped), with a warning once a layer. The map's `tile_size` property is world units per tile, else 1. An infinite map's grid is the bounds of its chunks, and its info says which Tiled cell is the engine's (0, 0). A map with no tile layers is refused with why. | `tiled::a_tiled_map_is_the_engines_map_and_collides_by_its_tilesets_properties`; argus-assets `tiled::*` |
| Drawn, collided with, navigated, and in the semantic frame, wherever the engine runs. | Done | Everything reads maps through `MapStore`, so `Tilemap { map = "assets/maps/level1.tmx" }` is drawn (`visible`), collided with by the physics step, navigated (argus-nav reads its collision), shadows light, and is in the semantic frame's `tiles`, in the daemon, the player, a release build, and the web page, since it is read through argus-os's file system, which a mounted pack serves. A layer Tiled hides is not drawn (`MapFile::layer_visible`) and still collides when it is a collision layer: Tiled's usual hidden collision layer. | `tiled::*`; `argus-daemon/tests/tiled.rs` (a body lands on its floor; an offscreen frame has its tiles' colours and none of the hidden layer's; the semantic frame's rows); argus-player `release_tiled` |
| A Tiled map is Tiled's to edit. | Done | `MapFile::tiled` marks a map read from Tiled, and `MapFile::set` refuses it; the daemon refuses `edit.tiles` on one when it is queued and, at commit, before a cell changes, with the hint "This map is Tiled's: edit it in Tiled"; `save` refuses an in-memory map. The editor paints through `edit.tiles`, so a stroke on a Tiled map shows that refusal in its problem bar and changes nothing. A save in Tiled moves the file's modification time and the store reads it again, as it reopens an index another process saved; its revision moves, so a navigation grid is built again. | `tiled::a_tiled_map_is_tileds_to_edit_and_a_save_in_tiled_is_read_again`, `argus-daemon/tests/tiled.rs` |
| Its structure through the protocol. | Done | `scene.tiles.info` (and each row of `scene.tiles.list`) gives every layer's `visible`, and `tiled`: what the file holds (its tilesets with firstgid and tile count, Tiled's layers and the engine layers each became, its object layers' objects) and what did not carry over; `scene.tiles.get` and `scene.tiles.collision` read it as any map. | `argus-daemon/tests/tiled.rs` |

## Known limitations, stated so they are decisions

- A big fill is a big edit. A 1,024 × 1,024 fill touches 256 chunks and takes 85 ms including the save; brush-sized edits stay well under a frame.
- Tiles are drawn as extra instances built on the CPU each frame, not cached per chunk on the GPU. At a screen's worth of tiles this costs 0.18 ms; a per-chunk vertex cache is the change if a zoomed-out view ever makes it matter.
- Auto-tiling picks from a fixed table per mode. Random variants of the same mask and terrains that blend into each other are not in; a sidecar field for each is the natural extension.
- The room document's `tileset` field (§4 `room.toml`) is not used as a default: each map layer names its own tileset.
- `asset.move` on a tilemap index moves the index file only, not its chunk folder. Moving maps is rare, and a tilemap-aware move is a small addition when it is needed.
- Entity ids `scene` and `room`, and ids ending `.map` or `.tilemap`, are refused, because the scene loader reads those file names as documents or data.
- A Tiled map is read whole into memory when it is opened, and again whenever Tiled saves it, not chunk by chunk as the engine's own maps are; it has no chunk folder to read lazily. Nothing in the engine writes it, scripts included: a game that changes its level at run time keeps those cells in a runtime map of its own.
- A Tiled map's object layers are data (`scene.tiles.info`, `asset.get`), not entities: the engine places nothing from them. A tile's animation does not play on a map layer.
