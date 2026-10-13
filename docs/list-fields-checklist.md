# List fields: checklist

Kevin, 2026-10-13: a def or component field is a scalar (GitHub issue 22). Bring Your Own Game Master keeps every list as a space-separated string: tags, a room kind's prop and monster pools, an item's loot floors, the order conditions came in. A key misspelt in a pool passes the load and fails only when a seed draws it; `Conditions` has a field per condition, so a mod can add a condition's def row but not its field; the order things came in is kept in another string by hand; and a mod cannot add one tag to a row without writing the whole string again.

## 0. The type

- `field.list { of = "ref", default = { ... }, doc = ... }` declares a list field, in a component or a def table. `of` is any scalar type but `entity`: `bool`, the integers, `f32`, `f64`, `vec2`, `str`, `ref`. A list keeps its order and may repeat an element.
- A list is one value, as a string is: a component row holds a 4-byte handle to it in the world's string table, so rows stay fixed-size, and snapshots, saves, keyframes, the digest, and parallel batches treat it as they treat a `str`. A list has at most 1,024 elements.
- `range` and `tune` are refused on a list. A list field is not available to a Rust plugin's `component!` (the plugin is refused, saying so).

## 1. Files and the protocol

- A list is a TOML array in entity files, prefabs, and def rows: `pool = ["monster:rat", "monster:bat"]`; a `vec2` element is `[x, y]`. An element of the wrong type is an error at its file and line.
- Every element of a `ref` list must name a row that exists, checked when the defs, scenes, and prefabs load, as a `ref` field is: a misspelt pool key is a load error, not a script error in some seed.
- The protocol's JSON is an array (`world.query`, `edit.set`, `world.describe` says `list of ref`); release packs carry lists in their binary rows.

## 2. Mods

- A mod's def row replaces a list it gives (`tags = [...]`), as it does any field; `"+tags" = [...]` adds elements to the end of the row's list, and `"-tags" = [...]` takes each one out. So a mod adds one tag to a row without writing the rest.

## 3. Scripts

- Reading a list field gives a read-only array (`for _, t in ipairs(row.tags)`); `table.find`, `#`, and `ipairs` work. Changing it is assigning a new one: `c.order = { "oiled", "burning" }`, or `local o = table.clone(c.order); table.insert(o, "wet"); c.order = o`. Writing into the array read is the read-only error, so a change that would be lost is loud.
- An event may carry a list field (`q:emit("ruled", { tags = { ... } })`), and a def row's lists are read-only like the rest of it.

## 4. Docs

- The scripting chapter (declaring a list, reading and changing one), content (list fields in def rows, `+`/`-` in mods), the reference (`field.list`), the file formats (TOML arrays), and the agent docs.

## Gate

- Tests for each part: a component list read, assigned, kept through a save, a keyframe, and a seek, and in the digest; a def list of refs with a misspelt key refused at its line; a mod's `+tags` and `-tags`; lists in the protocol, an entity file, and a release pack.
- Then Bring Your Own Game Master's own change (pools, tags, and condition order as lists) is the game's to do.

**Built (2026-10-13).** `FieldType::List(ListOf)` in argus-ecs: a list is one value, written as one string in the interner (strings length-prefixed, floats as bits), so rows, snapshots, saves, keyframes, the digest, and parallel batches treat it as a `str`. `field.list { of, default, doc }`; TOML and JSON arrays; every element of a list of refs checked at load, at the field's own line (def rows now keep each field's line); a mod's `"+field"` / `-field`; read-only arrays in Luau, assigned whole; events carry arrays as lists; the editor types a list as JSON. Tests: `crates/argus-ecs/tests/lists.rs`, `crates/argus-luau/tests/lists.rs`, `crates/argus-daemon/tests/lists.rs`, `crates/argus-player/tests/release_lists.rs`. Docs: scripting (List fields), content, releasing, both references. A missing ref in a def row is now reported at its field's line, not the row's header.
