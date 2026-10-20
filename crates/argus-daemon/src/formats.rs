//! The reference for a project's files and the engine's own components
//! (§4, §16): what each file is for, where it lives, every key it takes
//! with its type, default, and meaning, a real example, and what the
//! engine says when it is wrong; then every component the engine
//! registers, with its fields, generated from the engine's registry.
//! `argus docs` prints both; each project's ARGUS.md prints the files.
//!
//! Found while four agents wrote game templates from the example games:
//! each learned the file formats by reading the examples and the engine's
//! parsers, because nothing described them. The tests read each parser's
//! source, as the scripting reference's read the Luau binding, so a key a
//! parser starts to accept without a line here fails CI.

use argus_ecs::{FieldType, Value, World};
use std::fmt::Write as _;

/// One key of a file.
pub struct Key {
    /// How it is written: `name`, `[rooms] start`, `[[frames]] properties`.
    pub name: &'static str,
    /// Its TOML type.
    pub ty: &'static str,
    /// Its value when the file leaves it out ("required" when it may not).
    pub default: &'static str,
    /// What it means.
    pub doc: &'static str,
}

/// One kind of file.
pub struct FileDoc {
    /// Its heading: where it lives.
    pub title: &'static str,
    /// What it is for, how the engine finds it, and its rules.
    pub intro: &'static str,
    /// Its keys.
    pub keys: &'static [Key],
    /// Where the example comes from.
    pub example_from: &'static str,
    /// A short real example.
    pub example: &'static str,
    /// What the engine says when the file is wrong.
    pub errors: &'static str,
}

const fn k(name: &'static str, ty: &'static str, default: &'static str, doc: &'static str) -> Key {
    Key {
        name,
        ty,
        default,
        doc,
    }
}

/// What comes before the files: the layout, and how values are written.
pub const INTRO: &str = "A project is a directory with an `argus.toml`. Everything in it is a file the engine reads: TOML for data, Luau for code (`components/`, `systems/`, `defs/*.luau`, `lib/` for the modules they `require`, and `tests/`: the scripting and scenario references), and the assets' own formats. `.argus/` is the engine's (the index, keyframes, marks, saves, logs): never edit it, and keep it out of version control. Every problem with a file is reported with a code, a message, a hint, and the file and line (`ref`): a scene's in `argus scene issues`, a script's in `argus script errors`, an asset's in `argus asset list`, and `argus.toml`'s when the project opens. A scene with any issue does not spawn, so a run is always of files that read.";

/// How a component field's value is written in an entity or prefab file.
pub const VALUES: &str = "A component table's keys are its fields, and each value is written as its field's type wants: `bool` as true or false; an integer type (`u8` to `u64`, `i8` to `i64`) as an integer, or a float with no fraction (0.5 is refused, not cut); `f32` and `f64` as a number; `vec2` as `[x, y]`, two numbers; `str` as a string; `ref` as a string naming a def row (`\"item:bread\"`) or a project path (a prefab, `\"door/locked\"`, or a file, `\"assets/tiles.png\"`), `\"\"` for none; `entity` as `\"@id\"`, the id of another entity of the scene (`\"@room/id\"` for one in another room, `\"@id/child\"` for a child), `\"\"` for none; a list (`list of str`, `list of ref`, ...) as an array of its elements, each written as its type is (`[\"oiled\", \"burning\"]`, `[[1, 2], [3, 4]]` for vec2s), every ref in it naming something that exists. A field left out keeps its default (`argus describe <Component>` lists them), and a field with a range must be inside it. A component's whole table is flat: a field's value is never a table.";

/// Every kind of file, in the order a project is read.
pub const FILES: &[FileDoc] = &[
    FileDoc {
        title: "argus.toml: the project",
        intro: "At the project's root; the directory holding it is the project (`argus` finds the nearest one upward from where it runs, or `--project <dir>`). Read when the project opens, before anything else. Keys it does not know are ignored.",
        keys: &[
            k("name", "string", "required", "The game's name: the window's title, the folder its saves and players' reports go in on a player's machine (`%APPDATA%/<name>`, `~/.local/share/<name>`), and the name a release build and a mod's `game` carry."),
            k("clock", "string", "\"continuous@60\"", "The sim clock every scene runs on unless its `scene.toml` says otherwise: `continuous@<hz>` (a fixed step, any positive rate) or `discrete` (turns: the sim advances only when asked)."),
            k("default_scene", "string", "none", "The scene `session.open` (and so `argus daemon start` and every tool) opens when none is given: a directory name under `scenes/`."),
            k("plugins", "array of strings", "[]", "Rust plugins the game uses: each a crate in `plugins/<name>/`, built and loaded when the project opens (see `plugins/<name>/` below)."),
            k("modules", "array of strings", "[]", "The engine's optional modules the game uses: `rigid` (rigid bodies and joints on Rapier: RigidBody, Joint, RigidWorld) and `steam` (a release build runs on Steam when it is there; `platform.toml`). The daemon installs only these and a release build carries only these, so the two agree."),
            k("[ring] every", "integer, at least 1", "one second of ticks (continuous), ten turns (discrete)", "Ticks between the keyframes `seek` rebuilds frames from."),
            k("[ring] budget_mb", "integer, at least 1", "1024", "Memory the keyframes may use before the ring keeps fewer of the old ones."),
            k("[marks] key", "string", "\"F8\"", "The key that drops a mark while playing, by the name the window sends (`F8`, `Backquote`, `KeyM`)."),
            k("[time] day", "number above 0", "86400", "Sim seconds in one of the game's days: what a script's `q.day`, `q.day_time`, and `q.day_length`, and a scenario's `days()` and `hours()`, count in."),
            k("[strings] base", "string", "\"en\"", "The language the game is written in: its string table (`strings/<base>.toml`) is the one every other language falls back to."),
            k("[mods] enabled", "boolean", "true", "Whether the game takes mods (`mods/` below). false: a session asked for mods is refused, and `argus build --mod` too."),
            k("[reports] url", "string", "none", "The playtest server a desktop build sends its players' reports to (its `http://` address; the build speaks plain HTTP, so a LAN or tailnet address). Without it reports stay in the player's save folder."),
            k("[game] title", "string", "the name", "The game's title as a player reads it: the window's, the web page's, the Mac app's name, and the Windows executable's product name."),
            k("[game] version", "string", "\"1.0\"", "The game's own version, up to four numbers with dots (`1.2.0`): on the Windows executable and the Mac app."),
            k("[game] icon", "string", "none", "A sprite asset whose frame is the game's icon: a PNG in `assets/`, or an art script's sprite. `argus build` puts it where each platform looks: in the Windows executable, on the window, in the Mac app, beside a Linux executable, and on the web page and in its manifest (a phone's home screen). Square is best; a frame that is not sits in the middle of a clear square. It is scaled up by whole pixels, so pixel art stays sharp. Without it the game has no icon of its own."),
            k("[game] icon_frame", "integer, at least 0", "0", "The frame of that sprite."),
            k("[build] exclude", "array of strings", "[]", "Paths a build leaves out of the packs, as globs on the path from the project's folder: `m0/**` leaves out a folder and all in it, `**/*.psd` every Photoshop file, `notes.md` one file. Beside these, a build leaves out `tests/`, `tools/`, `dist/`, `mods/`, `plugins/`, the notes at the root, anything whose name starts with a dot, and what git ignores in a repository. The build's report lists what it left out, and why."),
        ],
        example_from: "examples/outpost/argus.toml",
        example: "name = \"outpost\"\nclock = \"continuous@60\"\ndefault_scene = \"valley\"\nplugins = []\nmodules = [\"steam\"]\n\n[time]\nday = 60\n\n[game]\ntitle = \"Outpost\"\nicon = \"colonist\"\nicon_frame = 0",
        errors: "With no `name`: \"argus.toml needs `name`\" (4605). A `[game]` key not listed above, a `version` that is not up to four numbers with dots, or an `icon_frame` below 0: 4605, at the table. A `[game] icon` that names no sprite, or a frame the sprite does not have, is refused by `argus build`, which says what the sprite has. A clock that is neither `discrete` nor `continuous@<hz>`: \"bad clock spec\" (4407). A module the engine does not have, or a `[ring]`, `[marks]`, `[time]`, `[strings]`, `[mods]`, `[reports]`, or `[build]` value of the wrong kind: 4605 naming the key and what it must be. Bad TOML: 4601 with its line. The project does not open until it is fixed.",
    },
    FileDoc {
        title: "scenes/<scene>/scene.toml: a scene",
        intro: "Each directory under `scenes/` with a `scene.toml` is a scene, named after the directory. A flat scene keeps its entity files beside `scene.toml`; a room scene keeps them in `scenes/<scene>/rooms/<room>/`, each room with a `room.toml` (below), and streams rooms in and out around one entity. Keys it does not know are ignored.",
        keys: &[
            k("clock", "string", "argus.toml's clock", "This scene's clock: `continuous@<hz>` or `discrete`."),
            k("seed", "integer, at least 0, or \"random\"", "\"random\"", "The run's seed, which every random stream is drawn from: a fixed seed replays exactly; `\"random\"` lets the runner pick one and record it. `session.open` and `scene.load` take a `seed` that wins over it."),
            k("bounds", "array of 4 numbers", "none", "A flat scene's rect `[x0, y0, x1, y1]` in world units: a following Camera with `bounded = true` keeps its view inside it. A room scene's rooms have their own."),
            k("[systems] enable", "array of strings", "[] (every system runs)", "When not empty, only the systems matching one of these patterns run, the engine's included: a system's name (`sim.player`), `prefix*` (`sim.*`), or `*`. The rest are disabled with the reason, as `argus describe systems=true` shows."),
            k("[systems] disable", "array of strings", "[]", "Systems matching these patterns are disabled (after `enable`)."),
            k("[rooms] start", "string", "none", "A room scene's first room: where loading begins when nothing is followed."),
            k("[rooms] radius", "integer, at least 0", "1", "Rooms within this many steps of the current room in the graph stay loaded; farther ones stream out, remembering what happened in them (a taken gem stays taken)."),
            k("[rooms] follow", "string", "none", "The entity rooms stream around (`\"@player\"`): the current room is the one whose bounds hold it. It must be in a room listed in `always`, and every other room needs bounds."),
            k("[rooms] always", "array of strings", "[]", "Rooms that never stream out: the followed entity's, with the camera, the HUD, and the menus."),
            k("[rooms] transition", "string", "none", "A timeline played at every room change, its actor the followed entity (`\"timeline:rooms.fade\"`, a row of `defs/timeline.toml`)."),
            k("[rooms.graph] <room>", "array of strings", "none", "The room graph: each key a room, its value the rooms it joins (edges go both ways, so each needs writing once). Each room's `room.toml` `neighbors` must agree."),
        ],
        example_from: "examples/hollow/scenes/hollow/scene.toml",
        example: "clock = \"continuous@60\"\nseed = 1\n\n[rooms]\nstart = \"a\"\nradius = 2\nfollow = \"@player\"\nalways = [\"persistent\"]\ntransition = \"timeline:rooms.fade\"\n\n[rooms.graph]\n\"a\" = [\"b\", \"deep-0-0\"]\n\"b\" = [\"a\", \"c\"]\n\"c\" = [\"b\"]",
        errors: "A missing `scene.toml`: \"cannot read\" (4602) with the scenes the project has. A `seed`, `clock`, `bounds`, `radius`, or list of the wrong kind: 4605 (4407 for the clock) at its line. `[rooms]` with no `rooms/` directory, a `follow` that names no entity or one outside `always`, rooms without bounds while something is followed, or a graph and `neighbors` that disagree: scene issues (4605). Issues stop the scene spawning.",
    },
    FileDoc {
        title: "scenes/<scene>/rooms/<room>/room.toml: a room",
        intro: "Every directory under a scene's `rooms/` is a room, named after the directory, and needs a `room.toml`. Its entity files are beside it, and their full ids are `<room>/<file stem>`. Keys it does not know are ignored.",
        keys: &[
            k("bounds", "array of 4 numbers", "none", "The room's rect `[x0, y0, x1, y1]` in world units: the followed entity is in the room whose bounds hold it, and a following Camera with `bounded = true` keeps its view inside the current room's. A room listed in `[rooms] always` may have none."),
            k("neighbors", "array of strings", "[]", "The rooms this one joins, as the scene's `[rooms.graph]` says (a mirror of it, checked). A mod's own room joins the graph through its `neighbors`."),
            k("tileset", "string", "none", "A tileset path, kept in the scene's index (`argus scene rooms`); the engine draws nothing from it (a Tilemap entity draws a room's tiles)."),
        ],
        example_from: "examples/hollow/scenes/hollow/rooms/b/room.toml",
        example: "bounds = [24.0, 0.0, 44.0, 24.0]\nneighbors = [\"a\", \"c\"]",
        errors: "A room directory without a `room.toml`: a scene issue (4602, \"Every room directory needs a room.toml\"). Bounds that are not four numbers: 4605. A `neighbors` list that disagrees with the graph: 4605 (\"room.toml neighbors are a mirror and must match\").",
    },
    FileDoc {
        title: "scenes/<scene>/<id>.toml: an entity",
        intro: "One authored entity per file, in a flat scene's directory or a room's. The file name without `.toml` is its id (`player.toml` is `@player`, `door.east.toml` is `@door.east`; in a room scene the full id is `<room>/<id>`, and `@id` from inside the same room finds it). The stems `scene` and `room`, and files ending `.map.toml`, `.tilemap.toml`, and `.navgrid.toml`, are not entities. Every top-level key that is not one of the keys below is a component: a `[Component]` table whose keys are field values (see how values are written, above). An empty table adds the component with its defaults; a component a file does not name is not on the entity. A tag in `tags` that no script declared becomes a tag component by being used. Change these files with `edit` (or `argus edit`), so the change feed records who and why and undo works; A change made to one on disk is taken in when the scene loads again (`argus scene load`); `reload` takes scripts and assets, not entity files.",
        keys: &[
            k("prefab", "string", "none", "The prefab this entity starts from: its path under `prefabs/` without `.toml` (`\"door/locked\"`). The file's own values override the prefab's field by field (prefabs, below)."),
            k("tags", "array of strings", "[]", "Tags: zero-size components, `q:each(\"enemy\")` and `where {has: [...]}` find them."),
            k("-tags", "array of strings", "[]", "Tags the prefab has that this entity drops."),
            k("[Component]", "table", "", "A component, with field overrides as its keys: `[Transform] pos = [3.0, 1.0]`."),
            k("-Component", "true", "", "Remove a component the prefab has: `-Collider = true` (must be true; removal is written out so a reader sees it)."),
            k("[children.<name>.<Component>]", "table", "", "A child entity named `<name>`, with its components (and its own `prefab`, `tags`, and `children` as `[children.<name>]` keys). Its id is `<id>/<name>` (`@pause/panel`); it is given a Parent pointing at this entity, and its Transform is local to it. Children nest: `[children.panel.children.title.UiText]`."),
            k("-children", "array of strings", "[]", "Children the prefab has that this entity drops."),
            k("[Parent] entity", "string (\"@id\")", "", "Hang this entity under another entity of the scene (a UI node under its menu, as Delve's controls): its Transform becomes local to that entity (the scene gives a parented entity with a Transform a GlobalTransform, which the hierarchy fills each tick); when the parent is despawned the link is cleared. A child in `[children]` has its Parent already."),
        ],
        example_from: "examples/delve/scenes/delve/player.toml",
        example: "tags = [\"player\"]\n\n[PlayerSlot]\nslot = 1\n\n[Actions]\n\n[Transform]\npos = [0.0, 0.0]\n\n[Health]\ncur = 10\nmax = 10\n\n[Sprite]\natlas = \"sprites/hero\"\nsize = [1.0, 1.0]\nz = 5.0",
        errors: "Each is a scene issue at its file and line, and the scene does not spawn: a component no script or the engine declares, \"no component named\" (4001, with a hint to `world.describe`); a field it does not have, \"has no field\" (4002, listing its fields); a value of the wrong type, \"expected a <type> value\" (4003); a value outside a field's range (4102); a field whose value is a table (4605, \"Fields are flat\"); a top-level key that is not a table (4605, \"components are tables\"); `-Component` not true (4605); an `@id` that names no entity (4603, \"Entity refs are @id, or @room/id across rooms\"); a `prefab` or a `ref` that names nothing (4603: when the moves log knows where it went, the hint says so).",
    },
    FileDoc {
        title: "prefabs/<path>.toml: a prefab",
        intro: "A prefab is an entity file kept under `prefabs/`, named by its path without the directory and extension (`prefabs/door/locked.toml` is `\"door/locked\"`), in subdirectories as deep as you like. It takes every key an entity file does, `prefab` included, so one prefab can start from another. An entity (or a child) with `prefab = \"door/locked\"` gets everything the prefab chain gives, then its own file: inheritance is field by field, never component by component, so `[Transform] pos = [1, 2]` in the entity keeps the prefab's `rot` and `scale`. An entity adds components and tags the prefab lacks, and removes the prefab's with `-Component = true`, `-tags`, and `-children`; a child the prefab declares is overridden at `[children.<name>.<Component>]`. A prefab spawns nothing by itself. `argus describe` and `world.prefabs` list them with their instances; `world.get` with `resolved: true` shows where each value came from (own, a prefab, or the default).",
        keys: &[
            k("prefab", "string", "none", "Its own base prefab."),
            k("tags, -tags, [Component], -Component, [children...], -children", "", "", "As in an entity file."),
        ],
        example_from: "`examples/swarm/prefabs/pause_menu.toml` and `examples/swarm/scenes/arena/pause.toml`",
        example: "# prefabs/pause_menu.toml\n[UiRoot]\nlayer = 10\n\n[UiNode]\njustify = \"center\"\nalign = \"center\"\nvisible = false\n\n[children.panel.UiNode]\npad = [28.0, 22.0]\ngap = 10.0\n\n# scenes/arena/pause.toml: the whole menu\nprefab = \"pause_menu\"",
        errors: "A prefab file that does not parse is reported when the project opens (4601 or 4605) and again by each scene that uses it. A `prefab` naming no prefab: 4603, \"missing reference: prefab ... does not exist\". A prefab that is its own base through any chain: 4604, \"inherits from itself\", with the loop. The rest as for entity files.",
    },
    FileDoc {
        title: "input.toml: the player's actions",
        intro: "At the project's root, optional. It names the game's actions and the keys, buttons, and sticks bound to each; gameplay reads actions and never sees a key, so a player can rebind them. Without it the game has no PlayerSlot or Actions components and the `input.*` methods answer 4901. An entity with `[PlayerSlot] slot = 1` and an empty `[Actions]` table gets slot 1's actions written onto it every tick (the input stage's `input.actions`); a system that `reads = {\"Actions\"}` reads them by name: `e:get(\"Actions\").jump_pressed`, `.move.x`. What each action type gives the Actions component: a `button` named `jump` is three bools, `jump` (held), `jump_pressed` (went down this tick), and `jump_released` (went up this tick); an `axis` is one f32 from -1 to 1; an `axis2` is a vec2 no longer than 1; a `pointer` is a vec2, the mouse's place on the UI canvas in its pixels. Actions also has the engine's `device` field: the kind of device the player last used (0 keyboard and mouse, 1 Xbox, 2 PlayStation, 3 Switch, 4 another gamepad, 5 touch), which button prompts follow; `device` cannot be declared. The engine reads some actions by name: a UiRoot's menus use `ui_move` (axis2), `ui_confirm`, `ui_cancel`, `ui_click` (buttons), and `pointer` (pointer) unless its fields name others; a CharacterController uses `move` and `jump` unless `move_action` and `jump_action` name others; a Camera pans and zooms with the actions its `pan_action` and `zoom_action` name. Keys the file does not know are ignored; so is a `type` that is not a string, which makes the action a button.",
        keys: &[
            k("[actions.<name>]", "table", "", "An action. The name is letters, digits, and `_`, not starting with a digit: it becomes a field of Actions. Actions are listed in name order."),
            k("[actions.<name>] type", "string", "\"button\"", "`button`, `axis`, `axis2`, or `pointer`."),
            k("[actions.<name>] doc", "string", "\"\"", "What it does: the doc of its Actions field, shown in ARGUS.md and the controls menu."),
            k("[actions.<name>] toggle", "boolean", "false", "Buttons only: a press turns it on and the next one off, instead of holding (a settings menu's InputToggle changes it for a player)."),
            k("[actions.<name>.bindings] keyboard", "array of strings", "[]", "Keys, by name: letters `a` to `z`, digits `0` to `9`, `space`, `enter`, `escape`, `tab`, `backspace`, `up`, `down`, `left`, `right`, `shiftleft`, `controlleft`, `altleft` (and `...right`), `f1` to `f24`, `minus`, `equal`, `comma`, `period`, `slash`, `semicolon`, `quote`, `backquote`, `bracketleft`, `bracketright`, `backslash`, `numpad0` to `numpad9`, `home`, `end`, `pageup`, `pagedown`, `insert`, `delete`: the window's key code names, matched without case and without a `Key`, `Digit`, or `Arrow` prefix (`KeyW` is `w`, `ArrowUp` is `up`, `Code(KeyW)` is `w`; `esc` is `escape`; `return` and `numpadenter` are `enter`). Not checked: a name no key sends never fires. A button takes any key. An axis takes `\"neg/pos\"` (`\"minus/equal\"`: -1 and 1) or one key (0 or 1). An axis2 takes `wasd` (w, a, s, d), `arrows` (up, left, down, right), `ijkl` (i, j, k, l), or four keys in the order up, left, down, right (`\"i,j,k,l\"`)."),
            k("[actions.<name>.bindings] mouse", "array of strings", "[]", "A button's mouse buttons: `left`, `right`, `middle`, `back`, `forward`, each with any of the modifiers `ctrl`, `shift`, `alt`, `meta` before it (`ctrl+left`, `shift+alt+right`): pressed with them held, it is a binding of its own, and a plain `left` of the same slot stands down meanwhile, so Ctrl and a click is not also a click. The modifier keys need bind nothing, so on a discrete clock holding Ctrl takes no turn. A pointer's: `position`. An axis's: `wheel`, the mouse wheel: 1 (up) or -1 for the tick of a notch, then 0 (the UI's `ui_scroll` scrolls a UiScroll with it). An axis2 takes none."),
            k("[actions.<name>.bindings] gamepad", "array of strings", "[]", "A button's gamepad buttons: `south`, `east`, `west`, `north` (A, B, X, Y on an Xbox pad), `lb`, `rb`, `lt`, `rt` (a trigger past half way), `select`, `start`, `mode`, `lthumb`, `rthumb`, `dpad_up`, `dpad_down`, `dpad_left`, `dpad_right`. An axis's: `left_x`, `left_y`, `right_x`, `right_y`, `lt`, `rt`. An axis2's: `left_stick`, `right_stick`, `dpad`. Sticks have a dead zone of 0.2 on each axis. Other names are refused (4905)."),
            k("[actions.<name>.bindings] platform", "array of strings", "[]", "The platform's own: `overlay`, a button that is held while the platform's overlay (Steam's Shift+Tab) is open."),
            k("[actions.<name>.bindings] touch", "array of strings", "[]", "Touch controls, by their `[touch.<name>]` names: a button's, a `button` control; an axis2's, a `stick`; an axis's, a stick's `x` or `y` (`\"stick.x\"`, up positive). A pointer takes none: a finger that lands on no control is the mouse, its `left` button and the pointer, so a game made for the mouse plays under a finger with no touch bindings at all."),
            k("[touch.<name>]", "table", "", "A touch control, a button or a stick on the screen, which touch bindings name. A finger that lands on it holds it until it lifts (a stick, one finger at a time). The engine draws the controls over the game while touch is the device the player last used (`device` 5), unless `draw = false`. Fingers are recorded as the actions they press, so replays, seeks, and reports have them."),
            k("[touch.<name>] kind", "string", "", "`button`, held while a finger is on it, or `stick`, a drag from its centre: as far as its edge is 1, nothing within a tenth of it."),
            k("[touch.<name>] anchor", "string", "\"center\"", "Where it is placed from on the UI canvas: `top-left`, `top`, `top-right`, `left`, `center`, `right`, `bottom-left`, `bottom`, `bottom-right`."),
            k("[touch.<name>] at", "array of 2 numbers", "[0, 0]", "Its centre from the anchor, inward, in UI canvas pixels: from a left anchor rightward, from a right anchor leftward, from a top anchor down, from a bottom anchor up; from the middle (top, bottom, left, right, and center are at a middle) right and down."),
            k("[touch.<name>] size", "number", "96", "Its diameter in UI canvas pixels. A finger landing within a quarter past its edge takes it; between two, the nearer."),
            k("[touch.<name>] label", "string", "\"\"", "A button's words, drawn on it and shown by button prompts; empty: the name of the action bound to it (`jump` is Jump)."),
            k("[touch.<name>] floating", "boolean", "false", "A stick only: a finger landing anywhere in its half of the screen (its anchor's side, or all of it for a middle anchor) takes it, centred where it landed. A button's own place comes first."),
            k("[touch.<name>] draw", "boolean", "true", "Drawn by the engine; false when the game draws its own (it reads the actions as ever)."),
            k("[slots.<n>] devices", "array of strings", "a single slot 1 with [\"keyboard\"]", "Player slot n (1 to 8) and the devices that feed it: `keyboard`, `mouse`, `gamepad` (every pad), `gamepad:0` (the first pad connected, and so on), `touch`. Touch goes with the mouse: a slot that lists `mouse` takes the fingers too. Without any `[slots]` there is one slot, 1, fed by the keyboard alone: list `mouse` and a gamepad to have them. A button is held when any binding holds it; an axis takes the binding with the largest value, an axis2 the longest."),
        ],
        example_from: "examples/hollow/input.toml",
        example: "[actions.move]\ntype = \"axis2\"\ndoc = \"Run left and right; down with jump drops through a plank\"\n[actions.move.bindings]\nkeyboard = [\"wasd\", \"arrows\"]\ngamepad = [\"left_stick\", \"dpad\"]\n\n[actions.pause]\ntype = \"button\"\n[actions.pause.bindings]\nkeyboard = [\"escape\"]\ngamepad = [\"start\"]\nplatform = [\"overlay\"]\n\n[actions.pointer]\ntype = \"pointer\"\n[actions.pointer.bindings]\nmouse = [\"position\"]\n\n[slots.1]\ndevices = [\"keyboard\", \"mouse\", \"gamepad:0\"]",
        errors: "When the project opens, 4905 at the action's line: a name that cannot be a field, a type that is not one of the four, `toggle` on an axis, a bindings device that is not keyboard, mouse, gamepad, platform, or touch, a mouse, gamepad, or platform name not in the lists above (the message lists the known ones), a touch binding that names no control of the right kind (the message lists the controls `[touch]` declares), a `[touch.<name>]` with a key not above, a `kind` not button or stick, an anchor not in the list, a `size` not above 0, or `floating` on a button, a slot number outside 1 to 8, or an action named `device`, `typed`, or `typing` (the engine's: `Actions.typed` is what the slot typed this tick, and while `Actions.typing` is true its keyboard types instead of moving actions, as a text box sets it). A table or list of the wrong shape: 4605. A button `fire` beside an action named `fire_pressed`: 4005, the field declared twice.",
    },
    FileDoc {
        title: "defs/<table>.toml: a data table's rows",
        intro: "A def table is declared in Luau (`def \"items\" { key = \"id\", prefix = \"item\", name = field.str {}, ... }` in `defs/`, `components/`, or `systems/`: the scripting reference) and its rows are in `defs/<table>.toml`, the file named after the table (not its prefix). Each top-level table is one row: its header is the row's key (quote it when it has dots: `[\"key.brass\"]`), its keys the row's fields, written as component fields are (a def row cannot hold an entity). A field a row leaves out has its declared default. Another def row is referred to as `<prefix>:<key>` in a `ref` field (`output = \"item:meal\"`), and such a ref must name a row. Scripts read rows with `q:def(\"item:meal\")` and `q:defs(\"items\")`; the protocol with `world.defs`; `edit` def and def_row ops change them in place. The engine declares two tables of its own, `dialogue` and `timeline` (below); a game that declares a table of the same name has its own instead.",
        keys: &[
            k("[\"<key>\"]", "table", "", "A row, named by its key (the table's `key` field, `id` by default, takes the header's value; an explicit different value is an error)."),
            k("<field>", "the field's type", "the field's default", "A field the table declares, written as its type wants (see how values are written, above)."),
        ],
        example_from: "examples/outpost/defs/recipes.toml",
        example: "[\"meal\"]\noutput = \"item:meal\"\ncount = 1\ninput = \"item:crop\"\ninput_count = 2\nwork = 3.0",
        errors: "Each is a project issue at its file and line, the bad row is left out, and no scene spawns until it is fixed: a file whose table no script declares, \"has no schema\" (4607); a field the table does not declare, \"has no field\" (4002, listing its fields); a value of the wrong type (4003), out of range (4102), or that is an entity or a table (4607); `[[row]]` arrays or top-level values, \"must be a row table\" (4607); a key field that disagrees with the header (4607); a ref to a row that does not exist, \"missing reference: def ... does not exist\" (4603). An unquoted dotted header (`[hermit.start]`) is the row `hermit` with a table `start` in it: 4002 or 4607.",
    },
    FileDoc {
        title: "defs/dialogue.toml: conversations",
        intro: "The engine's `dialogue` table (prefix `dialogue`, key `id`): each row is a node of the game's conversations. A Talk component (spawned by a script, a timeline's line cue, or the protocol, with `start` a node) plays them in a box of UI nodes. A row without `from` is a node: it shows its `line` said by its `speaker` (both string table keys), offering the choices from it, then goes to `next`. A row with `from` and a `line` is a choice of that node (its line is what the player picks); a row with `from` and no `line` is a branch: when a node has no line and no choice to offer, it goes to its first branch whose `when` holds (or to its own `next`). A node whose `when` fails is passed over to its `next`. `effects` are made when a node is shown, a choice is taken, or a branch followed; an empty `next` ends the conversation. A condition (`when`) is tests joined by `and` and `or` (and binds tighter): `[not] [who:]Component.field [op value]`, op one of `==` (or `=`), `!=`, `<`, `<=`, `>`, `>=`, a value a number, true, false, or a string (quoted or one word), who `listener` (the default), `speaker`, or `@id`; a bare path is true when it is a true bool, a non-empty string, an entity, or a non-zero number. Effects are separated by `;`, each `[who:]Component.field = value`, `+= n`, `-= n`, or `emit <event>`. Refs in `from` and `next` are `dialogue:<key>` or the bare key. The engine's box uses the string keys `dialogue.continue` and `dialogue.end` (English by default) and the theme's `panel`, `button`, and `accent` colours.",
        keys: &[],
        example_from: "examples/hollow/defs/dialogue.toml",
        example: "[\"hermit.start\"]\nnext = \"dialogue:hermit.hello\"\n\n[\"hermit.start.again\"]\nfrom = \"dialogue:hermit.start\"\nwhen = \"speaker:Hermit.met\"\nnext = \"dialogue:hermit.again\"\n\n[\"hermit.hello\"]\nspeaker = \"hermit.name\"\nline = \"hermit.hello\"\neffects = \"speaker:Hermit.met = true\"\nsound = \"sfx/voice\"\nnext = \"dialogue:hermit.ask\"\n\n[\"hermit.ask.give\"]\nfrom = \"dialogue:hermit.ask\"\nline = \"hermit.give\"\nwhen = \"@hud:Run.gems >= 1 and not speaker:Hermit.has_gem\"\neffects = \"speaker:Hermit.has_gem = true; emit hermit.given\"\nnext = \"dialogue:hermit.thanks\"",
        errors: "Row errors as for any def table. A `when` or `effects` that does not parse, or a bare `from` or `next` that names no row, is a `script.errors` row (phase dialogue) with the file and line; the scene still runs, and a `when` that did not parse never stops its node. At run time a problem is a `dialogue.problem` event with `why` (no such node, no choice n, 64 nodes in a row without a line).",
    },
    FileDoc {
        title: "defs/timeline.toml: timelines",
        intro: "The engine's `timeline` table (prefix `timeline`, key `id`): a row without `timeline` is a timeline, and a row with `timeline = \"timeline:<name>\"` is a cue on it, fired `at` seconds in (cues sort by `at`, file order breaking ties). A Timeline component plays one: a script spawns it (`q:spawn({\"Timeline\"}):get(\"Timeline\").name = \"outro\"`, with its `actor` and `other`), as the `run` tool's `timeline` and a scene's `[rooms] transition` (its actor the followed entity) do. Kinds of cue: `tween` drives `who`'s `path` (`Camera.height`, `Transform.pos.x`) from `from` (or its value now) to `to` over `duration` with `easing`; `anim` sets `who`'s Animator to `state`; `camera` makes `who`'s Camera draw for `duration` seconds (0: to the end); `line` starts a conversation at the dialogue node `line`, `who` speaking to `other` (or to the actor); `event` makes its `effects` (the dialogue's language, who being `actor`, `other`, or `camera`). Any cue may play a `sound` from `who`, be passed over when its `when` fails, and `wait`: the timeline holds until the cue is done (an event cue that waits holds until its `when` holds; a camera cue cannot wait). `who` is `actor` (the default), `other`, `camera` (the one drawing), or `@id`. Easings: linear, step, and in_, out_, or in_out_ with quad, cubic, quart, sine, expo, back, elastic, or bounce (`out_back`). A timeline's `length` of 0 is its last cue's end.",
        keys: &[],
        example_from: "examples/hollow/defs/timeline.toml",
        example: "[\"outro\"]\nlength = 5.0\n\n[\"outro.wide\"]\ntimeline = \"timeline:outro\"\nkind = \"camera\"\nwho = \"@wide\"\n\n[\"outro.zoom\"]\ntimeline = \"timeline:outro\"\ntrack = \"wide\"\nkind = \"tween\"\nwho = \"@wide\"\npath = \"Camera.height\"\nfrom = [12.0, 0.0]\nto = [14.0, 0.0]\nduration = 2.5\neasing = \"in_out_sine\"\n\n[\"outro.call\"]\ntimeline = \"timeline:outro\"\nat = 3.0\nkind = \"line\"\nwho = \"@hermit\"\nline = \"dialogue:outro.start\"\nwait = true",
        errors: "Row errors as for any def table (`at = 5000` is out of range, 4102). A cue that cannot work (a kind that is not one of the five, a tween without a path, an anim without a state, a line without a line, an event that does nothing, a `who` that is no one, a path or a `when` naming a component or field there is not) is a `script.errors` row (phase timeline) and the cue is left out; a cue past the timeline's length is reported and kept. An easing that is not one of the names is reported when the tween runs. At run time a problem is a `timeline.problem` event.",
    },
    FileDoc {
        title: "strings/<language>.toml: player-facing text",
        intro: "One file per language under `strings/`, named by its code (`en.toml`, `fr.toml`); the base language (argus.toml `[strings] base`, `en` by default) is the one every other falls back to, key by key. Keys are any TOML keys, tables and dotted keys flattened with dots (`[menu] resume = \"Resume\"` is `menu.resume`), and every value is a string. A UiText's `key` and a script's `q:text(key, args)` read them in the run's language (the first UiSettings' `language`, empty for the base): a string may hold `{Component.field}` (the UiText's own entity's field) and `{@id:Component.field}` (a named entity's) holes, filled each layout, and a script's `{name}` holes are filled from its `args`. A key no table has shows as `[key]`. There are no plural forms: choose a key in the script (`hud.one_fall`, `hud.falls`). By custom each file names its language in `language.name`, which the settings menu shows. The engine's own keys, which a table may override: `dialogue.continue`, `dialogue.end`, `input.press_a_key`. A release build ships each language but the base in a pack of its own. `edit` text ops change a string in place.",
        keys: &[
            k("<key>", "string", "", "A string, by its dotted key."),
            k("language.name", "string", "", "The language's own name (\"Français\")."),
        ],
        example_from: "examples/hollow/strings/en.toml",
        example: "language.name = \"English\"\n\n[settings]\ntitle = \"Settings\"\ntext = \"Text size: {percent}%\"\n\n[hud]\nno_falls = \"no falls\"\none_fall = \"1 fall\"\nfalls = \"{n} falls\"",
        errors: "A file that does not parse, or a value that is not a string, is a `script.errors` row (phase strings); the rest of the strings load and the game runs. A hole naming nothing shows as `{?Component.field}`.",
    },
    FileDoc {
        title: "ui.toml: the UI's theme",
        intro: "At the project's root, optional (without it, the defaults below). The UI canvas every UiRoot not hung from a world entity is laid out on, the default font and text size, and the colour tokens a UiPanel's or UiText's `color` may name. Read once when the scene assembles; mods cannot change it. Keys it does not know are ignored.",
        keys: &[
            k("width", "number, at least 1", "960", "The UI canvas's width in UI pixels; it is scaled to the window."),
            k("height", "number, at least 1", "540", "The canvas's height."),
            k("font", "string", "\"\" (the built-in pixel font)", "A font asset's name (`\"fonts/LiberationSans-Regular\"`, for `assets/fonts/LiberationSans-Regular.ttf`). Not checked: a name no font has draws in the pixel font."),
            k("text_size", "number above 0", "16", "The text size in UI pixels a UiText without its own `size` has."),
            k("text_scale", "number above 0", "1", "The text size factor when the world has no UiSettings entity (with one, its `text_scale`)."),
            k("high_contrast", "boolean", "false", "Whether high-contrast colours are in effect when the world has no UiSettings entity."),
            k("[colors] <token>", "string", "", "A colour token, `#RGB`, `#RRGGBB`, or `#RRGGBBAA`: overrides one of the engine's (`text` #E8E4DC, `muted` #9AA1A8, `panel` #15181CE6, `border` #3A4048, `focus` #FFD166, `accent` #E8703F, `disabled` #6A7078, `button` #262C34, which the engine's dialogue box's buttons use) or adds one."),
            k("[high_contrast_colors] <token>", "string", "the same token's [colors] value, for a new token", "The token's colour in high contrast."),
        ],
        example_from: "examples/outpost/ui.toml",
        example: "font = \"fonts/LiberationSans-Regular\"\ntext_size = 15\n\n[colors]\nbutton = \"#5E4632\"\nchosen = \"#A27840\"\n\n[high_contrast_colors]\nbutton = \"#000000\"\nchosen = \"#FFFFFF\"",
        errors: "4005 at `ui.toml:1` (not the line), and the scene does not assemble: the file does not parse, a size that is not a positive number, a font that is not a string, `high_contrast` not true or false, or a colour that is not `#RGB`, `#RRGGBB`, or `#RRGGBBAA`.",
    },
    FileDoc {
        title: "audio.toml: buses, sounds, and music",
        intro: "At the project's root, optional. Sounds are named (`sfx/step`, `music/cave.low`) and played by name: a script's `q:play(\"sfx/step\", {entity = e})`, an animator event's `sound`, a dialogue node's or a timeline cue's `sound`; music is a `[music.<name>]` track a Music component's `track` plays, its layers fading in as the component's `intensity` rises. A sound is defined here, or by a sidecar under `assets/` with the same keys (`type = \"sound\"`, below), or by a bare `assets/<name>.wav` or `.ogg` (a sound named by its path, on the sfx bus); one defined here wins. File paths are relative to `assets/`. Without the file, or without `[buses]`, the buses are master, with music, sfx, ui, and ambience under it, all at 0 dB. A mod's own audio.toml adds buses, sounds, music, and ducks in its namespace and may replace the game's sounds and tracks (not `[spatial]`). Keys the file does not know are ignored, and so is a value of the wrong type (it takes the default).",
        keys: &[
            k("[buses] <name>", "table { parent, gain_db }", "", "A mixer bus. A game's `[buses]` replaces the default set (master is added if it is missing)."),
            k("[buses] <name> parent", "string", "\"master\"", "The bus it feeds; master has none."),
            k("[buses] <name> gain_db", "number", "0", "Its gain in decibels."),
            k("[[duck]] when", "string", "required", "A bus: while it plays, `duck` is lowered (the music under dialogue)."),
            k("[[duck]] duck", "string", "required", "The bus lowered."),
            k("[[duck]] db", "number, at most 0", "-6", "How far, in decibels."),
            k("[[duck]] attack_ms, release_ms", "number", "50, 200", "How fast it goes down and comes back."),
            k("[spatial] min_distance", "number", "2", "World units within which a spatial sound plays at full gain (from the AudioListener)."),
            k("[spatial] max_distance", "number", "30", "World units beyond which it is silent."),
            k("[sounds.\"<name>\"] bus", "string", "\"sfx\" (\"music\" for a name under music/)", "The bus it plays on."),
            k("[sounds.\"<name>\"] variations", "array", "[]", "What it plays, one picked at random each time (from the run's seed): each a file path (`\"sfx/step1.wav\"`, WAV or Ogg Vorbis) or a tone table (below)."),
            k("[sounds.\"<name>\"] tone", "table", "none", "One synthesised tone instead (below)."),
            k("[sounds.\"<name>\"] file", "string", "none", "One file instead. A sound needs at least one of variations, tone, and file."),
            k("[sounds.\"<name>\"] gain", "number, at least 0", "1", "Its gain."),
            k("[sounds.\"<name>\"] gain_jitter", "number, 0 to 1", "0", "How much the gain varies from play to play."),
            k("[sounds.\"<name>\"] pitch_jitter", "number, 0 to 0.9", "0", "How much the pitch varies."),
            k("[sounds.\"<name>\"] spatial", "boolean", "true", "Attenuated and panned by its distance from the AudioListener (needs an `entity` to play from)."),
            k("[sounds.\"<name>\"] loop", "boolean", "false", "Loops until stopped."),
            k("[sounds.\"<name>\"] max_voices", "integer, at least 1", "8", "How many may play at once; one more stops the oldest."),
            k("tone wave", "string", "\"sine\"", "`sine`, `square`, `saw`, `triangle`, or `noise`."),
            k("tone freq", "number", "440", "Hertz (not for noise)."),
            k("tone ms", "number", "120", "Length in milliseconds."),
            k("tone attack_ms, release_ms", "number", "3, 40", "Its envelope."),
            k("tone gain", "number, 0 to 1", "0.5", "Its gain."),
            k("tone slide", "number", "0", "Hertz per second the pitch moves."),
            k("tone notes", "array of numbers", "[]", "A sequence of pitches in hertz instead of `freq` and `ms`, each `note_ms` long."),
            k("tone note_ms", "number", "250", "Each note's length."),
            k("tone punch", "number, 0 to 1", "0", "How much louder the body's start (after the attack) is than its end: a hit's snap."),
            k("tone slide_accel", "number", "0", "Hertz per second per second the slide itself changes (a fall that eases off)."),
            k("tone jump, jump_ms", "number, number", "1, 0", "A jump in pitch partway, as a multiple (1.5 is a fifth up; 1 none), `jump_ms` into the note: a coin's second note."),
            k("tone vibrato, vibrato_hz", "number, number", "0, 6", "Vibrato: its depth, a share of the pitch (0.05 is 5 percent), and its rate."),
            k("tone duty, duty_sweep", "number, number", "0.5, 0", "A square wave's share high (less is thinner) and how that moves a second."),
            k("tone repeat_ms", "number", "0", "Start the slide, jump, and vibrato over every this many ms: a power-up's rising steps."),
            k("tone lowpass, lowpass_sweep, resonance", "number, number, 0 to 1", "0, 0, 0", "A low-pass filter: its cutoff in hertz (0 none), how the cutoff moves a second, and its resonance."),
            k("tone highpass, highpass_sweep", "number, number", "0, 0", "A high-pass filter: its cutoff in hertz (0 none) and how it moves a second."),
            k("tone phaser_ms, phaser_sweep", "number, number", "0, 0", "A phaser: a copy this many ms behind taken away (0 none), and how the delay moves a second."),
            k("tone noise_pitch", "boolean", "false", "For noise: a new value every half period of `freq` (an explosion's rumble, pitch and slide apply) instead of every sample."),
            k("[music.<name>] layers = [{ sound, from, to }]", "array of inline tables", "required", "The track's layers, each a `sound` (its first variation, looped); a layer's gain rises from 0 at intensity `from` (0 by default) to 1 at `to` (`from` by default: a step)."),
            k("[music.<name>] bus", "string", "\"music\"", "The bus it plays on."),
            k("[music.<name>] bpm", "number, 20 to 400", "none", "Its tempo, so a change of track waits for a bar or a beat."),
            k("[music.<name>] beats_per_bar", "integer, 1 to 16", "4", "Beats in a bar."),
            k("[music.<name>] change_on", "string", "\"bar\" with a bpm, else \"now\"", "When a change of track starts: `bar`, `beat` (both need a bpm), or `now`."),
            k("[music.<name>] fade_ms", "number", "1000", "How long tracks cross-fade."),
        ],
        example_from: "examples/hollow/audio.toml",
        example: "[buses]\nmaster   = { gain_db = 0 }\nsfx      = { parent = \"master\", gain_db = -4 }\nmusic    = { parent = \"master\", gain_db = -8 }\ndialogue = { parent = \"master\", gain_db = -2 }\n\n[[duck]]\nwhen = \"dialogue\"\nduck = \"music\"\ndb = -6\n\n[sounds.\"music/cave.low\"]\ntone = { wave = \"triangle\", notes = [110, 110, 131, 110, 98, 98, 110, 131], note_ms = 750, attack_ms = 30, release_ms = 200, gain = 0.5 }\n\n[music.cave]\nbpm = 80\nfade_ms = 400\nlayers = [ { sound = \"music/cave.low\" }, { sound = \"music/cave.high\", from = 0.3, to = 0.8 } ]",
        errors: "5101 at the table's line: a bus whose parent is not a bus, or a loop of parents; a duck naming no bus (`[[duck]]` must be an array of tables); a sound with nothing to play, a bad variation or tone, a wave that is not one of the five, or a bus that does not exist; a track without layers, a layer without a sound, a bpm outside 20 to 400, `beats_per_bar` outside 1 to 16, or `change_on` that is not bar, beat, or now (bar and beat need a bpm). The scene still runs without sound: `audio.status` shows the error. A music layer naming no sound, or a file that does not decode, is a warning there.",
    },
    FileDoc {
        title: "platform.toml: achievements and the store",
        intro: "At the project's root, optional: the game's achievements, which `q:achieve(id)` unlocks (and `q:achieve(id, current, max)` shows progress on), and its Steam app. The daemon and a build without Steam run on the null platform (`platform.status` shows what it was asked; its cloud is `.argus/platform/cloud`); a release build of a game whose argus.toml lists `modules = [\"steam\"]` runs on Steam when Steam is there. Rich presence and the overlay are script calls (`q:presence`, `q:overlay`), not settings. Only the game's file is read, never a mod's. Strict: a key it does not know is an error.",
        keys: &[
            k("[steam] app_id", "integer, at least 1", "none", "The Steam app (480 is Spacewar, Valve's test app)."),
            k("[cloud] saves", "boolean", "true", "Whether saves are copied to the platform's cloud (the newer copy wins when one loads)."),
            k("[achievements.<id>]", "table", "", "An achievement; the id is letters, digits, and `_`."),
            k("[achievements.<id>] name", "string", "the id", "What a player sees."),
            k("[achievements.<id>] doc", "string", "\"\"", "How it is earned."),
            k("[achievements.<id>] steam", "string, not empty", "the id", "Its Steamworks API name."),
            k("[achievements.<id>] hidden", "boolean", "false", "Kept and shown by `platform.status`; neither platform uses it yet."),
        ],
        example_from: "examples/hollow/platform.toml",
        example: "[steam]\napp_id = 480\n\n[achievements.out_of_the_hollow]\nname = \"Out of the hollow\"\ndoc = \"Reach the way out at the end of the hall\"\nsteam = \"ACH_WIN_ONE_GAME\"",
        errors: "6301 at the section's line, and the scene does not assemble: a section that is not steam, achievements, or cloud; a key a section does not have; an app_id that is not a whole number above 0; an achievement id that is not letters, digits, and `_`; an achievement key of the wrong type or an empty `steam`. `q:achieve` with an id the file does not declare: 6302.",
    },
    FileDoc {
        title: "assets/: art, sound, and their sidecars",
        intro: "Everything under `assets/` (and a mod's `mods/<id>/assets/`) is imported when the project opens and again when it changes (the daemon polls twice a second; `asset.import` now). An asset's name is its path under `assets/` without the extension: `assets/sprites/hero.png` is `sprites/hero`, and scenes name it so (`[Sprite] atlas = \"sprites/hero\"`, `[Animator] machine = \"anim/hero\"`, `[Emitter] effect = \"fx/pop\"`, `[Skeleton] skeleton = \"puppet/puppet\"`, ui.toml's `font = \"fonts/LiberationSans-Regular\"`; a Tilemap's `map` is the exception, a path from the project's root). The source decides the kind: `.png` a sprite (`<name>.normal.png` beside it is its normal map, for lights), `.aseprite` or `.ase` a sprite, `.art.luau` a sprite an art script draws, `.wav` or `.ogg` a sound, `.wgsl` a material's shader, `.ttf` or `.otf` a font, `.json` a Spine 4 skeleton, `.tsx`, `.tsj`, `.tmx`, `.tmj` Tiled files (above); other extensions and names starting with `.` are ignored. A `.toml` of the same name beside the source is its sidecar (an older `.atlas.toml` still is); a `.toml` alone is an asset by itself, its kind its `type` (`sprite`, `sound`, `animator`, `emitter`, `material`, `font`) or, without one, the keys it has (`states` or `initial`: an animator; `rate`, `burst`, or `lifetime`: an emitter; `shader`, `params`, or `blend`: a material; `variations`, `tone`, `file`, or `bus`: a sound; `kind`, `grid`, `frames`, `clips`, `pivot`, or `color`: a sprite). Two sources with one name (a `.png` and a `.wav`) are refused. `asset.list` lists them with their warnings; `asset.get` one as imported; `asset.sidecar` and `edit` asset ops read and change a sidecar. A mod's asset in its namespace is added, and one with a name the game has replaces it.",
        keys: &[],
        example_from: "",
        example: "",
        errors: "5201 a sidecar that does not read (at its line), 5202 a source that does not (an image, a sound, a font), 5203 two sources for one name or a `.toml` that describes nothing, 6002 a mod's asset outside its namespace. Each is in `asset.list`, the asset left out; warnings (an unknown key, a frame outside the sheet) are there too.",
    },
    FileDoc {
        title: "assets/<name>.toml beside a .png: a sprite",
        intro: "How a sprite's image is cut into frames and clips, and its tiles' properties when it is a tileset. Without any sidecar the whole image is one frame with its pivot at the centre. Alone (no image), `kind`, `size`, and `color` make a placeholder shape, so a game runs before its art exists and the PNG replaces it with nothing else changed. The image's pixels per frame and the frame's place decide what is drawn; its size in the world is the Sprite's `size`. A tileset is a sprite a tilemap layer names: tile n is frame n - 1, and the engine reads three frame properties, `solid` (true: solid from every side), `one_way` (true: solid from above only, a plank to jump up through), and `slope` (`\"[left, right]\"`, heights from 0 to 1 at each edge); a property is true when it is true, 1, or yes. A top-level key it does not know is a warning; inside `[[frames]]`, `[clips]`, and `[autotile]` unknown keys are ignored.",
        keys: &[
            k("type", "string", "", "\"sprite\" (only needed when the keys do not say it)."),
            k("grid", "array [columns, rows]", "[1, 1]", "Cut the image into equal frames, row by row from the top-left."),
            k("pivot", "array [x, y]", "[0.5, 0.5]", "The point of a frame the entity's position is, as fractions of the frame, y up from the bottom."),
            k("duration_ms", "number, at least 1", "100", "Each frame's time in a clip without its own `fps`."),
            k("pixels_per_unit", "number", "16", "Kept and reported by `asset.get`; the drawn size is the Sprite's `size`."),
            k("[[frames]] rect", "array [x, y, w, h]", "", "A frame by its pixels; when any entry has a rect, the rects are the frames (and `grid` is not used)."),
            k("[[frames]] index", "integer", "", "The frame (from 0) this entry changes."),
            k("[[frames]] pivot, duration_ms", "as above", "", "This frame's own."),
            k("[[frames]] properties", "table", "{}", "Free key = value text the game reads (`asset.get`); `solid`, `one_way`, and `slope` collide in a tilemap: `properties = { solid = true }`."),
            k("[clips.<name>] frames", "array of integers", "", "The clip's frames in order (or `from` and `to`)."),
            k("[clips.<name>] from, to", "integer", "", "A run of frames, both included (from above to plays backwards)."),
            k("[clips.<name>] fps", "number", "each frame's duration_ms", "Frames a second."),
            k("[clips.<name>] loop", "boolean", "true", "Whether it repeats."),
            k("[clips.<name>] direction", "string", "\"forward\"", "`forward`, `reverse`, or `pingpong` (also written `ping-pong`)."),
            k("[autotile.<terrain>] mode", "string", "\"4bit\"", "How `edit` tiles ops with `terrain` pick tiles by their neighbours: `single` (1 tile), `4bit` (16 tiles, by the neighbours up 1, right 2, down 4, left 8), or `blob47` (also written `blob`: 47 tiles, the corners too)."),
            k("[autotile.<terrain>] tiles", "array of integers", "required", "The tiles (from 1: tile n is frame n - 1), as many as the mode needs."),
            k("[autotile.<terrain>] edges", "string", "\"connect\"", "Whether the map's edge counts as the terrain (`connect`) or not (`open`)."),
            k("kind", "string", "\"rect\"", "A placeholder's shape: `rect` or `circle` (no image only; beside a PNG it is ignored with a warning)."),
            k("size", "array [w, h]", "[16, 16]", "A placeholder's size in pixels."),
            k("color", "string", "\"#FFFFFF\"", "A placeholder's colour, `#RRGGBB` or `#RRGGBBAA`."),
            k("layers", "array of strings", "every visible layer", "Beside an Aseprite file only (below)."),
        ],
        example_from: "`examples/hollow/assets/hero.toml` and `examples/hollow/assets/tiles/rock.toml`",
        example: "# hero.toml: eight frames in a row\ngrid = [8, 1]\n\n[clips.idle]\nframes = [0]\n\n[clips.run]\nfrom = 1\nto = 4\nfps = 12\n\n[clips.land]\nframes = [7, 0]\nfps = 12\nloop = false\n\n# tiles/rock.toml: one tile, solid\ngrid = [1, 1]\n\n[[frames]]\nindex = 0\nproperties = { solid = true }",
        errors: "5201 at the line: a grid, pivot, rect, clip, or autotile of the wrong shape, a clip with no frames, a direction, mode, or edges value that is not one of the listed, an autotile with the wrong number of tiles or a tile 0, a shape that is not rect or circle, a colour that is not `#RRGGBB` or `#RRGGBBAA`. A grid that does not fit the image, a rect outside it, or an autotile tile past the last frame: 5201 when it imports. A sidecar alone without kind, size, or color: 5203.",
    },
    FileDoc {
        title: "assets/<name>.aseprite: Aseprite files",
        intro: "An `.aseprite` or `.ase` file is a sprite: its frames (with their durations), its tags as clips (repeat 0 loops, 1 plays once; forward, reverse, and ping-pong), its slices as pivots and properties, laid out on a sheet when it imports. A sidecar `<name>.toml` beside it may pick layers and change what the file says, without opening Aseprite (`asset.get` lists the file's layers, tags, and slices): `layers` (names or group paths, `\"fx/sparks\"`; a picked layer is drawn even when hidden), `pivot` and `duration_ms` for every frame, `[[frames]]` by `index` (pivot, duration_ms, properties), and `[clips.<tag>]` changing only what it writes of that tag's clip (frames, fps, loop, direction); a clip of another name is a new one. `grid`, frame rects, and shape keys are ignored with a warning: the frames are the file's.",
        keys: &[],
        example_from: "",
        example: "",
        errors: "5201: `layers` that is not a list of names, or names none of which the file has (some missing: a warning); a clip naming no tag and giving no frames. 5203: a normal map beside an Aseprite file. A file that does not read: 5202.",
    },
    FileDoc {
        title: "assets/<name>.art.luau: an art script",
        intro: "A Luau file the importer runs to draw a sprite, in place of a PNG and its sidecar: `assets/sprites/slime.art.luau` is the sprite `sprites/slime`. It draws on canvases with the kit `art` and ends with `return art.sprite{ ... }`, the sprite's frames and clips; the importer packs the frames into a sheet and caches it as it does a PNG, so scenes, animators, and a release use it as any sprite (a release carries the sheet, never the script). It is imported again when it changes, while the game runs. Coordinates are pixels, x right and y down from the top left, as in a PNG; a colour is `\"#RRGGBB\"`, `\"#RRGGBBAA\"`, `\"#RGB\"`, or `\"none\"`; drawing writes the colour as given (only `draw` blends). Sandboxed and the same on every machine: no `os`, files, clock, or network; `math` is libm's (pure Rust, the same everywhere; the `^` operator is the platform's `pow`, so use `math.pow` where it matters); `math.random` is refused (use `art.rng(seed)`); `print` comes back as a warning. Scripts share code and colours through modules: `local pal = require(\"colours\")` runs `assets/colours.lib.luau` (Luau that returns a value, such as a palette or a helper function) once, and a change to a module imports again every script that requires it; a module is not an asset, and a release leaves it out. A sidecar beside it is not read. Ten seconds and 512 MB at most; a canvas is 1 to 2048 pixels a side.",
        keys: &[
            k("art.sprite{ frames }", "list", "[]", "Frames in order, numbered from 0 as `Sprite.frame` counts: each a canvas, or `{ canvas, pivot = {x, y}, ms = n, properties = { slope = {0, 1} } }`. The same canvas twice is one frame."),
            k("art.sprite{ clips }", "table", "{}", "Clips by name: `walk = { a, b, c, fps = 8 }` (canvases, or frame numbers from 0) or `walk = { from = 1, to = 4, fps = 8 }`, with `loop` (default true) and `direction` (forward, reverse, pingpong). A canvas only a clip names becomes a frame after `frames`' (clips taken by name)."),
            k("art.sprite{ pixels_per_unit }", "number", "16", "Kept in the sidecar the script writes and reported by `asset.get`, as a sprite sidecar's is; the drawn size is the Sprite's `size`."),
            k("art.sprite{ pivot }", "{x, y}", "{0.5, 0.5}", "Every frame's pivot, a fraction of the frame, y up from the bottom (`{0.5, 0}` stands it on its feet)."),
            k("art.sprite{ ms }", "integer", "100", "How long a frame shows when its clip gives no fps."),
            k("art.canvas(w, h, fill)", "function", "clear", "A new canvas, filled with `fill`. Its size is `c.w`, `c.h`."),
            k("c:pixel(x, y, color), c:get(x, y), c:alpha(x, y)", "methods", "", "One pixel. `get` answers its colour, `\"#RRGGBB\"` when it is opaque and `\"#RRGGBBAA\"` when it is not (`\"#00000000\"` clear), or nil outside; `alpha` answers how drawn it is, 0 (clear) to 255."),
            k("c:rect(x, y, w, h, color), c:box(x, y, w, h, color)", "methods", "", "A filled rectangle; its outline, one pixel wide."),
            k("c:line(x0, y0, x1, y1, color)", "method", "", "A line, pixel by pixel."),
            k("c:rows(x, y, rows, key)", "method", "", "Pixel art as text: `rows` a list of strings, each character a colour from `key` (`{ [\"#\"] = \"#1E3A20\", o = \"#4CAF50\" }`), drawn from (x, y); a character the key does not have (`.`) leaves its pixel as it is."),
            k("c:circle(cx, cy, r, color), c:ring(cx, cy, r, color), c:ellipse(cx, cy, rx, ry, color)", "methods", "", "A filled circle, its one-pixel ring, a filled ellipse. A centre of 7.5 sits between pixels, for an even width."),
            k("c:poly({x1, y1, x2, y2, ...}, color)", "method", "", "A filled polygon, three points or more."),
            k("c:fill(x, y, color)", "method", "", "Flood fill: the pixels of (x, y)'s colour joined to it."),
            k("c:outline(color, corners)", "method", "", "A line round what is drawn: every clear pixel beside a drawn one (diagonals too with corners = true)."),
            k("c:shade{ light, dark, from = {dx, dy} }", "method", "from = {-1, -1}", "Light on the drawn pixels facing the light (`from`, a direction), dark on those facing away."),
            k("c:dither(x, y, w, h, color, amount)", "method", "", "An ordered dither of `color` over the rectangle, `amount` from 0 to 1."),
            k("c:noise(x, y, w, h, colors, scale, seed, only_drawn)", "method", "scale 4, seed 1", "The rectangle in `colors` (a list) picked by smooth noise; only the drawn pixels with only_drawn."),
            k("c:speckle(colors, amount, seed)", "method", "seed 1", "A share `amount` of the drawn pixels in random `colors`."),
            k("c:mirror(axis), c:flip(axis), c:rotate(turns)", "methods", "\"x\", 1", "Mirror copies the left (or top) half onto the other; flip and rotate (quarter turns clockwise) answer a new canvas."),
            k("c:draw(other, x, y), c:recolor({ [from] = to }), c:copy(), c:crop(x, y, w, h), c:scale(k)", "methods", "", "Draw another canvas over this one (blending by alpha); swap colours; a copy; a part; k times bigger."),
            k("art.grid(canvas, w, h)", "function", "", "A sheet cut into frames of w x h, row by row."),
            k("art.rng(seed)", "function", "seed 1", "Seeded numbers, the same every run: `:int(lo, hi)`, `:float(lo, hi)`, `:chance(p)`, `:pick(list)`."),
            k("art.noise(x, y, seed)", "function", "", "Smooth noise from 0 to 1."),
            k("art.mix(a, b, t), art.lighten(c, k), art.darken(c, k), art.alpha(c, a), art.hsv(h, s, v, a), art.rgb(r, g, b, a), art.rgba(c)", "functions", "", "Colours: between two (rounding halves up), toward white or black, with an alpha, from hue (degrees), saturation, and value, from 0 to 255; and `art.rgba` answers a colour's four numbers, 0 to 255, for arithmetic of your own."),
        ],
        example_from: "a slime of two frames, bouncing",
        example: "local pal = { body = \"#4CAF50\", shine = \"#A5D6A7\", eye = \"#1B1B1B\", line = \"#1E3A20\" }\nlocal function slime(squash)\n  local c = art.canvas(12, 12)\n  c:ellipse(5.5, 7 + squash, 5 - squash, 4 - squash, pal.body)\n  c:pixel(4, 6, pal.eye)\n  c:pixel(7, 6, pal.eye)\n  c:shade{ light = pal.shine }\n  c:outline(pal.line)\n  return c\nend\nreturn art.sprite{\n  pixels_per_unit = 12,\n  pivot = {0.5, 0},\n  clips = { bounce = { slime(0), slime(1), fps = 4 } },\n}",
        errors: "5202 at the script's line: a Luau error, a colour that is not one, a canvas past 2048 pixels, a sprite with no frames, a clip naming a frame there is not, a key art.sprite does not take, a script that ran past ten seconds, `math.random` or `os`. 5203: a normal map beside an art script.",
    },
    FileDoc {
        title: "assets/<name>.toml with type = \"sound\": a sound",
        intro: "A sound defined beside its files instead of in audio.toml: the same keys as an audio.toml `[sounds.\"<name>\"]` table (bus, variations, tone, file, gain, gain_jitter, pitch_jitter, spatial, loop, max_voices, and a tone's keys), its variation paths relative to the sidecar's folder first, then to `assets/`. Beside a `.wav` or `.ogg` of its name the sidecar still has to name what it plays. A sound audio.toml defines with the same name wins. Keys it does not know are ignored.",
        keys: &[],
        example_from: "`assets/sfx/step.toml` in the importer's tests",
        example: "type = \"sound\"\nbus = \"sfx\"\ngain = 0.8\npitch_jitter = 0.05\nvariations = [\"step1.wav\", \"step2.wav\", { wave = \"noise\", ms = 40 }]",
        errors: "5101 at line 1 (the messages are audio.toml's); a variation file that is not there: 5202. A bus audio.toml does not have is a warning, and the sound plays on sfx.",
    },
    FileDoc {
        title: "assets/<name>.toml with type = \"animator\": an animator",
        intro: "A state machine an Animator component runs on its entity's sprite (`[Animator] machine = \"anim/hero\"`; its `sprite` field names the sprite, else the entity's Sprite atlas or Skeleton): each state plays a clip of the sprite, fires events at frames, and leaves by the first transition whose condition holds (`[[any]]` transitions are checked first, from every state). A condition is terms joined by `and` and `or` (`and` binds tighter, so `a and b or c` holds when a and b both do, or c does): `[not] operand [op value]`, the operand a field of the entity (`KinematicBody.grounded`, `KinematicBody.velocity.x`), `abs(<field>)`, or `end` (a clip that does not loop finished), `time` (seconds in the state), `frame` (the clip's position), `loops`; op `<`, `<=`, `>`, `>=`, `==`, `!=`; value a number, true, or false; a bare operand is true when it is not zero. Set `Animator.play` to a state's name to enter it from a script. A key it does not know is a warning, but a transition's is an error.",
        keys: &[
            k("type", "string", "", "\"animator\"."),
            k("initial", "string", "the first state", "The state it starts in."),
            k("face", "\"Component.field\"", "none", "A field the sprite faces by: below zero it is mirrored (`Sprite.flip_x`), above zero it is not, at zero it keeps the way it faced. The art is drawn facing right. `face = \"KinematicBody.velocity.x\"` faces the way the entity moves."),
            k("[states.<name>] clip", "string", "required", "A clip of the sprite."),
            k("[states.<name>] loop", "boolean", "the clip's", "Whether the clip repeats in this state."),
            k("[states.<name>] fps", "number or \"Component.field\"", "the clip's", "Frames a second; a field path reads it from the entity (a tune field)."),
            k("[states.<name>] windup", "integer", "0", "Frames at the clip's start that are the wind-up art."),
            k("[states.<name>] anticipation", "number or field path", "none", "How many frames (at the clip's rate) the wind-up lasts."),
            k("[states.<name>] hold", "number or field path", "none", "Extra frames the hold frame is held."),
            k("[states.<name>] hold_frame", "integer", "the last", "Which clip position is held."),
            k("[states.<name>] events = [{ frame, name, sound }]", "array of inline tables", "[]", "Events at clip positions (from 0): an event named `name` emitted with the entity, frame, and state (`q:events(\"footstep\")` sees it), and `sound` played from the entity; a name, a sound, or both."),
            k("[[states.<name>.transitions]] to", "string", "required", "The state it goes to."),
            k("[[states.<name>.transitions]] when", "string", "required", "Its condition."),
            k("[[states.<name>.transitions]] window", "array [first, last]", "any position", "Only while the clip is on these positions."),
            k("[[states.<name>.transitions]] blend", "number, seconds", "0", "Cross-fade into the next state's clip."),
            k("[[any]]", "array of tables", "", "Transitions (to, when, window, blend) from every state."),
        ],
        example_from: "examples/hollow/assets/anim/hero.toml",
        example: "type = \"animator\"\ninitial = \"idle\"\n\n[states.idle]\nclip = \"idle\"\n\n[[states.idle.transitions]]\nto = \"run\"\nwhen = \"KinematicBody.grounded and abs(KinematicBody.velocity.x) > 0.5\"\n\n[states.run]\nclip = \"run\"\nevents = [{ frame = 0, name = \"footstep\", sound = \"sfx/step\" }, { frame = 2, name = \"footstep\", sound = \"sfx/step\" }]\n\n[[any]]\nto = \"jump\"\nwhen = \"not KinematicBody.grounded and KinematicBody.velocity.y > 0\"",
        errors: "5201 at the line: no states, a state without a clip, a transition without `to` or `when` or naming no state, an unknown transition key, a condition that does not parse, a window or blend of the wrong shape, an event without a frame, an `initial` that is not a state. A clip the sprite does not have is reported when an entity runs the machine.",
    },
    FileDoc {
        title: "assets/<name>.toml with type = \"emitter\": a particle effect",
        intro: "A particle effect an Emitter component plays at its entity (`[Emitter] effect = \"fx/pop\"`). Its particles live in a pool, not as entities. A number key is one number; a range is a number or `[min, max]`; a curve is a number or `[[t, value], ...]` over a particle's life (t from 0 to 1, never going back); a colour is `\"#RRGGBBAA\"` or `[[t, \"#RRGGBBAA\"], ...]`. The keys marked tune are the effect's knobs: the tuning panel's sliders, and `tune.fields asset=fx/pop`. A key it does not know is a warning.",
        keys: &[
            k("type", "string", "", "\"emitter\"."),
            k("sprite", "string", "none", "An atlas to draw particles from (else a described `particle`)."),
            k("frame", "integer", "0", "Its frame."),
            k("particle", "string", "\"rect\"", "The described shape without a sprite: `rect` or `circle`."),
            k("material", "string", "none", "A material asset (or a built-in) to draw them with."),
            k("shape", "string", "\"point\"", "Where particles start: `point`, `circle` (within `radius`), or `box` (within `box`)."),
            k("radius", "number", "0.5", "A circle's radius, world units (tune)."),
            k("box", "number or [w, h]", "[1, 1]", "A box's size (tune)."),
            k("rate", "number", "0", "Particles a second while it plays (tune)."),
            k("burst", "integer", "0", "Particles at once when it starts (tune)."),
            k("duration", "number", "0", "Seconds it emits; 0 is as long as it plays (tune)."),
            k("max", "integer, at least 1", "1000", "The pool's size."),
            k("lifetime", "range, above 0", "[1, 1]", "Seconds a particle lives (tune)."),
            k("speed", "range", "[1, 1]", "Starting speed, world units a second (tune)."),
            k("direction", "number", "90", "Degrees: 0 is right, 90 up (tune)."),
            k("spread", "number, 0 to 360", "360", "Degrees across the cone (tune)."),
            k("gravity", "[x, y]", "[0, 0]", "Acceleration (tune)."),
            k("drag", "number", "0", "Velocity lost a second (tune)."),
            k("space", "string", "\"world\"", "`world`: particles stay where they were born; `local`: they move with the emitter."),
            k("z", "number", "0", "Draw order."),
            k("velocity", "curve", "1", "Speed multiplier over life (tune)."),
            k("size", "curve", "0.2", "Size over life, world units (tune)."),
            k("rotation", "curve", "0", "Rotation over life, degrees (tune)."),
            k("color", "colour or curve", "\"#FFFFFFFF\"", "Colour over life."),
        ],
        example_from: "examples/swarm/assets/fx/pop.toml",
        example: "type = \"emitter\"\nparticle = \"circle\"\nmax = 20000\nlifetime = [0.2, 0.45]\nspeed = [3, 7]\nspread = 360\ndrag = 3\nsize = [[0, 0.35], [1, 0]]\ncolor = [[0, \"#FFF2A0FF\"], [0.5, \"#FF9040C0\"], [1, \"#FF402000\"]]\nz = 4\nmaterial = \"fx/glow\"",
        errors: "5201 at the line: a number outside its range (the message gives it), a range, size, or vector of the wrong shape, a curve whose t goes back, a colour that is not `#RRGGBB` or `#RRGGBBAA`, a shape, particle, or space that is not one of the listed.",
    },
    FileDoc {
        title: "assets/<name>.wgsl and its sidecar: a material",
        intro: "A material draws a sprite (`[Sprite] material = \"fx/glow\"`), particles, or a skeleton with a shader: a `.wgsl` whose fragment half defines `fn material(in: MaterialInput, p: Params) -> vec4<f32>`, with a sidecar `<name>.toml` declaring its parameters; or a sidecar alone over a built-in shader (`unlit`, `lit`, `palette`, `dissolve`, `outline`, `sdf_text`, and `sdf_text_bold`, which rich text's bold draws with), changing its blend or its parameters' defaults. A parameter may be fed per sprite from a component field (`from`), so a tween or a script animates it. The shader is checked when it imports; one that does not compile is a `script.errors` row (phase material) at its line, and the last good version keeps drawing. A key it does not know is a warning.",
        keys: &[
            k("type", "string", "", "\"material\"."),
            k("shader", "string", "<name>.wgsl beside it", "A built-in's name or a `.wgsl` path under `assets/`."),
            k("blend", "string", "\"alpha\" (or the built-in's)", "`alpha`, `add`, or `multiply`."),
            k("emissive", "boolean", "false (or the built-in's)", "Drawn at its own colour whatever the light."),
            k("doc", "string", "\"\"", "What it is for."),
            k("[params.<name>] type", "string", "required", "`f32` (also `float` or `number`), `vec2`, `vec4`, `color` (or `colour`), or `texture`. The name is a lower-case WGSL name, the shader's `p.<name>`."),
            k("[params.<name>] default", "by type", "", "A number, `[x, y]`, `[x, y, z, w]`, `\"#RRGGBB\"`, or a sprite asset's name."),
            k("[params.<name>] range", "[min, max]", "none", "f32 only: its bounds, for the tuning panel."),
            k("[params.<name>] doc", "string", "\"\"", "What it does."),
            k("[params.<name>] from", "string", "none", "A field that feeds it per sprite: `Component.field`, `Component.vec.x`, or `@id:Component.field` (at most four numbers per sprite in all)."),
        ],
        example_from: "examples/swarm/assets/fx/glow.toml",
        example: "shader = \"unlit\"\nblend = \"add\"",
        errors: "5401 at the line: no shader, a shader that is neither a built-in nor a `.wgsl`, a blend or param type that is not one of the listed, a param name that is not a lower-case WGSL name, a default of the wrong kind or outside its range, a range on anything but f32, a `from` that is not a field path; over a built-in, a param it does not have. 5402: the shader does not compile, at its line.",
    },
    FileDoc {
        title: "assets/<name>.ttf or .otf: a font",
        intro: "A font is baked into a distance-field atlas of the characters text needs (printable ASCII, Latin-1, and the engine's own), so text is sharp at any size; ui.toml's `font` and a UiText's `font` name it. A sidecar `<name>.toml` beside it may set how. A key it does not know is a warning.",
        keys: &[
            k("type", "string", "", "\"font\"."),
            k("size", "number, 8 to 256", "32", "The em in atlas pixels."),
            k("spread", "number, 1 to 32", "4", "The distance field's reach in atlas pixels."),
            k("chars", "string", "\"\"", "More characters to bake (a language's letters)."),
        ],
        example_from: "",
        example: "",
        errors: "5201 at the line: a size, spread, or chars of the wrong kind or outside its range. A file that is not a font this reads (a `.ttc` is not): 5202. Characters the font has no glyph for are a warning.",
    },
    FileDoc {
        title: "assets/<name>.json: a Spine skeleton",
        intro: "Any `.json` under `assets/` is a skeleton exported from Spine 4 as JSON (Spine's own format, not described here), its images unpacked into the export's `images` folder beside it (each an ordinary sprite, `puppet/images/arm`). A Skeleton component poses it (`[Skeleton] skeleton = \"puppet/puppet\"`), each slot drawn by a part entity under it; with an Animator whose machine's clips are the skeleton's animations, states play them (`examples/puppet/assets/puppet/moves.toml`). Bones, slots, region attachments, skins, events, IK, and the bone, slot, event, draw order, and IK timelines are read; transform and path constraints, physics, meshes, and deforms are not (a warning). A `.toml` beside it is not read.",
        keys: &[],
        example_from: "",
        example: "",
        errors: "5201: a JSON that does not read, has no `skeleton` section, or was exported by a Spine before 4. Parts the engine does not apply are warnings in `asset.list`.",
    },
    FileDoc {
        title: "*.tilemap.toml: a tilemap",
        intro: "A tile grid the engine draws, collides with, and navigates: a text index anywhere in the project (`maps/`, or beside a scene's entities, where the scene loader skips it), placed in the world by an entity with `[Tilemap] map = \"maps/floor.tilemap.toml\"` and a Transform, the top-left corner of tile (0, 0) (rows go down). Its cells are in chunk files, `<chunks>/<layer>/<cx>_<cy>.bin` (one per chunk with any tile in it, each chunk x chunk cells of `cell` bytes, little-endian, row by row) listed in `<chunks>/manifest.txt`, whose hash the index carries. Make one with `scene.tiles.create` and change it with `edit` tiles ops (cells, a fill, an auto-tiled terrain, or an erase), never by hand. A tile is a number: 0 is none, n is frame n - 1 of its layer's tileset. A layer with a tileset is drawn at its z; a layer with `collision = true` collides, each tile by its tileset's frame properties (`solid`, `one_way`, `slope`), and a collision layer with no tileset is solid wherever it has a tile and is never drawn: walls a script fills. A NavGrid with `tilemap = \"@floor\"` takes its blocked cells from the collision. A runtime map (`runtime = true`) has no chunks: its cells start empty each run and a script writes them (`q:tiles(map)`), and they are part of the run (its digest, keyframes, and saves). Unknown keys are ignored, and so is a value of the wrong type (an integer key given a float takes its default).",
        keys: &[
            k("kind", "string", "required", "Must be \"tilemap\"."),
            k("width, height", "integer, at least 1", "required", "Size in tiles."),
            k("tile_size", "number", "1", "World units per tile. Must be above 0 (it is not checked)."),
            k("chunk", "integer, at least 1", "64", "Tiles per side of a chunk file."),
            k("cell", "integer: 1, 2, or 4", "2", "Bytes per tile: the largest tile id is 255, 65535, or more."),
            k("chunks", "string", "required unless runtime", "The chunk folder beside the index (`level.chunks`): one name, no slashes."),
            k("hash", "string", "required unless runtime", "`fnv1a64:<16 hex digits>` of `manifest.txt`, checked when the map opens; `edit` keeps it current."),
            k("runtime", "boolean", "false", "Cells the run makes, not files: no chunks or hash, and only a runtime map's cells may be written by a script."),
            k("[[layers]] name", "string", "required", "The layer's name (a plain folder name, unique)."),
            k("[[layers]] tileset", "string", "none", "A sprite asset whose frames are the tiles (`\"tiles/rock\"`, for `assets/tiles/rock.png` and its sidecar). None: not drawn, and solid where it has tiles if it collides."),
            k("[[layers]] z", "number", "0", "Draw order, higher in front."),
            k("[[layers]] collision", "boolean", "false", "Whether its tiles collide (and block navigation)."),
            k("[[layers]] parallax", "[x, y], or one number", "1", "How fast it crosses the screen as the camera moves, against the world: below 1 slower (a far background), 0 not at all, above 1 faster. Drawing only: its tiles collide where they are."),
            k("parallax_origin", "[x, y]", "[0, 0]", "Where the camera's centre is when the parallax layers are drawn where their tiles are: world units from the map's top-left corner, y up."),
        ],
        example_from: "examples/delve/maps/floor.tilemap.toml",
        example: "kind = \"tilemap\"\nruntime = true\nwidth = 40\nheight = 20\ntile_size = 1.0\nchunk = 20\ncell = 1\n\n[[layers]]\nname = \"floor\"\ntileset = \"tiles/dungeon\"\nz = 0.0\n\n[[layers]]\nname = \"walls\"\nz = 0.0\ncollision = true",
        errors: "5301 at the index: not \"tilemap\", no `[[layers]]` (an inline `layers = [...]` array is not read), a layer without a name, a size or chunk under 1, a cell size that is not 1, 2, or 4, or a chunks or layer name that is not a plain folder name. A manifest whose hash differs from the index's, or a chunk that is not the size or hash its manifest line says: 5302 (\"The chunks changed outside the tool\"). `[Tilemap] map` naming a file that does not open makes the map draw and collide with nothing, and its error is in `scene.tiles.info`.",
    },
    FileDoc {
        title: "Tiled maps and tilesets: .tmx, .tmj, .tsx, .tsj",
        intro: "Maps and tilesets made in Tiled are read as they are, never written. A tileset (`assets/tiles/cave.tsx` or `.tsj`, its image a PNG under `assets/`) is a sprite asset named by its path (`tiles/cave`), its tiles its frames; a tile's custom properties are frame properties (`solid`, `one_way`, `slope` collide) and a tile's animation becomes a clip. A map under `assets/` is a `tilemap` asset (`asset.get` shows its layers and objects), and `[Tilemap] map = \"assets/maps/level1.tmx\"` places it like a `*.tilemap.toml`: orthogonal, square tiles only, finite or infinite, CSV or base64 data (none, zlib, or gzip; not zstd). Each tile layer is an engine layer (one per tileset it uses, named `<layer> (<tileset>)`), its custom properties `z` (default: the layer's index) and `collision` (true, 1, or yes); the map's `tile_size` property sets world units per tile (1 by default); a tileset's `pixels_per_unit` property its sprite scale (default: its tile width). Group properties carry down to their layers; a hidden layer is not drawn but still collides. Object layers are data for the game (`asset.get`), not entities. An embedded tileset is a sprite named `<map path>/<tileset name>` (`maps/level1.tmx/terrain`). A `.toml` sidecar beside a tileset may add frame properties, clips, `pixels_per_unit`, and `[autotile]` rules (Tiled's wang sets are not read); beside a map it is not read. A layer's parallax factors (a group's multiplied into its layers) and the map's parallax origin are carried. Flips, image layers, opacity, tint, and tile collision shapes are dropped with a warning.",
        keys: &[],
        example_from: "",
        example: "",
        errors: "A file that is not a Tiled map or tileset, a map that is not orthogonal or whose tiles are not square, zstd data, a layer whose tile count does not fit its size, or a tileset that is an image collection: 5202 at the asset (5301 when a Tilemap opens it). The rest are warnings in `asset.list`.",
    },
    FileDoc {
        title: "*.navgrid.toml: a baked navigation grid",
        intro: "What `nav.bake` writes (`scenes/<scene>/<grid>.navgrid.toml` by default) and a NavGrid's `baked` field reads instead of deriving the grid from a tilemap: an index beside a `.navgrid.bin` of one byte per cell, the step cost (0 blocked), rows from the bottom. The scene loader skips it. Made by the tool, not by hand.",
        keys: &[
            k("width, height", "integer", "required", "Cells."),
            k("cell", "number", "required", "World units per cell."),
            k("origin", "array of 2 numbers", "required", "The grid's lower-left corner in the world."),
            k("diagonal", "boolean", "false", "Whether paths may step diagonally (the NavGrid's own `diagonal` wins)."),
            k("hash", "string", "none", "16 hex digits of the grid; when present it must match the chunk."),
            k("open, blocked, chunk, from", "", "", "Written by `nav.bake` for a reader; not read."),
        ],
        example_from: "",
        example: "",
        errors: "5502 naming the file and what is wrong (a missing key, a chunk of the wrong size, a hash that differs), with the hint to bake it again or clear `NavGrid.baked`.",
    },
    FileDoc {
        title: "*.map.toml: bulk data",
        intro: "The first form of bulk data: a grid of numbers kept as a text index beside a binary chunk, in a scene or room directory, so nobody reads a 40,000-line array. `scene.tiles.get` reads a rect of one; nothing in the engine draws or collides with it. Tilemaps (`*.tilemap.toml`, below) are the form the engine draws, collides with, and edits; use them for tiles.",
        keys: &[
            k("kind", "string", "required", "What the data is: `tiles`, `props`, `nav`, or the game's own word."),
            k("width, height", "integer", "required", "Columns and rows."),
            k("layers", "array of strings", "[\"default\"]", "Layer names, in the chunk's order."),
            k("cell", "integer: 1, 2, or 4", "required", "Bytes per cell."),
            k("chunk", "string", "required", "The chunk's file name, beside the index: layers in order, each width x height cells, row by row, little-endian."),
            k("hash", "string", "required", "`fnv1a64:<16 hex digits>` of the chunk, checked when it opens."),
            k("tileset", "string", "none", "A tileset path reference."),
        ],
        example_from: "the engine's own description",
        example: "kind    = \"tiles\"\nwidth   = 64\nheight  = 48\nlayers  = [\"ground\", \"props\"]\ncell    = 2\nchunk   = \"tiles.map.bin\"\nhash    = \"fnv1a64:9f0c...\"",
        errors: "A missing key or a cell size that is not 1, 2, or 4: 4605. A chunk that cannot be read: 4602. A chunk whose size the index does not imply, or whose hash differs: 4608, saying which.",
    },
    FileDoc {
        title: "mods/<id>/mod.toml: a mod",
        intro: "A mod is a directory shaped like a small project, `mods/<id>/` in the game's project (a player's go in their save folder's `mods/`, or are `.pack` files `argus build --mod <id>` makes), with a `mod.toml`. Its `components/`, `systems/`, `defs/`, `prefabs/`, `scenes/`, `assets/`, `strings/`, and `audio.toml` are laid over the game's. Everything a mod adds is named in its namespace, its id and a separator: `lanterns.lamp`, `lanterns_Vigil`, `lanterns/kindle`, `lanterns-vigil` (components, systems, def tables, def rows, prefabs, scenes, rooms, entities, assets, sounds, music, string keys). Its `lib/` holds modules of its own: the mod's files look there first, then in the game's `lib/`, and the game's files only in the game's, so a mod adds modules but cannot replace one. A name the game (or a mod before it) already has is changed instead: a def row field by field, a prefab, an entity file (same room and id), an asset, a sound, a track, or a string replaced whole; components, systems, and def tables cannot be redeclared. A mod adds entities to the game's rooms (`rooms/a/lanterns.lantern.toml`) and rooms of its own (with a `room.toml` whose `neighbors` join the graph), but cannot change a scene's `scene.toml` or a room's `room.toml`, or the game's argus.toml, input.toml, ui.toml, platform.toml, or plugins. The daemon loads only the mods a session names (`session.open` and `scene.load` take `mods`, in load order); `mods.list` shows them, what each added, and what each changed. A mod with any problem in its files is left out whole, with every reason, and so is a mod that requires it. Keys mod.toml does not know are ignored.",
        keys: &[
            k("id", "string", "required", "A lowercase letter, then lowercase letters, digits, or `_`, at most 40 characters; the same as the directory's name."),
            k("name", "string", "the id", "What a player sees."),
            k("version", "string", "\"0\"", "Its version, recorded with every run made with it."),
            k("game", "string", "none", "The game it is for (argus.toml's `name`): another game's mod is left out."),
            k("requires", "array of strings", "[]", "Mods that must load before it."),
            k("doc", "string", "\"\"", "What it does."),
        ],
        example_from: "examples/hollow/mods/brighter/mod.toml",
        example: "id = \"brighter\"\nname = \"Brighter lanterns\"\nversion = \"1.0\"\ngame = \"hollow\"\nrequires = [\"lanterns\"]\ndoc = \"Bigger lanterns, and a keeper proud of them.\"",
        errors: "6001 at `mod.toml:1`: it does not parse, has no id, an id that is not one, an id that is not its directory's name, `requires` that is not a list of ids. 6002: something a mod adds outside its namespace, or a component, system, def table, scene setting, or room it would change. 6003: a mod that needs one not loaded before it, or is for another game. 6004: mods asked for when argus.toml says `[mods] enabled = false`.",
    },
    FileDoc {
        title: "mods/load-order.toml: which mods a player's game loads",
        intro: "In a player's mods folder (a release build's), optional: without it every mod there loads, by id, each after the mods it requires. With it, only the mods it lists load, in its order. The daemon does not read it (a session names its mods). Keys it does not know are ignored.",
        keys: &[k(
            "order",
            "array of strings",
            "required",
            "Mod ids in load order.",
        )],
        example_from: "crates/argus-player/tests/mods.rs",
        example: "order = [\"b\", \"a\"]",
        errors: "6001: it does not parse, or has no `order`. A mod it does not list is left out (6003, \"is not in load-order.toml\"), and so is a listed one that needs a mod listed after it.",
    },
    FileDoc {
        title: "plugins/<name>/: a Rust plugin",
        intro: "A hot path in Rust: a crate in `plugins/<name>/` that argus.toml's `plugins` lists, built with cargo when the project opens and on `reload` (into `.argus/plugins/target`), and loaded in the order argus.toml lists them. Its `Cargo.toml` makes a `cdylib` whose library name is the plugin's name with `-` as `_`, depends on `argus-plugin = { path = \"../../.argus/plugins/sdk\" }` (the SDK the engine writes there before each build), and has an empty `[workspace]` of its own. Its `src/lib.rs` declares its components (field for field as the game declares them) and systems with `argus_plugin::plugin!`; the SDK documents itself (`cargo doc --open --manifest-path .argus/plugins/sdk/Cargo.toml`). The engine reads nothing in Cargo.toml itself: cargo does. `argus build` puts the built library in `plugins/` beside the game's executable (a web build cannot have plugins).",
        keys: &[],
        example_from: "examples/swarm/plugins/swarm-hot/Cargo.toml",
        example: "[package]\nname = \"swarm-hot\"\nversion = \"0.1.0\"\nedition = \"2021\"\npublish = false\n\n[lib]\ncrate-type = [\"cdylib\"]\n\n[dependencies]\nargus-plugin = { path = \"../../.argus/plugins/sdk\" }\n\n[workspace]",
        errors: "A `script.errors` row with the plugin, file, and line: phase compile (6101: no `Cargo.toml`, no cargo, a compiler error, no cdylib built), load (6102: no library, no `argus_plugin_v1` export, another ABI), or install (6103 a component whose layout is not the game's, 6104 a system declared twice or touching a component the game does not declare). The last good build keeps running.",
    },
    FileDoc {
        title: "settings.toml: a player's settings",
        intro: "Not in the project: a release build keeps it in the player's save folder (`%APPDATA%\\<name>` on Windows, `~/Library/Application Support/<name>` on macOS, `$XDG_DATA_HOME/<name>` or `~/.local/share/<name>` elsewhere, `<name>` argus.toml's), the web page in the browser. The game writes it when the player changes a setting (the first UiSettings entity's fields, a rebind, a toggle) and reads it before the first tick; only what differs from the game's own input.toml is written. The daemon never reads it. It never stops the game: a file that does not parse, an unknown key, or a binding that does not apply is passed over.",
        keys: &[
            k("[ui] text_scale", "number", "1", "UiSettings.text_scale (0.5 to 4)."),
            k("[ui] high_contrast", "boolean", "false", "UiSettings.high_contrast."),
            k("[ui] fullscreen", "boolean", "false", "UiSettings.fullscreen."),
            k("[ui] language", "string", "\"\" (the base)", "UiSettings.language: a string table's code."),
            k("[bindings.<action>] <device>", "array of strings", "", "The player's bindings for an action on a device, as input.toml's (an empty list unbinds it)."),
            k("[toggles] <action>", "boolean", "", "A button action set to toggle (or hold)."),
        ],
        example_from: "a file the game wrote",
        example: "[ui]\ntext_scale = 1.25\nhigh_contrast = true\nfullscreen = false\nlanguage = \"\"\n\n[bindings.jump]\nkeyboard = [\"x\"]\n\n[toggles]\ncrouch = true",
        errors: "",
    },
    FileDoc {
        title: "templates/<name>/template.toml: a template",
        intro: "Not part of a game: a template `argus new` copies (from the install's `templates/`, `$ARGUS_TEMPLATES`, or the engine checkout's) is a whole project, every file but this one copied, with argus.toml's `name` replaced by the new game's. `argus new` then opens it and runs its scenarios, which must pass, and writes its ARGUS.md, AGENTS.md, and CLAUDE.md. `argus new` lists templates with what their `about` says; it reads the one line `about = \"...\"` and nothing else.",
        keys: &[k(
            "about",
            "string, on one line",
            "\"\"",
            "What the template is, for the list.",
        )],
        example_from: "templates/blank/template.toml",
        example: "about = \"a player that walks over a floor, and one scenario: the smallest start\"",
        errors: "",
    },
];

/// A def table's schema, as the engine declares it.
type TableSchema = fn() -> argus_scene::DefSchema;

/// Def tables the engine declares, whose rows the reference lists from
/// their schemas.
const ENGINE_TABLES: &[(&str, TableSchema)] = &[
    ("defs/dialogue.toml", argus_dialogue::schema),
    ("defs/timeline.toml", argus_timeline::schema),
];

/// The files' reference as Markdown, headings from `level`: the layout,
/// how values are written, then each file.
pub fn files(level: usize) -> String {
    let h = "#".repeat(level);
    let mut o = String::new();
    let _ = writeln!(o, "{INTRO}\n\n{VALUES}\n");
    for f in FILES {
        let _ = writeln!(o, "{h} {}\n\n{}\n", f.title, f.intro);
        if !f.keys.is_empty() {
            o.push_str("| key | type | default | what it is |\n|---|---|---|---|\n");
            for key in f.keys {
                let _ = writeln!(
                    o,
                    "| `{}` | {} | {} | {} |",
                    key.name,
                    cell(key.ty),
                    cell(key.default),
                    cell(key.doc)
                );
            }
            o.push('\n');
        }
        if let Some((_, schema)) = ENGINE_TABLES.iter().find(|(t, _)| f.title.starts_with(t)) {
            let d = schema();
            let _ = writeln!(
                o,
                "Its rows' fields (from the engine's declaration; key `{}`, prefix `{}`):\n",
                d.key, d.prefix
            );
            fields_table(&mut o, &d.schema.fields);
        }
        if !f.example.is_empty() {
            // A bare path is set as code; a phrase carries its own.
            let from = if f.example_from.contains(' ') {
                f.example_from.to_string()
            } else {
                format!("`{}`", f.example_from)
            };
            let lang = if f.title.contains(".luau") {
                "lua"
            } else {
                "toml"
            };
            let _ = writeln!(o, "From {from}:\n\n```{lang}\n{}\n```\n", f.example);
        }
        if !f.errors.is_empty() {
            let _ = writeln!(o, "When it is wrong: {}\n", f.errors);
        }
    }
    o
}

/// A component's or def table's fields as a Markdown table.
fn fields_table(o: &mut String, fields: &[argus_ecs::FieldSchema]) {
    o.push_str(
        "| field | type | default | range | tune | what it is |\n|---|---|---|---|---|---|\n",
    );
    for f in fields {
        let _ = writeln!(
            o,
            "| `{}` | {} | `{}` | {} | {} | {} |",
            f.name,
            f.ty.name(),
            cell(&toml_value(&f.default, f.ty)),
            f.range
                .map(|(lo, hi)| format!("{lo} to {hi}"))
                .unwrap_or_default(),
            if f.tune { "tune" } else { "" },
            cell(&f.doc)
        );
    }
    o.push('\n');
}

/// A markdown table cell: no pipes or newlines.
fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace(['\n', '\r'], " ")
}

// ----- the engine's components -------------------------------------------

/// The input map the engine's components are read with: one action of
/// each type, so `Actions` shows the fields each type gives.
const ENGINE_INPUT: &str = "[actions.jump]\ntype = \"button\"\ndoc = \"A button action, here named jump\"\n\n[actions.zoom]\ntype = \"axis\"\ndoc = \"An axis action, here named zoom\"\n\n[actions.move]\ntype = \"axis2\"\ndoc = \"An axis2 action, here named move: a vector no longer than 1\"\n\n[actions.pointer]\ntype = \"pointer\"\ndoc = \"A pointer action, here named pointer\"\n";

/// What a component needs besides the engine: the optional module it is
/// in, or the project file it comes with (by the crate that declares it).
const NEEDS: &[(&str, &str)] = &[
    ("argus-rigid", "Only in a game whose argus.toml lists `modules = [\"rigid\"]`."),
    ("argus-input", "Only in a game with an `input.toml`."),
    ("input.toml", "Only in a game with an `input.toml`; its fields are the game's actions (input.toml, above): here one of each type, named jump, zoom, move, and pointer, beside the engine's `device`. A button's field is true while it is held, its `_pressed` and `_released` fields on the tick it went down or up."),
];

/// An empty game, assembled as every game is (`argus_player::assemble`),
/// so its world holds exactly the components the engine registers, and
/// its scheduler the engine's systems: with `full`, every optional module
/// and an input map; without, neither. It is written to a temporary
/// directory and removed.
pub fn engine_world(full: bool) -> Result<(World, argus_sim::Scheduler), String> {
    let dir = std::env::temp_dir().join(format!(
        "argus-docs-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let modules = if full {
        argus_scene::project::MODULES
            .iter()
            .map(|m| format!("{m:?}"))
            .collect::<Vec<_>>()
            .join(", ")
    } else {
        String::new()
    };
    let argus_toml = format!(
        "name = \"engine\"\nclock = \"continuous@60\"\ndefault_scene = \"s\"\nmodules = [{modules}]\n"
    );
    let mut files = vec![
        ("argus.toml", argus_toml.as_str()),
        ("scenes/s/scene.toml", "seed = 1\n"),
    ];
    if full {
        files.push(("input.toml", ENGINE_INPUT));
    }
    let made = (|| -> std::io::Result<()> {
        for (rel, text) in &files {
            let p = dir.join(rel);
            std::fs::create_dir_all(p.parent().unwrap_or(&dir))?;
            std::fs::write(p, text)?;
        }
        Ok(())
    })();
    let out = made.map_err(|e| e.to_string()).and_then(|()| {
        let cfg = argus_scene::ProjectConfig::load(&dir).map_err(|e| e.message)?;
        let a = argus_player::assemble(
            &dir,
            &cfg,
            "s",
            None,
            &argus_player::Options {
                audio: false,
                ..Default::default()
            },
        )
        .map_err(|e| e.message)?;
        Ok((a.world, a.sched))
    });
    let _ = std::fs::remove_dir_all(&dir);
    out
}

/// Where a component is declared, as a reader names it: the engine crate
/// (`argus-physics`), or the project file (`input.toml`).
fn declared_in(source: &str) -> String {
    let path = source.split(':').next().unwrap_or(source);
    path.split(['/', '\\'])
        .find(|p| p.starts_with("argus-"))
        .unwrap_or(path)
        .to_string()
}

/// The engine's components as Markdown, headings from `level`: each with
/// its doc, its fields (type, default, range, tune, doc), and the engine
/// systems that write and read it, from the registry of an empty game.
pub fn components(level: usize) -> String {
    let h = "#".repeat(level);
    let mut o = String::from(
        "Every component the engine declares, read from the registry of an empty game assembled as every game is: what entity files and prefabs write as `[Component]` tables, and what scripts `q:each`, `get`, and `insert`. A game's own are in its ARGUS.md. A field's default is how an entity file writes it; one left out of a file keeps it. Written by and read by name the engine's systems; a game's systems that touch it are in `argus describe <Component>`.\n\n",
    );
    let (world, mut sched) = match engine_world(true) {
        Ok(w) => w,
        Err(e) => {
            let _ = writeln!(
                o,
                "The list could not be made here: {e}. `argus describe` in any project lists them.\n"
            );
            return o;
        }
    };
    let registry = world.registry().clone();
    for s in registry.schemas() {
        let from = declared_in(s.source.as_deref().unwrap_or(""));
        let _ = writeln!(o, "{h} {}\n", s.name);
        let _ = writeln!(o, "{}.\n", s.doc.trim_end_matches('.'));
        if let Some((_, need)) = NEEDS.iter().find(|(c, _)| *c == from) {
            let _ = writeln!(o, "{need}\n");
        }
        if s.fields.is_empty() {
            o.push_str("A tag: it has no fields.\n\n");
        } else {
            fields_table(&mut o, &s.fields);
        }
        let id = registry.id(&s.name).expect("registered");
        let list = |v: Vec<String>| {
            if v.is_empty() {
                "no engine system".to_string()
            } else {
                v.iter()
                    .map(|n| format!("`{n}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        };
        let w = list(sched.who_writes(id).unwrap_or_default());
        let r = list(sched.who_reads(id).unwrap_or_default());
        let by = if from.starts_with("argus-") {
            format!("Declared by {from}")
        } else {
            format!("Made from the game's {from}")
        };
        let _ = writeln!(o, "{by}. Written by {w}; read by {r}.\n");
    }
    o
}

/// A field's default as an entity file writes it.
fn toml_value(v: &Value, ty: FieldType) -> String {
    let num = |f: f64| {
        if ty == FieldType::F32 {
            argus_ecs::schema::shortest_f64(f as f32).to_string()
        } else {
            f.to_string()
        }
    };
    match v {
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::UInt(u) => u.to_string(),
        Value::Float(f) => num(*f),
        Value::Vec2(p) => format!(
            "[{}, {}]",
            argus_ecs::schema::shortest_f64(p.x),
            argus_ecs::schema::shortest_f64(p.y)
        ),
        Value::Str(s) | Value::Ref(s) => format!("{s:?}"),
        Value::Entity(_) => "\"\"".to_string(),
        Value::List(items) => {
            let of = match ty {
                FieldType::List(of) => of.ty(),
                other => other,
            };
            let each: Vec<String> = items.iter().map(|x| toml_value(x, of)).collect();
            format!("[{}]", each.join(", "))
        }
    }
}

/// The tests read each parser's source, as the scripting reference's read
/// the Luau binding: every key-like string literal in a parser is a key
/// its file's reference lists, or a value it names.
#[cfg(test)]
mod tests {
    use super::*;

    /// A Rust source as the compiler sees it: string literals (their
    /// contents), brackets, semicolons, and the rest; comments dropped.
    #[derive(Debug, PartialEq)]
    enum Tok {
        Str(String),
        Open,
        Close,
        Semi,
        Other,
    }

    fn lex(src: &str) -> Vec<(usize, Tok)> {
        let b = src.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
        while i < b.len() {
            let c = b[i];
            if c == b'/' && b.get(i + 1) == Some(&b'/') {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            } else if c == b'/' && b.get(i + 1) == Some(&b'*') {
                i += 2;
                while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                    i += 1;
                }
                i += 2;
            } else if c == b'r'
                && (i == 0 || !ident(b[i - 1]))
                && matches!(b.get(i + 1), Some(b'"') | Some(b'#'))
            {
                // A raw string: r"..." or r#"..."#.
                let mut j = i + 1;
                let mut hashes = 0;
                while b.get(j) == Some(&b'#') {
                    hashes += 1;
                    j += 1;
                }
                if b.get(j) != Some(&b'"') {
                    out.push((i, Tok::Other));
                    i += 1;
                    continue;
                }
                let start = j + 1;
                let close = format!("\"{}", "#".repeat(hashes));
                let end = src[start..].find(&close).map_or(b.len(), |e| start + e);
                out.push((i, Tok::Str(src[start..end].to_string())));
                i = end + close.len();
            } else if c == b'"' {
                let start = i + 1;
                let mut j = start;
                while j < b.len() && b[j] != b'"' {
                    if b[j] == b'\\' {
                        j += 1;
                    }
                    j += 1;
                }
                out.push((i, Tok::Str(src[start..j.min(b.len())].to_string())));
                i = j + 1;
            } else if c == b'\'' {
                // A char literal ('x', '\n', '{'), or a lifetime.
                if b.get(i + 1) == Some(&b'\\') {
                    let end = src[i + 2..].find('\'').map_or(b.len(), |e| i + 2 + e);
                    i = end + 1;
                } else {
                    let ch = src[i + 1..].chars().next().map_or(1, char::len_utf8);
                    if b.get(i + 1 + ch) == Some(&b'\'') {
                        i += ch + 2;
                    } else {
                        i += 1;
                    }
                }
                out.push((i, Tok::Other));
            } else {
                let t = match c {
                    b'{' | b'(' | b'[' => Tok::Open,
                    b'}' | b')' | b']' => Tok::Close,
                    b';' => Tok::Semi,
                    _ => Tok::Other,
                };
                out.push((i, t));
                i += 1;
            }
        }
        out
    }

    /// The item whose text begins with `sig`: a function to its closing
    /// brace, a const to its semicolon. A panic names a source that
    /// changed shape.
    fn body<'a>(src: &'a str, sig: &str, file: &str) -> &'a str {
        let at = src.find(sig).unwrap_or_else(|| {
            panic!("{file} has no `{sig}`: the parser moved, so point the test (and check the reference) at where it went")
        });
        let mut depth = 0i32;
        for (i, t) in lex(&src[at..]) {
            match t {
                Tok::Open => depth += 1,
                Tok::Close => {
                    depth -= 1;
                    if depth == 0 && src[at + i..].starts_with('}') {
                        return &src[at..at + i + 1];
                    }
                }
                Tok::Semi if depth == 0 => return &src[at..at + i + 1],
                _ => {}
            }
        }
        panic!("{file}'s `{sig}` does not end");
    }

    /// Every string literal in `code` that could be a key: one word of
    /// letters, digits, `_`, and `-` (a message has spaces; a path dots).
    fn literals(code: &str) -> Vec<String> {
        let mut out: Vec<String> = lex(code)
            .into_iter()
            .filter_map(|(_, t)| match t {
                Tok::Str(s) => Some(s),
                _ => None,
            })
            .filter(|s| {
                s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                    && s.chars().any(|c| c.is_ascii_alphabetic())
            })
            .collect();
        out.sort();
        out.dedup();
        out
    }

    fn words(s: &str) -> Vec<String> {
        s.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
            .filter(|w| !w.is_empty())
            .map(str::to_string)
            .collect()
    }

    /// The words of a file's key column.
    fn key_words(f: &FileDoc) -> Vec<String> {
        f.keys.iter().flat_map(|k| words(k.name)).collect()
    }

    /// Everything a file's reference says.
    fn text_of(f: &FileDoc) -> String {
        format!(
            "{} {} {} {} {}",
            f.title,
            f.intro,
            f.errors,
            f.keys
                .iter()
                .map(|k| format!("{} {} {} {}", k.name, k.ty, k.default, k.doc))
                .collect::<Vec<_>>()
                .join(" "),
            f.example
        )
    }

    fn file(title: &str) -> &'static FileDoc {
        FILES
            .iter()
            .find(|f| f.title.starts_with(title))
            .unwrap_or_else(|| panic!("no file {title:?} in the reference"))
    }

    /// Whether the reference's text has `w` as a word (or as a backticked
    /// phrase).
    fn mentions(text: &str, w: &str) -> bool {
        words(text).iter().any(|x| x == w) || text.contains(&format!("`{w}`"))
    }

    /// A parser: the source file, its text, and the items that read keys.
    type Parser<'a> = (&'a str, &'a str, &'a [&'a str]);

    /// Every key-like literal in each parser item is a key the file's
    /// reference lists, or one of `values` (literals a parser compares a
    /// value with, which the reference must still mention).
    fn check(title: &str, parsers: &[Parser], values: &[&str]) {
        let f = file(title);
        let keys = key_words(f);
        let text = text_of(f);
        let mut seen = 0;
        for (file, src, sigs) in parsers {
            for sig in *sigs {
                for lit in literals(body(src, sig, file)) {
                    seen += 1;
                    // A crate's name is a source label, not a key.
                    if lit.starts_with("argus-") {
                        continue;
                    }
                    if values.contains(&lit.as_str()) {
                        assert!(
                            mentions(&text, &lit),
                            "{title}: the value {lit:?} ({file}, `{sig}`) is not in the reference (formats.rs)"
                        );
                    } else {
                        assert!(
                            keys.contains(&lit),
                            "{title}: {file}'s `{sig}` reads {lit:?}, which the reference (formats.rs) does not list as a key"
                        );
                    }
                }
            }
        }
        assert!(seen > 0, "{title}: no keys found in its parsers");
    }

    /// Every name in `names` is in the file's reference.
    fn all_mentioned(title: &str, names: &[&str], what: &str) {
        let text = text_of(file(title));
        for n in names {
            assert!(
                mentions(&text, n),
                "{title}: the {what} {n:?} is not in the reference (formats.rs)"
            );
        }
    }

    #[test]
    fn the_lexer_finds_literals_and_ends_items() {
        let src = "fn a() { let x = t.get(\"key\"); if c == '{' { \"{not}\".len() } // \"no\"\n }\nconst K: &[&str] = &[\"k1\", \"k-2\"];\nfn b() {}";
        let a = body(src, "fn a(", "test");
        assert!(a.ends_with('}') && a.len() == src.find("\nconst").unwrap());
        assert_eq!(literals(a), vec!["key"]);
        assert_eq!(literals(body(src, "const K", "test")), vec!["k-2", "k1"]);
    }

    #[test]
    fn argus_toml() {
        check(
            "argus.toml",
            &[(
                "argus-scene/src/project.rs",
                include_str!("../../argus-scene/src/project.rs"),
                &["fn parse(text: &str)"],
            )],
            &["en"],
        );
        let doc = file("argus.toml").keys.iter().find(|k| k.name == "modules");
        for m in argus_scene::project::MODULES {
            assert!(
                doc.is_some_and(|k| k.doc.contains(&format!("`{m}`"))),
                "argus.toml: the module {m} is not in `modules` (formats.rs)"
            );
        }
    }

    #[test]
    fn scene_room_and_entity_files() {
        let scene = include_str!("../../argus-scene/src/scene.rs");
        check(
            "scenes/<scene>/scene.toml",
            &[("argus-scene/src/scene.rs", scene, &["fn parse_scene_doc("])],
            &["random"],
        );
        check(
            "scenes/<scene>/rooms/<room>/room.toml",
            &[("argus-scene/src/scene.rs", scene, &["fn parse_room_doc("])],
            &[],
        );
        let doc = include_str!("../../argus-scene/src/doc.rs");
        check(
            "scenes/<scene>/<id>.toml",
            &[("argus-scene/src/doc.rs", doc, &["fn from_table("])],
            &[],
        );
        // A prefab file is an entity file: its reference names the same keys.
        let prefab = text_of(file("prefabs/"));
        for k in [
            "prefab",
            "tags",
            "-tags",
            "-children",
            "children",
            "-Component",
        ] {
            assert!(prefab.contains(k), "prefabs/: {k}");
        }
        check(
            "*.map.toml",
            &[(
                "argus-scene/src/bulk.rs",
                include_str!("../../argus-scene/src/bulk.rs"),
                &["pub fn open(root: &Path, rel: &str)"],
            )],
            &["default"],
        );
    }

    #[test]
    fn input_toml() {
        let map = include_str!("../../argus-input/src/map.rs");
        check(
            "input.toml",
            &[(
                "argus-input/src/map.rs",
                map,
                &["pub fn parse(file: &str, text: &str)", "const DEVICES"],
            )],
            &["position", "wheel"],
        );
        // The action types, and every name the resolver gives a meaning.
        check(
            "input.toml",
            &[
                (
                    "argus-input/src/map.rs",
                    map,
                    &["fn parse(s: &str) -> Option<ActionKind>"],
                ),
                (
                    "argus-input/src/devices.rs",
                    include_str!("../../argus-input/src/devices.rs"),
                    &["pub fn resolve(", "pub fn key_name("],
                ),
            ],
            &[
                "button",
                "axis",
                "axis2",
                "pointer",
                "wasd",
                "arrows",
                "ijkl",
                "w",
                "a",
                "s",
                "d",
                "i",
                "j",
                "k",
                "l",
                "up",
                "down",
                "left",
                "right",
                "left_stick",
                "right_stick",
                "dpad",
                "left_x",
                "left_y",
                "right_x",
                "right_y",
                "dpad_up",
                "dpad_down",
                "dpad_left",
                "dpad_right",
                "lt",
                "rt",
                "esc",
                "escape",
                "return",
                "enter",
                "Key",
                "Digit",
                "Arrow",
                "Code",
                "numpadenter",
            ],
        );
        use argus_input::devices::{MOUSE_NAMES, PAD_NAMES, PLATFORM_NAMES};
        for (device, names) in [
            ("gamepad", PAD_NAMES),
            ("mouse", MOUSE_NAMES),
            ("platform", PLATFORM_NAMES),
        ] {
            let k = file("input.toml")
                .keys
                .iter()
                .find(|k| k.name.ends_with(&format!("bindings] {device}")))
                .unwrap_or_else(|| panic!("input.toml: no {device} bindings key"));
            for n in names {
                assert!(
                    k.doc.contains(&format!("`{n}`")),
                    "input.toml: the {device} name {n:?} is not in the reference (formats.rs)"
                );
            }
        }
        all_mentioned(
            "input.toml",
            &[argus_input::map::DEVICE_ACTION],
            "engine action",
        );
        // Touch controls (reaching players item 1): every key the parser
        // reads, the kinds, and every anchor.
        check(
            "input.toml",
            &[(
                "argus-input/src/touch.rs",
                include_str!("../../argus-input/src/touch.rs"),
                &["pub fn parse(file: &str"],
            )],
            &["button", "stick", "center"],
        );
        let anchor = file("input.toml")
            .keys
            .iter()
            .find(|k| k.name == "[touch.<name>] anchor")
            .expect("input.toml: no [touch.<name>] anchor key");
        for a in argus_input::touch::ANCHORS {
            assert!(
                anchor.doc.contains(&format!("`{a}`")),
                "input.toml: the anchor {a:?} is not in the reference (formats.rs)"
            );
        }
    }

    #[test]
    fn def_tables_dialogue_and_timelines() {
        // A def table's own keys are declared in Luau (the scripting
        // reference); its file is rows by key.
        all_mentioned("defs/<table>.toml", &["key", "prefix", "id"], "def key");
        all_mentioned("defs/dialogue.toml", argus_dialogue::graph::ROLES, "role");
        all_mentioned("defs/timeline.toml", argus_timeline::table::ROLES, "role");
        check(
            "defs/timeline.toml",
            &[(
                "argus-timeline/src/table.rs",
                include_str!("../../argus-timeline/src/table.rs"),
                &["fn parse(s: &str) -> Option<Kind>"],
            )],
            &["tween", "anim", "camera", "line", "event"],
        );
        // Every easing is linear, step, or a curve with in_, out_, in_out_.
        let t = text_of(file("defs/timeline.toml"));
        for e in argus_tween::ease::NAMES {
            let base = e
                .strip_prefix("in_out_")
                .or_else(|| e.strip_prefix("in_"))
                .or_else(|| e.strip_prefix("out_"))
                .unwrap_or(e);
            assert!(
                mentions(&t, base),
                "defs/timeline.toml: the easing {e} is not in the reference (formats.rs)"
            );
        }
        // The rows' fields come from the schemas themselves.
        let md = files(3);
        for (_, schema) in ENGINE_TABLES {
            for f in schema().schema.fields {
                assert!(md.contains(&format!("| `{}` |", f.name)), "{}", f.name);
            }
        }
    }

    #[test]
    fn strings_and_ui_toml() {
        let engine: Vec<&str> = argus_ui::strings::ENGINE.iter().map(|(k, _)| *k).collect();
        let t = text_of(file("strings/"));
        for k in engine {
            assert!(
                t.contains(&format!("`{k}`")),
                "strings: the engine's key {k}"
            );
        }
        check(
            "ui.toml",
            &[(
                "argus-ui/src/theme.rs",
                include_str!("../../argus-ui/src/theme.rs"),
                &["pub fn parse(text: &str)"],
            )],
            &[],
        );
        let colors: Vec<&str> = argus_ui::theme::DEFAULT_COLORS
            .iter()
            .map(|(n, _, _)| *n)
            .collect();
        all_mentioned("ui.toml", &colors, "colour token");
    }

    #[test]
    fn audio_toml_and_sound_sidecars() {
        let src = include_str!("../../argus-audio/src/config.rs");
        check(
            "audio.toml",
            &[(
                "argus-audio/src/config.rs",
                src,
                &["fn tone_from(", "fn sound_from(", "fn parse_on("],
            )],
            &[
                "master", "music", "sfx", "ui", "ambience", "sine", "square", "saw", "triangle",
                "noise", "bar", "beat", "now",
            ],
        );
        // A sound sidecar takes a sound table's keys.
        let sidecar = text_of(file("assets/<name>.toml with type = \"sound\""));
        for k in literals(body(src, "fn sound_from(", "argus-audio/src/config.rs")) {
            if k != "music" && k != "sfx" {
                assert!(mentions(&sidecar, &k), "sound sidecar: {k}");
            }
        }
    }

    #[test]
    fn platform_toml() {
        check(
            "platform.toml",
            &[(
                "argus-platform/src/config.rs",
                include_str!("../../argus-platform/src/config.rs"),
                &["pub fn parse(file: &str, text: &str)"],
            )],
            &[],
        );
    }

    #[test]
    fn assets_and_their_sidecars() {
        let lib = include_str!("../../argus-assets/src/lib.rs");
        // Every source extension the importer groups is in the overview.
        let overview = text_of(file("assets/:"));
        let groups = body(lib, "fn group_files<", "argus-assets/src/lib.rs");
        for s in lex(groups).into_iter().filter_map(|(_, t)| match t {
            Tok::Str(s) if s.starts_with('.') => Some(s),
            _ => None,
        }) {
            assert!(
                overview.contains(&s),
                "assets/: the importer reads {s} files, which the reference does not say"
            );
        }
        let sidecar = include_str!("../../argus-assets/src/sidecar.rs");
        // Each `type`, and what a sidecar alone is told apart by.
        check(
            "assets/:",
            &[("argus-assets/src/sidecar.rs", sidecar, &["pub fn kind_of("])],
            &[
                "type",
                "sprite",
                "sound",
                "animator",
                "emitter",
                "material",
                "font",
                "states",
                "initial",
                "rate",
                "burst",
                "lifetime",
                "shader",
                "params",
                "blend",
                "variations",
                "tone",
                "file",
                "bus",
                "kind",
                "grid",
                "frames",
                "clips",
                "pivot",
                "color",
            ],
        );
        check(
            "assets/<name>.toml beside a .png",
            &[(
                "argus-assets/src/sidecar.rs",
                sidecar,
                &["const SPRITE_KEYS", "fn parse_sprite_with("],
            )],
            &[
                "rect",
                "circle",
                "forward",
                "reverse",
                "pingpong",
                "ping-pong",
                "single",
                "4bit",
                "blob47",
                "blob",
                "connect",
                "open",
            ],
        );
        // The tile properties the engine reads are a sprite's.
        check(
            "assets/<name>.toml beside a .png",
            &[(
                "argus-tiles/src/lib.rs",
                include_str!("../../argus-tiles/src/lib.rs"),
                &["pub fn collide_of(", "fn truthy("],
            )],
            &["solid", "slope", "one_way", "true", "1", "yes"],
        );
        let animator = include_str!("../../argus-assets/src/animator.rs");
        check(
            "assets/<name>.toml with type = \"animator\"",
            &[(
                "argus-assets/src/animator.rs",
                animator,
                &["pub fn parse_animator(", "fn transitions("],
            )],
            &[],
        );
        check(
            "assets/<name>.toml with type = \"animator\"",
            &[(
                "argus-assets/src/animator.rs",
                animator,
                &["pub fn parse_condition("],
            )],
            &[
                "and", "not", "abs", "end", "time", "frame", "loops", "true", "false", "or",
            ],
        );
        let effect = include_str!("../../argus-assets/src/effect.rs");
        check(
            "assets/<name>.toml with type = \"emitter\"",
            &[(
                "argus-assets/src/effect.rs",
                effect,
                &["const KEYS", "pub fn parse_emitter("],
            )],
            &["rect", "circle", "point", "box", "world", "local"],
        );
        for k in argus_assets::KNOBS {
            let doc = file("assets/<name>.toml with type = \"emitter\"")
                .keys
                .iter()
                .find(|x| x.name == k.key);
            assert!(
                doc.is_some_and(|d| d.doc.contains("(tune)")),
                "emitter: the knob {} is not marked tune (formats.rs)",
                k.key
            );
        }
        let material = include_str!("../../argus-material/src/lib.rs");
        check(
            "assets/<name>.wgsl",
            &[(
                "argus-material/src/lib.rs",
                material,
                &[
                    "const KEYS",
                    "const PARAM_KEYS",
                    "fn parse(s: &str) -> Option<ParamType>",
                ],
            )],
            &[
                "f32", "float", "number", "vec2", "vec4", "color", "colour", "texture",
            ],
        );
        all_mentioned(
            "assets/<name>.wgsl",
            &argus_material::BUILTIN_NAMES,
            "built-in",
        );
        check(
            "assets/<name>.ttf",
            &[(
                "argus-assets/src/font.rs",
                include_str!("../../argus-assets/src/font.rs"),
                &["fn options("],
            )],
            &["font"],
        );
        // A skeleton is any .json; Aseprite files are .aseprite and .ase.
        assert!(argus_assets::spine::is_skeleton_file("puppet/puppet.json"));
        all_mentioned("assets/<name>.json", &["skeleton"], "section");
        all_mentioned("assets/<name>.aseprite", &[".ase", "layers"], "word");
    }

    #[test]
    fn tilemaps_tiled_and_nav_grids() {
        check(
            "*.tilemap.toml",
            &[(
                "argus-tiles/src/map.rs",
                include_str!("../../argus-tiles/src/map.rs"),
                &["pub fn open(root: &Path, rel: &str)"],
            )],
            &["tilemap"],
        );
        // A Tiled file's custom properties the engine reads.
        let tiled = include_str!("../../argus-assets/src/tiled.rs");
        let t = text_of(file("Tiled maps"));
        let mut found = 0;
        for (i, _) in tiled.match_indices(".get(\"") {
            let before = tiled[..i].trim_end();
            if before.ends_with("props") || before.ends_with("properties") {
                let rest = &tiled[i + 6..];
                let name = &rest[..rest.find('"').unwrap_or(0)];
                found += 1;
                assert!(
                    t.contains(&format!("`{name}`")),
                    "Tiled: the custom property {name:?} is not in the reference (formats.rs)"
                );
            }
        }
        assert!(found >= 4, "the Tiled reader's properties changed shape");
        check(
            "*.navgrid.toml",
            &[(
                "argus-nav/src/bake.rs",
                include_str!("../../argus-nav/src/bake.rs"),
                &["pub fn read(root: &Path, index: &str)"],
            )],
            &[],
        );
    }

    #[test]
    fn mods_plugins_settings_and_templates() {
        let mods = include_str!("../../argus-player/src/mods.rs");
        check(
            "mods/<id>/mod.toml",
            &[(
                "argus-player/src/mods.rs",
                mods,
                &["pub fn parse(file: &str, text: &str)"],
            )],
            &[],
        );
        check(
            "mods/load-order.toml",
            &[("argus-player/src/mods.rs", mods, &["pub fn read_order("])],
            &[],
        );
        check(
            "settings.toml",
            &[(
                "argus-player/src/settings.rs",
                include_str!("../../argus-player/src/settings.rs"),
                &["pub fn from_toml("],
            )],
            &[],
        );
        let build = include_str!("../../argus-plugins/src/build.rs");
        assert!(build.contains("\"cdylib\"") && build.contains("Cargo.toml"));
        all_mentioned(
            "plugins/<name>/",
            &["cdylib", "Cargo.toml", "argus-plugin"],
            "word",
        );
        let scaffold = include_str!("../../argus-cli/src/scaffold.rs");
        assert!(
            scaffold.contains("\"template.toml\"")
                && scaffold.contains("strip_prefix(\"about = \")"),
            "argus new reads template.toml differently now: update its reference (formats.rs)"
        );
    }

    /// Every file name the engine's loaders open is a file the reference
    /// describes.
    #[test]
    fn every_file_the_loaders_open_is_in_the_reference() {
        let sources = [
            include_str!("../../argus-scene/src/project.rs"),
            include_str!("../../argus-scene/src/scene.rs"),
            include_str!("../../argus-input/src/map.rs"),
            include_str!("../../argus-ui/src/theme.rs"),
            include_str!("../../argus-audio/src/config.rs"),
            include_str!("../../argus-platform/src/config.rs"),
            include_str!("../../argus-player/src/mods.rs"),
            include_str!("../../argus-player/src/settings.rs"),
            include_str!("../../argus-cli/src/scaffold.rs"),
        ];
        let titles: Vec<&str> = FILES.iter().map(|f| f.title).collect();
        let mut n = 0;
        for src in sources {
            for (_, t) in lex(src) {
                let Tok::Str(s) = t else { continue };
                if s.ends_with(".toml") && !s.contains(['/', ' ', '{']) {
                    n += 1;
                    assert!(
                        titles.iter().any(|t| t.contains(&s)),
                        "the engine reads {s}, which the reference (formats.rs) does not describe"
                    );
                }
            }
        }
        assert!(n >= 10, "only {n} file names found");
    }

    #[test]
    fn the_engine_components_come_from_its_registry() {
        let (full, _) = engine_world(true).expect("the empty game assembles");
        let (bare, _) = engine_world(false).expect("the bare game assembles");
        let md = components(3);
        for s in full.registry().schemas() {
            assert!(md.contains(&format!("### {}\n", s.name)), "{}", s.name);
        }
        for c in [
            "Transform",
            "Sprite",
            "Camera",
            "Collider",
            "KinematicBody",
            "Tilemap",
            "UiRoot",
            "Light",
            "RigidBody",
            "Actions",
        ] {
            assert!(md.contains(&format!("### {c}\n")), "{c}");
        }
        // What only some games have is said to be so, and nothing else.
        for s in full.registry().schemas() {
            let only = bare.registry().id(&s.name).is_none();
            let from = declared_in(s.source.as_deref().unwrap_or(""));
            assert_eq!(
                only,
                NEEDS.iter().any(|(c, _)| *c == from),
                "{} ({from}): is it in every game?",
                s.name
            );
        }
    }

    #[test]
    fn the_reference_reads_as_plain_markdown() {
        let all = format!("{}{}", files(3), components(3));
        assert!(!all.contains('\u{2014}'), "no em dashes");
        for f in FILES {
            assert!(!f.title.is_empty() && !f.intro.is_empty(), "{}", f.title);
            if f.title.contains(".art.luau") {
                // An art script's example draws what it says.
                if let Err(e) = argus_assets::art::run("example.art.luau", f.example) {
                    panic!("{}'s example does not draw: {}", f.title, e.message);
                }
            } else if !f.example.is_empty() {
                assert!(
                    toml_edit::ImDocument::parse(f.example.to_string()).is_ok(),
                    "{}'s example is not TOML",
                    f.title
                );
            }
        }
    }
}
