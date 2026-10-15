# Releasing

This chapter is about getting a game to players: `argus build` and the check it makes, the web page and serving it, a release for the other platform, patches, mods, Rust plugins, the platform layer and Steam, reports from players' builds, and what a player's machine needs.

## argus build

```sh
argus build
```

This makes a release for the machine you are on, in `dist/<target>/` inside the project (`dist/windows` on Windows, `dist/linux` on Linux). The output of a build of the blank template:

```text
built blank for windows (release) in <project>\dist\windows
  packs: 8 files, 2125 bytes; this build ships 8 new blobs, 2125 bytes
  release forms: 0 sprites on 0 pages (0 BC7), 2 entity files in 1 bundles, 0 def tables, 0 sounds as Vorbis
  executable: <project>\dist\windows\blank.exe
  check: the build plays and looks as the project does (the same world after 300 ticks, drawn the same)
```

What the directory holds:

| File | What it is |
|---|---|
| `<name>.exe` (or `<name>` on Linux) | The game, named after `argus.toml`'s `name`. What a player double-clicks. |
| `data/game.pack` | The game: scripts compiled to bytecode, scenes, defs, and assets. |
| `data/lang-<code>.pack` | One pack per language other than the base: `strings/<code>.toml`, and anything under `lang/<code>/`. |
| `plugins/` | The game's Rust plugins, built, when it has any. |
| `steam_api64.dll` (`libsteam_api.so` on Linux) | Steam's library, when `argus.toml` lists `modules = ["steam"]`. |
| `build.json` | What was made, and the check's result. |

The packs are built from the import cache, not copied from the folder. Sprites are packed onto texture pages, some of them compressed (BC7) where no pixel would show the difference. Entity files, prefabs, and def tables are in a binary form of the same data. WAV sounds are stored as Ogg Vorbis. Left out of the packs: `tests/`, `tools/`, `dist/`, `mods/`, `plugins/` (their source), the notes at the top for people and agents (`README.md`, `ARGUS.md`, `AGENTS.md`, `CLAUDE.md`, `GEMINI.md`, `template.toml`), and anything whose name starts with a dot (`.argus/`, `.git/`). Also left out: whatever git ignores, when the project is in a git repository (a `node_modules/`, play sessions, a scratch folder), and whatever `[build] exclude` in `argus.toml` names:

```toml
[build]
exclude = ["m0", "notes/**", "**/*.psd"]
```

Each entry is a pattern on the path from the project's folder: `*` matches within one folder, `**` across folders, and a pattern that names a folder leaves out everything in it. A sprite made from an image left out is left out with it. Everything else in the project goes in. The build's report lists what it left out under `excluded`, by reason (`[build] exclude` or `.gitignore`), so `argus build` output shows a folder that should have shipped and did not.

A failed asset import or a script error stops the build, naming each one.

A release build has no daemon in it: no socket, no `.argus/`, nothing an agent or editor can attach to. What stays is the bounded recorder for player reports (below) and save migrations, so a patched game loads old saves.

### The check

Before it says done, the build checks itself:

1. The project's own scene is run for 300 ticks from the project's folder, and again from the packs it just made. The two worlds' digests must match.
2. The executable is run headless (`<name>.exe --headless 300`) and must reach the same digest.
3. One frame is drawn in software from the project and from the packs, and the two pictures are compared. An edge moved by one pixel is forgiven; a changed colour is not.

If the pictures differ, `look-project.png`, `look-build.png`, and `look-diff.png` are written beside `build.json`, and the build fails. The `check` block of `build.json` has every digest:

```json
"check": {
  "executable": "01b162584ec033bc",
  "looks": { "differing": 0, "pixels": 230400 },
  "packs": "01b162584ec033bc",
  "project": "01b162584ec033bc",
  "said": "the build plays and looks as the project does (the same world after 300 ticks, drawn the same)",
  "same": true,
  "ticks": 300
}
```

A build for the other platform cannot run its executable here, so its check is the packs' (step 1 and 3), and `build.json` says the executable was not run.

### Flags

| Flag | What it does |
|---|---|
| `--target windows\|linux\|macos\|web` | What to build for. The default is this machine's. |
| `--out <dir>` | Where to write. The default is `<project>/dist/<target>`. |
| `--since <dir>` | Make a patch against an earlier build (below). |
| `--release` | Optimised. A prebuilt runtime always is, so from an install this changes nothing. |
| `--from-source` | Compile the runtime from the engine's source instead (engine work; needs Rust and the source tree). |
| `--mod <id>` | Build a mod's pack instead (below). |
| `--json` | The report as JSON. |

### Where the executable comes from

An Argus install carries prebuilt game runtimes in `runtime/` beside `bin/` (or wherever `$ARGUS_RUNTIME` points): `runtime/<os>/standard/` (every module but Steam), `runtime/<os>/steam/` (with Steam's library), and `runtime/web/`. `argus build` puts your packs beside the right one, so no compiler is needed. A runtime from another Argus version is refused, with a hint to reinstall.

Run from a checkout of the engine, with `--from-source`, or with `$ARGUS_ENGINE` set, `argus build` compiles the runtime from source with cargo instead, which is slow the first time. There `--release` matters: without it the executable is a debug build.

## The web

```sh
argus build --target web
```

This writes `dist/web/`: `index.html`, `main.js`, `pkg/` (the wasm module), and `data/` (the packs and `packs.json`). The page's title is the game's name, and under the game it lists the game's controls, each action's keyboard keys from `input.toml` (the menu's own `ui_` actions left out). The check compares the packs' world and picture with the project's; the page itself is not run.

The page must be served over HTTP; opening `index.html` as a file does not work. A static web server should do, as long as it serves `.wasm` as `application/wasm` (Python's does); the engine's own checks use the playtest server below. For a quick look on your own machine:

```sh
cd dist/web
python -m http.server 8000
```

Then open `http://localhost:8000/`. The page draws with WebGPU, and falls back to WebGL2 in a browser without it. Query parameters: `?scene=` and `?seed=` choose what loads, `?lang=fr` plays in a language pack, and `?webgl` forces WebGL2. The page's sound uses an AudioWorklet, which a browser allows only in a secure context: from `localhost`, or over HTTPS. Over plain HTTP from another machine it falls back to an older way, which a slow frame can interrupt. Players' settings and saves are kept in the browser.

To collect players' reports (F8) as well, serve the page with the playtest server from the engine's repository, which takes them at `POST /marks` and sends the page compressed:

```sh
node tools/web/serve.mjs dist/web 8765 127.0.0.1
```

It is not part of an Argus install yet. See [Playtesting](playtesting.md) for pulling its reports into marks, and [docs/playtest.md](../playtest.md) for a server on a tailnet with HTTPS.

A game with Rust plugins has no web build, since a page cannot load a library: `argus build --target web` says so and stops. Swarm is such a game.

### The game's own page script

A project's `web/` folder goes into the build beside the page. When it has a `game.js`, the page imports it once the game is running, with `window.argus.player` set. That is where a game talks to the world outside it: a server, an AI model, the browser's own storage. The script trades text with the game ([Talking to the host](gameplay.md#talking-to-the-host)):

```js
const { player } = window.argus;
setInterval(async () => {
  for (const { channel, text } of JSON.parse(player.take_sent())) {
    if (channel !== "gm.request") continue;
    const answer = await fetch("/gm", { method: "POST", body: text }).then((r) => r.text());
    player.message("gm.ruling", answer);
  }
}, 100);
```

`player.take_sent()` is what the game sent since the last call (`q:send`), as JSON `[{tick, channel, text}]`. The first call also has what the game sent before the script was loaded, a start screen's message from an `on_load` system, say. `player.message(channel, text)` sends the game a message and returns the turns it took. On a discrete clock that is 1: the turn runs inside the call, so what the game answered is in `take_sent()` straight after. On a continuous clock it is 0, and the scripts get the message on the next tick the page runs, within a frame. A player's report keeps the messages with the rest of the input, so a report replays without the outside world.

What else a page script has:

| On `window.argus` | What it is |
|---|---|
| `player.get(entity, path)` | The world as JSON: `player.get("@player", "Health.cur")` a field, `player.get("@player", "Health")` a component, `player.get("@player", "")` the whole entity, `{entity, components}`. Entities are spelled as the protocol spells them, `@id` or `e:12`. It reads any entity, switched off or not. |
| `player.query(components)` | Every entity with all these components, `player.query("Monster,Health")`, as a JSON list of `{entity, components}` with those components, in entity order. Switched-off entities are in, as in the protocol's `world.query`; a name with `!` leaves out the entities that have it: `player.query("Monster,!Inactive,!InactiveUnder")` is what a script's `q:each("Monster")` sees. |
| `player.semantic()` | What is on screen now, as JSON: the semantic frame an agent reads with `argus observe` and a screen reader is given (the UI's text and buttons in reading order, and the entities in view with a `Describe`). What a player sees. |
| `player.status()` | JSON: the tick, entity and event counts, the world's digest, frames drawn. |
| `player.key(code, pressed)` | A key down or up, by `KeyboardEvent.code` (`"KeyW"`, `"ControlLeft"`), as the page feeds the game its keys. Returns the turns it took. |
| `player.mouse(button, pressed)` | A mouse button (`"left"`, `"right"`, `"middle"`) down or up. A binding's modifiers are the keys held, so a script that clicks for a `ctrl+left` presses `player.key("ControlLeft", true)` first. Returns the turns it took. |
| `player.pointer(x, y, css_w, css_h)` | The mouse at a place on the canvas element, in its CSS pixels, with the element's CSS size: the pointer, on the UI canvas. |
| `player.wheel(lines)` | The wheel turned, lines up positive: it scrolls what is under the pointer. |
| `player.text(text)` | Text typed, for a text box: characters, and Backspace, Enter, Escape as their control characters. |
| `openReport()` | Opens the F8 report form, as F8 does. |
| `setRunning(false)`, `setRunning(true)` | Stops and restarts the game's frame loop (what the report form does while it is open). |

`player.digest_rows()` is the engine's own debug text for the digest checks, not for games to read.

These are how a page script feeds the game input of its own, as the page's listeners do the player's. The page itself listens to the canvas for `PointerEvent`s and `KeyboardEvent`s, so a script may also dispatch synthetic `PointerEvent`s there (`button: 2` for a right click); a synthetic `MouseEvent` is not heard. Every one is input like a real one: the run records it, so a report replays it.

The player's own Ctrl and click need nothing from a script. A pointer or wheel event says which of Ctrl, Shift, Alt, and Meta are held, and the page passes that on as the keys, so a `ctrl+left` binding works even when Ctrl was pressed while the page's own text box had the focus.

The page's own elements are the canvas (`#game`), the report form (`#report`, `#note`, `#cancel`), the status lines (`#said`, `#status`, `#controls`, `#f8`), and the screen reader's (`#reader-buttons`, `#reader-said`, `#reader-hud`). A script's own elements go in `#page`, an empty `<div>` below the canvas; style them with the script's own `<style>` element, and give them ids of the game's own.

The game hears keys only while the canvas has focus. A key pressed in a script's text box goes to the text box, F8 included, so a script with a text box opens the report form itself (`window.argus.openReport()`), and gives the canvas back its focus (`document.getElementById("game").focus()`) when the player is done typing.

## A release for the other platform

From an install, build for the other desktop platform with its runtime:

```sh
argus build --target linux     # on Windows
argus build --target windows   # on Linux
```

If the install lacks that runtime, the build says which ones it has:

```text
argus build: the install has no linux standard runtime in <install>\runtime (it has: web, windows standard, windows steam); reinstall Argus with that platform, or build from the engine's source with --from-source
```

Two limits. A game with Rust plugins is released on its own platform, since its plugins are compiled for the machine that builds them. And from source, a desktop build is made only on its own platform.

The macOS runtime is new in 0.0.5. It is one universal program, for Apple Silicon and Intel Macs, made on Linux, so every install can release a game for the Mac. It had not run on a Mac when it shipped; [docs/mac-testing.md](../mac-testing.md) is how it is being tested. A Mac release is the game's executable beside its `data/`, run from Terminal; it is not yet an `.app`, and it is signed ad hoc, not notarised, so a player who downloads it with a browser clears the mark first: `xattr -dr com.apple.quarantine <folder>`.

## Patches

When you change a released game, build again with `--since` pointing at the build you shipped. Only what changed goes into a patch pack:

```sh
argus build --out dist/v1                    # the first release
# ... change the game ...
argus build --out dist/v2 --since dist/v1    # the update
```

```text
built blank for windows (release) in dist/v2
  packs: 8 files, 2125 bytes; this build ships 1 new blobs, 350 bytes
```

`dist/v2/data/` then holds the old `game.pack` unchanged and a new `patch-2.pack` with only the changed files. Language packs are rebuilt whole. Packs are content-addressed, so a one-line script change ships a few hundred bytes. Keep each build you ship in its own directory, so the next patch has something to point at.

## Mods

Argus games take mods unless they say otherwise. A mod is a folder `mods/<id>/` in the project, shaped like a small project, with a `mod.toml`. From `examples/hollow/mods/brighter/mod.toml`:

```toml
id = "brighter"
name = "Brighter lanterns"
version = "1.0"
game = "hollow"
requires = ["lanterns"]
doc = "Bigger lanterns, and a keeper proud of them."
```

A mod's `components/`, `systems/`, `defs/`, `lib/`, `prefabs/`, `scenes/`, `assets/`, `strings/`, and `audio.toml` are laid over the game's. Everything it adds is named with its id first (`lanterns.keeper`, `lanterns_Lantern`, `lanterns/lantern`). A name the game already has is replaced instead: a def row field by field (a list it can add to with `"+field"` and take from with `-field`), a prefab, an entity file, an asset, a sound, or a string. A mod cannot redeclare components, systems, or def tables, change a `scene.toml` or `room.toml`, or touch `argus.toml`, `input.toml`, `ui.toml`, `platform.toml`, or plugins. A mod with any problem is left out whole, with every reason, and so is a mod that requires it. The [Reference](reference.md) has every key and rule (`mods/<id>/mod.toml` in `argus docs`).

### Mods while you work

The daemon loads only the mods a session names, in that order:

```sh
argus daemon start --mods lanterns,brighter
argus reload mods='[lanterns, brighter]'     # the scene again, with these mods
argus reload mods='[]'                       # and without
argus query mods=true                        # what each added and changed, and what was left out
```

A scenario names its mods with `mods = { "lanterns", "brighter" }` (see `examples/hollow/tests/mods.luau`).

### A mod's pack

```sh
argus build --mod lanterns
```

```text
built mod lanterns for hollow: <project>\dist\mods\lanterns.pack (27 files, 28425 bytes, 9 scripts compiled)
  loads with the game; what it adds: {"assets":3,"components":4,"def rows":14,"entities":9,"rooms":1,"scripts":9,"sounds":3,"strings":11}
  check: the game plays the same with the mod from its pack as from its directory (300 ticks)
```

It checks that the mod loads with the game (with the mods it requires before it), that nothing is left out, and that the game plays the same with the mod from its pack as from its folder. The game's own pack never holds `mods/`.

### Mods on a player's machine

A player puts mod packs (or mod folders) in the `mods/` folder of the game's save folder. Without a `load-order.toml` there, every mod loads, by id, each after the mods it requires. With one, only the mods it lists load, in its order:

```toml
order = ["lanterns", "brighter"]
```

A mod that cannot load is left out, and the game's bottom line says which and why. The game also takes `--mods <dir>` to read mods from elsewhere, and `--no-mods` for none. A game that wants no mods sets this in `argus.toml`, and then looks at no folder:

```toml
[mods]
enabled = false
```

## Rust plugins

A Rust plugin is a hot path written in Rust: a crate in `plugins/<name>/` that the daemon builds with cargo and loads beside the game's Luau. When one is worth it is in [Tuning and performance](tuning-and-performance.md#when-to-move-a-hot-path-into-a-rust-plugin).

List it in `argus.toml` (from `examples/swarm/argus.toml`):

```toml
plugins = ["swarm-hot"]
```

Its `Cargo.toml` makes a `cdylib`, depends on the SDK the engine writes into the project, and has an empty `[workspace]` of its own (`examples/swarm/plugins/swarm-hot/Cargo.toml`):

```toml
[package]
name = "swarm-hot"
version = "0.1.0"
edition = "2021"
publish = false

[lib]
crate-type = ["cdylib"]

[dependencies]
argus-plugin = { path = "../../.argus/plugins/sdk" }

[workspace]
```

Its `src/lib.rs` declares the components it uses, field for field as the game declares them, and its systems with the same declarations a Luau system has. Shortened from `examples/swarm/plugins/swarm-hot/src/lib.rs`:

```rust
use argus_plugin::prelude::*;

component! {
    /// components/swarm.luau's Enemy.
    pub struct Enemy { pub speed: f32, pub damage: i32, pub wave: u32, pub hold: bool }
}

fn chase(ctx: &Ctx) -> Result {
    let pe = ctx.entity("@player")?;
    let p = ctx.get::<Transform>(pe)?.pos;
    let dt = ctx.dt();
    ctx.each::<(&Enemy, &mut Transform)>(|_, (en, t)| {
        // ... move t.pos toward p ...
    })
}

plugin! {
    components: [Enemy, Player, Transform, Dasher],
    systems: [
        System::new("sim.chase", "sim")
            .after("sim.waves")
            .after("sim.dash")
            .reads(&["Enemy", "Transform", "Player"])
            .writes(&["Transform"])
            .run(chase),
    ],
}
```

How it works:

- **The SDK.** Before each build the engine writes the SDK it was built with into `.argus/plugins/sdk`, so a plugin always builds against the engine that loads it. It documents itself: `cargo doc --open --manifest-path .argus/plugins/sdk/Cargo.toml`.
- **Building.** The daemon builds each plugin, optimised, when the project opens and on `argus reload`, into `.argus/plugins/target`. A one-line change to Swarm's plugin builds in under half a second on the reference laptop, and is running about 40 ms later, without losing a frame.
- **Errors.** A compile error is a `script.errors` row with file and line (`argus script errors`), never a crash. A plugin that does not build keeps its last good build running. A panic stops only the system that panicked.
- **Checking.** Each component's layout is checked against the game's, field for field, before any plugin system runs.
- **Releasing.** `argus build` builds the plugins and puts each library in `plugins/` beside the executable.

A plugin needs Rust (from https://rustup.rs) on every machine that builds the game: yours, and any that runs the daemon or `argus build` for it. Without cargo, the build says the plugin needs Rust. Players need no Rust.

Limits: a game with plugins has no web build and is released on its own platform; mods cannot carry plugins; a plugin is trusted code, outside the Luau sandbox; a plugin's own statics are not in snapshots, so keep its state in components.

## The platform layer and Steam

A game talks to its platform through one small interface: achievements, rich presence, cloud saves, the overlay, and button pictures. Scripts ask; the platform answers in the log.

| Script call | What it does |
|---|---|
| `q:achieve("out_of_the_hollow")` | Unlock an achievement `platform.toml` declares. |
| `q:achieve("deep_diver", current, max)` | Show progress on one. |
| `q:presence("status", "In the cave")` | Set a rich presence key (no value clears it). |
| `q:overlay("friends")` | Open the platform's overlay at a page. |

Declare the achievements and the Steam app in `platform.toml` at the project's root. From `examples/hollow/platform.toml`:

```toml
[steam]
app_id = 480

[achievements.out_of_the_hollow]
name = "Out of the hollow"
doc = "Reach the way out at the end of the hall"
steam = "ACH_WIN_ONE_GAME"
```

`steam` is the achievement's Steamworks API name. An achievement the file does not declare is refused before the platform hears of it (a `platform.error` event). `[cloud] saves = false` keeps saves off the cloud. App 480 is Spacewar, Valve's test app every Steam account has; use it until your game has its own.

### Without Steam: the null platform

The daemon, and any build without Steam, runs on the null platform. It keeps what the game asked, so achievements and presence are tested without Steam:

```sh
argus platform status              # achievements unlocked, presence, cloud files, the overlay
argus platform overlay open=true   # as a player's Shift+Tab on Steam
argus platform reset cloud=true    # forget it all, the cloud's files too
```

Its cloud is `.argus/platform/cloud`, so deleting a save from `.argus/saves/` tests a second machine. The overlay is an input, `platform = ["overlay"]` in `input.toml`, so a game binds its pause to it, as Hollow does. `examples/hollow/tests/platform.luau` tests achievements, presence, and the overlay's pause this way.

### On Steam

List the module in `argus.toml`:

```toml
modules = ["steam"]
```

`argus build` then uses the Steam runtime and puts Steam's library beside the executable. The game runs on Steam when Steam is running and logged in; otherwise it runs on the null platform and says "Playing without Steam" on its bottom line. A save goes to the disk and to Steam's cloud, and a load takes the newer copy.

Limits: stats, leaderboards, and multiplayer are not in the interface. The web page always runs on the null platform. `hidden` on an achievement is kept but not used yet.

## Reports from players' builds

A player's build keeps the last thirty to sixty seconds of play. When the player presses F8 (or the project's `[marks] key`) and types a note, or the game crashes, it saves a report in the save folder's `reports/`. With `[reports] url` in `argus.toml`, a desktop build also posts each report to that playtest server, and retries ones that did not go at the next start:

```toml
[reports]
url = "http://100.70.114.55:8765/"
```

`argus mark import path=<report>` or `argus mark pull url=<server>` turns reports into marks, which reproduce like any mark. A report replays in a build of the same code; the mark says which code made it. [Playtesting](playtesting.md) has the whole flow.

## What a player's machine needs

| | Needs |
|---|---|
| Windows | Windows 10 or 11, 64-bit. |
| Linux | x86-64, with ALSA and X11 or Wayland (a desktop has them). The Linux runtime is built on Ubuntu 24.04; [docs/playtest.md](../playtest.md) says its builds need glibc 2.39 or newer. |
| Graphics | DirectX 12 or Vulkan. Without a GPU the game draws in software: Windows has WARP built in; Linux needs Mesa's lavapipe (`mesa-vulkan-drivers`). |
| The web | A browser with WebGPU or WebGL2. |
| Steam | For a Steam build's achievements and cloud saves, the Steam client running and logged in. Without it the game still plays. |

Players need nothing else: no Argus, no Rust, no runtime to install. A game keeps its saves, settings, reports, and mods in its save folder, named after `argus.toml`'s `name`:

| System | Save folder |
|---|---|
| Windows | `%APPDATA%\<name>` |
| Linux | `$XDG_DATA_HOME/<name>`, or `~/.local/share/<name>` |
| macOS | `~/Library/Application Support/<name>` |

The executable takes a few flags, mostly for testing: `--lang <code>` plays in a language pack, `--scene` and `--seed` choose what loads, `--data <dir>` reads packs from elsewhere, `--mods <dir>` and `--no-mods` as above, and `--headless <ticks>` runs with no window and prints the world's digest.

## Where to go next

- [Playtesting](playtesting.md): marks, reports, and the playtest server.
- [Tuning and performance](tuning-and-performance.md): profiling, and when a plugin pays.
- [Content](content.md): languages and strings, which become language packs.
- [Testing](testing.md): the scenarios a release should pass first.
- [docs/install.md](../install.md): installing Argus and its runtimes.
- [Reference](reference.md): `argus help build`, and `platform.toml`, `mods/`, and `plugins/` in `argus docs`.
