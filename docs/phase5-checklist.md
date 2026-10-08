# Phase 5 checklist

Section 18 of the design document gives phase 5 as one sentence and a gate. This file breaks the sentence into items, attaches every constraint from the rest of the document that each item has to meet, and records status, as `phase4-checklist.md` did for phase 4. The order and four decisions were agreed with Kevin on 2026-10-04.

Phase 5 sentence: *ongoing. Rust plugin crates with dylib hot reload; parallel stage scheduling; 2D lighting; optional rigid-body module; skeletal 2D module; platform plugin with the Steam implementation; bounded release recorder with player bug reports; accessibility pass; every §17 target met and gated in CI; Aseprite and Tiled importers; browser editor spike; cross-platform determinism spike.*

Gate: *All four reference games ship as example projects, each with its test suite green in CI on the reference laptop, and an agent given any one of them can add a feature end to end without reading engine source.*

## What the whole phase must meet

- P1 to P5 and the protocol's rules hold for everything added: every new subsystem is reached through the protocol, documented in the one schema (§16), visible to the agent's tools, and costs nothing for a game that does not use it (§12: "costs nothing for a game that doesn't attach the relevant components").
- §6: a run stays a value. Nothing added may make the same scene, seed, input log, and code version reach a different world on the same machine. Parallel systems, plugins, rigid bodies, and skeletons are all checked by digests across runs.
- §15: the release build keeps the protocol compiled out. Plugins, the platform layer, and the recorder are the release build's; the browser editor and CI are not.
- §19 risk, Rust compile times on the plugin path: most logic stays Luau, and the Rust path is a handful of hot systems that change rarely.
- §19 non-goals stay non-goals: no 3D, no networking, no consoles or mobile, no visual scripting, no asset store, and cross-platform bit-determinism is measured (item 11), not promised.

## Already built, and carried on here

From the audit of every Partial and Deferred row in the crates' traces (55 rows: 29 were done in later phases and are marked so here, the rest are below):

- The bounded recorder and player reports exist since phase 2 item 7: a keyframe every 30 seconds and the newest two kept, F8 and a panic writing `-report.argr` and `-crash.argr`, the web page's F8 posting to the playtest server, and `mark.import` and `mark.pull` turning them into marks.
- Accessibility exists in large part: rebinding at runtime (`input.bind`, Hollow's Controls page), text scale and high contrast as UI theme tokens (`UiSettings`), hold and toggle alternatives in the action map, and the semantic frame's UI lines in reading order as a screen-reader feed.
- §17's 23 rows are in `prof.targets`, each with its bound and the system to look at; the release timing tests measured them at the phase 3 and 4 gates.
- Cross-platform runs were measured once: Hollow's world and event digests agree natively, in Chrome, and in wasm under Node, and a Windows and a Linux release reach the same world after 300 ticks.
- The sidecar format already holds Aseprite's and Tiled's structure (per-frame durations, tags as clips with a direction, per-tile properties, one-way tiles).
- The `lit` material has ambient light and one point light; the post stack renders through textures.

## Carried in, found while planning

Rows that no phase scheduled, each given a home:

- **Prefabs are not editable through the protocol** (§3 "instance counts on change", the editor's trace): `Scene::reapply_prefab` has no caller outside tests, and no edit op writes a prefab file. Item 4, with the other data files.
- **Release packs are not yet what §15 describes**: textures one PNG per sprite rather than compressed atlases, audio as authored WAV rather than the platform's codec, defs and scenes as TOML rather than a binary form. Item 1, the release build.
- **Image goldens with a perceptual threshold** (§9) were never built; the harness has state goldens only. Item 0, where CI needs them.
- **The CRT pass** of §13's post stack was never built. Item 3, with the other render passes.
- **Swarm has no pause menu**, and Outpost only its inspector (§12: "every game's pause menu"). Item 2, where the settings live.
- **Effect numbers are not in the tuning panel** (§13: "every knob feel needs is a field the tuning panel can show"): emitters' sidecars can be edited but `tune.fields` does not list them. Item 3, with the lights' own knobs.
- **The broadphase is rebuilt whole each tick** (§12: "rebuilt incrementally"). Item 5, with the scheduler's performance work.
- **Delve's map is tile entities, not an engine tilemap**, so the semantic frame gives it as one group (§8, Delve at L4). Item 4, with the Tiled importer.
- **Web audio runs on the page's main thread**, not an AudioWorklet (the web spike's note). Item 10, with the browser.
- **macOS has no build**: there is no Mac here. Item 11 records it; nothing else can.
- **A mark's spoken note** (§10: "an optional one-line note spoken or typed") needs a microphone and speech to text. Decided at item 1, with a model choice for Kevin.
- **`argus token`** waits for the first remote client (decided in phase 4). Item 10 if the browser editor is reached from another machine.
- Rows that wait on a person, not code: a recorded human feel-tuning session of Hollow's controller (the kinematic tier, §12), and a second person's web mark (phase 2's gate). They go to the gate's hand check.
- Stated limitations that stay: a pixel font in any language (SDF fonts cover other scripts), a desktop report saved for the player to send (item 1 may post it), a crash that corrupts memory is not caught.

## Decided with Kevin (2026-10-04)

- **The order below.** CI with every §17 row gated first, so everything after it lands under CI. Then the near-done items (the release build, accessibility), then features (lighting, importers), then the engine work (parallel stages, plugins, rigid bodies, skeletons), then the platform layer and the two spikes.
- **CI runs on this laptop**, the reference machine, as a self-hosted GitHub Actions runner on the private repository. Kevin installs and registers it; the workflow, the scripts, and the timing gates are the engine's.
- **Skeletal 2D is our own runtime reading Spine's JSON export.** No Spine code in the engine; whoever authors the files uses the Spine editor. The state machine and events are argus-anim's, as §13 says.
- **Steam is tested against Spacewar** (app 480, Valve's public test app) on Kevin's logged-in Steam client, through the `steamworks` crate. Game code and tests use the null implementation when Steam is not there.

## 0. CI on the reference laptop, and every §17 target gated

**Status: done** (2026-10-05) in `tools/ci/ci.sh`, `.github/workflows/ci.yml`, and `docs/ci.md` (the runner Kevin installs), the harness (`expect:image`), `argus-render` (`Image::compare`; every uniform at an offset; `ARGUS_SOFTWARE`, `ARGUS_BACKEND`), the daemon (`obs.frame {golden, threshold}`; `prof.targets`' gates), `argus-cli` (`tests/release.rs`), and the web tools (`check.mjs`'s cold load and verdict, `serve.mjs`'s brotli, `deploy.sh` carrying the server). CI runs six steps on every push to main and every pull request, each to the end, keeping every log, the web check's pictures, an image golden's picture and difference, and a failing scenario's replay: fmt, clippy with no warnings, the workspace's tests and the render tests again on WARP, the four games' suites, the release timing tests (every §17 row but the web page's), and Hollow's page built and checked in headless Chrome. All six pass on this laptop. Every §17 row has its gate but Rust plugin reload (item 7). Traces: `argus-test`'s, `argus-render`'s, the daemon's, `argus-cli`'s, and `argus-web`'s.

- §17: "Each reference game contributes the rows it stresses, and all of them are perf assertions the engine's own test suite runs." 23 rows; three have no measure yet (web cold load, release incremental build, Rust plugin reload, the last item 6's).
- §17: "Numbers on a mid-range laptop core (one thread for sim unless the scheduler finds parallel stages)": the bounds are this laptop's, in a release build.
- The gate: "each with its test suite green in CI on the reference laptop". All four games' suites, the workspace tests, and the release timing tests, on every push to main.
- §2: headless rendering "with a software adapter in CI". The render tests run on the software rasteriser in CI as they can locally.
- §9: image goldens with a perceptual threshold (carried in): a picture expectation in the harness, failing with the difference image.
- §17: "Web build, cold load < 5 s on a 20 Mbit link" and "Release build, incremental < 30 s": measured and gated.
- A failure in CI is the same report the agent gets locally: the scenario, the seed, the replay, the system to look at.

Decisions taken while building it:

- **CI is a script the workflow calls.** `tools/ci/ci.sh [step...]` is what CI runs and what a person or the agent runs by hand, so a CI failure reproduces with one command. The workflow only checks out, cleans everything but the build caches (a cold build is most of an hour), runs the script with Git's bash by its full path (this laptop has WSL's and Cygwin's too), and keeps the outputs for 14 days.
- **The runner runs as Kevin, not as a service.** A service is another Windows account, without his Rust, wasi-sdk, and Chrome, and its reach to the GPU is unreliable; the timing gates are about this laptop as it is used. It starts at log-on (`docs/ci.md`), so a push waits while the laptop is off.
- **The Outpost hand-tuning gate is left out of CI.** It runs 21 minutes (twenty seeds of thirty days, four values); the other release tests take minutes. It stays a phase gate's.
- **Image goldens are drawn in software.** `expect:image` draws with WARP on any machine, so a golden recorded on one machine matches on another. Pixels are compared with pixelmatch's YIQ difference (threshold 0.1), and a picture fails when more than 0.1% of its pixels differ.
- **Found: WARP drew no world.** The first golden was the HUD on black. A uniform bound through DirectX 12's descriptor heap read the one in the next bind group made with the same layout, so the world drew with the UI's view and every post pass with the last pass's numbers. Every uniform is bound at an offset now (a root constant buffer, which bypasses the heap), and WARP draws within 0.1% of the GPU's pixels; a test compares the two, plain and through five post passes. `ARGUS_SOFTWARE=1` sends every renderer to WARP, and CI runs the render tests so a second time.
- **The page reaches 5 s by compression, not a smaller build.** It took 6.5 s and 14.1 MB. A profile without debug info shrank the raw module from 97 to 13 MB but not the page's, since wasm-bindgen already drops it (11.3 MB either way), so the page is built as before. The server sends brotli (quality 9, each file compressed once while it is unchanged): 2.7 MB, 2.6 s on 20 Mbit with 40 ms each way.
- **CI runs by hand until there are public builds (Kevin, 2026-10-07).** A run on every push takes about forty minutes, which slows private work too much. The workflow runs from the Actions tab (`workflow_dispatch`); `tools/ci/ci.sh` is the same checks locally; the per-push trigger returns with public builds.
- **The incremental build is measured as a developer makes one.** A copy of Hollow in a git repository, built, one script changed and committed, built again over the first with `--since`: the code version moves, so cargo relinks the player, and only the new blobs ship. 4.7 s against 30.

## 1. The release build: recorder, reports, and packs

**Status: done** (2026-10-07) in `argus-player` (`reports`, `http` moved from the daemon, `pack::build`'s Vorbis, binary forms, and pages), `argus-assets` (`bc7`, `pages`), `argus-scene` (`binary`; `[reports] url`), `argus-render` (pages, BC7 on the GPU), `argus-audio` (`sound::decode`), `argus-game` (sending), and `argus-cli` (the build's look check, the release-forms line). Hollow's packs: 8 sprites on 2 pages (6 BC7), 10,078 entity files in 201 bundles, 2 def tables in binary; 1,372 files where there were 11,255. Hollow drawn from its packs is pixel for pixel Hollow drawn from its folder; its scene opens 2.7 times faster; its page still loads in 2.5 s. Traces: `argus-player`'s, `argus-scene`'s, `argus-render`'s, `argus-audio`'s, `argus-assets`'s, `argus-game`'s, the daemon's.

- §15: "The recorder stays, in a bounded form: the last thirty seconds of actions and a keyframe, so that a player pressing a report key or hitting a crash produces a replay file the team can open with `sim.seek`. A player bug report becomes a mark." Built in phase 2; what is left: a desktop report reaching the team as the page's does.
- §15: "save migrations stay, because they are how a patched game loads an old save." Built; kept under CI.
- §15: "A release pack is built from the import cache, not the source tree: compressed textures in atlases, audio in the platform's preferred codec, defs and scenes in a binary form of the same schema. Packs are content-addressed so a patch ships only changed chunks. Localization packs are separate." The first three are carried in.
- §10: a mark's note "spoken or typed": the spoken half needs a speech-to-text model, Kevin's choice.

Decisions taken while building it:

- **Typed notes only (Kevin, 2026-10-07).** Of Whisper on this laptop, Whisper on klab, Windows' speech recognition, or none, Kevin chose none. A mark's note is typed, in the window, the editor, or `mark.note`.
- **A desktop report goes where the page's goes.** The project names its playtest server in `argus.toml` (`[reports] url`, Hollow's is klab's tailnet address); the game posts each report there as the page does, and a report that did not go waits in the save location for the next start. Plain HTTP, the client the daemon already had, moved to `argus-player`: no TLS crate for a tailnet address.
- **Sounds keep their names.** A release pack holds a WAV as Ogg Vorbis at the WAV's path, and the decoder goes by the first bytes, so nothing that names a sound changes. Vorbis because a pure-Rust decoder exists (the page needs one) and the reference encoder builds with the GCC here. Building it found that a release build could not load a sound file at all: the WAV reader opened the disk, and a release build's files are in memory.
- **Binary is the loaders' documents, not typed values.** The schemas come from scripts at load, so the bundles hold what the loaders made of the TOML before typing, and the same function types both. Every value keeps its line; a file that did not parse stays TOML. A room's entities are one bundle, which is the unit a patch ships.
- **Found: the first bundles were three times the TOML.** Each value carried its file's whole path; a bundle now keeps its strings once and a place as a line in its entry's file. The packs are 3.3 MB where the TOML was 2.6 (numbers are full width) and 2.5 MB over the wire.
- **Atlases always, BC7 only where nobody can see it.** Hollow's sprites are 3,840 pixels in all, so compression saves nothing measurable for the reference games; pages save textures and draw calls. A sprite ships BC7 only when every pixel of it compressed passes the image goldens' test over black and over white, with exact alpha at 0 and 255; otherwise its page is lossless.
- **Our own BC7 encoder, two modes, checked on the GPU.** Modes 5 and 6 need none of BC7's partition tables, which nothing here holds and which a wrong entry would break only on a GPU. A test draws the blocks with the GPU's decoder and with ours and requires every pixel the same; ours is also what a device without BC7 uses.
- **Found: CI's timing tests competed with each other.** A test binary runs its tests on parallel threads, so `argus-scene`'s cold open (3 tests in one file) and `argus-index`'s resync (2) read a fifth slow and missed their bounds now and then, here and on the runner. The perf step runs them one at a time (`--test-threads=1`), in the same seven and a half minutes.
- **Found: CI's web check could wait forever.** Its first run on the runner after this item waited two hours on a Chrome that did not answer, with nothing in its report. `tools/web/check.mjs` now uses free ports (a check never meets another, or a playtest server on 8765), gives every DevTools call a minute and the whole check five, fails at once when Chrome closes or cannot start, says which stage it stopped at, and keeps Chrome's output in `chrome.log` beside the screenshots; CI gives it ten minutes at most.
- **Found: the runner's cache guard emptied CI's logs.** Past its size the guard ran `cargo clean`, which took the log folder the script had just made, and every step failed to write; and at 60 GB it would have emptied a working cache (debug and release, about 85 GB) nearly every run. The logs are made after the cleaning, the line is 100 GB, and the size comes from robocopy's listing (a second) where du took minutes.
- **Found after it was done (2026-10-07): the hero flashed the exit when stopping.** Kevin saw it in the release build. Sprites on one page draw as one run, keyed by the page's first sprite, and an animator's crossfade looked its outgoing frame up in that key's frames, the exit's, not the hero's; a folder has a texture per sprite, so it never showed there, and the build's look check draws one frame, mid-idle. The crossfade takes its frame from the sprite's own atlas, and `release_pages` now compares every tick of the hero running and stopping.
- **`argus build` checks the look too.** It drew nothing before; it now draws a frame of the project and of the build in software and refuses a build whose pixels differ, leaving the two pictures and a difference image beside `build.json`.

## 2. Accessibility pass

**Status: done** (2026-10-07) in `argus-input` (rebinds in the journal and the snapshot, replay files from version 4; `InputToggle`), `argus-ui` (`settings`; a hidden node's words), `argus-player` (`settings`; a caught key's tick), `argus-render` (`see_as`, `access`, the window's AccessKit adapter), `argus-game` (the screen reader's tree and presses; `settings.toml`), `argus-web` and `tools/web` (settings in the browser; the page's reader), the daemon (`obs.frame {see_as}`, `render.stream {see_as}`, a caught key's tick), the editor (the viewport's See as), and the games: Delve, Swarm, and Outpost have a pause menu with Controls and Settings, as Hollow had. Kevin's hand check: Narrator (Windows, Ctrl and Enter) or NVDA on the desktop Hollow, and a screen reader on the page. Traces: `argus-input`'s, `argus-ui`'s, `argus-player`'s, `argus-render`'s, `argus-post`'s, `argus-game`'s, `argus-web`'s, the daemon's, the editor's.

- §12: "Rebinding comes with the action map; text scaling and high-contrast palettes are UI theme tokens; input-hold and toggle alternatives are action-map options; and the semantic frame (§8) is, without modification, a screen-reader feed: the same call the agent uses to know what's on screen can drive a player's narrator." Each in every game: rebinding and text and contrast in all four menus, hold or toggle as a settings button, and the frame read by the player's own screen reader on the desktop and the page.
- §12: "A colour-blind simulation post pass (§13) lets the agent and the human check readability without guessing." Built: protanopia, deuteranopia, tritanopia, and achromatopsia, for the agent's `obs.frame` and the editor's viewport.
- Settings that last between launches (the UI's and the game's traces): a store outside the world, since a replay must not change with them. Built, and a report replays with them.
- §12: "every game's pause menu" (carried in): Swarm and Outpost get one, with the settings. Built, and Delve's got its pages too.

Decisions taken while building it:

- **The narrator is the player's own screen reader (Kevin, 2026-10-07).** Of a voice of the game's own (a speech model to ship) or the screen reader the player already uses, Kevin chose the screen reader. On the desktop the game hands the semantic frame to the operating system as an AccessKit tree (Narrator, NVDA, JAWS), built only while a screen reader is reading; on the page the same frame is hidden buttons and two live regions a browser's screen reader reads. A press from either is the pointer on the button and a click, so it is recorded input and replays.
- **Settings live outside the world and go in before the first tick.** `settings.toml` in the save location; the page keeps the same text in the browser. They are put in before the recorder starts, so its first keyframe holds them and a report replays with the player's settings in anyone's player. Only what differs from the game is kept, so an update that changes a default reaches a player who never changed it. The daemon reads none: a project's tests run as the project is.
- **Found: a rebind was not in the run.** Neither the journal nor a keyframe held the action map, so a report replayed with the viewer's keys, and the controls page, which is world state, drifted from the run. The key a controls page catches and `input.bind` are journaled now, and a snapshot holds the bindings, the toggles, and what waits for a key.
- **Found: a hidden page's words came from the layout's cache.** A replay begun at a keyframe after a rebind showed "Jump: " where the run had "Jump: x", since the hidden controls page kept the words the cache last measured. A hidden node reads the world's words.
- **Found: a turn-based game said "press a key" until the next key** (the agent, building Delve's page). A discrete game ticks only on a command, and the caught key was none. It is now, in the player and in a daemon in play; the menu has the game frozen, so no turn of play passes.
- **The colour-blind view is a way of looking, not a pass a game ships.** It is drawn after the UI, so what is checked is everything the player sees, and it is a renderer setting the agent and the editor choose per frame, not a component in a scene. Machado, Oliveira, and Fernandes (2009) at full severity, in linear RGB; achromatopsia as luminance.
- **Hold or toggle is a button a settings page can have** (`InputToggle {action}`), switching the action in the tick so a replay switches it too. No reference game has one yet; a test does.
- **Each game's menu in its own idiom.** Delve's is entity files, as its menu was. Swarm's is one prefab its three scenes use, and its Restart resets the run in place (a death had no way out before). Outpost's is built from Luau on load, so an old save gets one. Outpost's Escape belonged to the inspector, so the two take turns: the inspector backs out first, the menu opens on a later Escape, and inside the menu Escape backs out a page at a time. Outpost's menu is keyboard only, since Start and Select are the colony's pause and the inspector.
- **A closed menu costs a little, not nothing.** The UI systems still walk hidden nodes: Swarm's stress scene spends about 0.1 ms a tick on its closed menu in a debug build, against a tick of about 60, and its test fails at 0.5 ms. Skipping a hidden subtree would make it free; that waits for a game that needs it.

## 3. 2D lighting

**Status: done** (2026-10-07) in a new crate, `argus-light` (the `Light`, `Lighting`, `Occluder`, and `Emissive` components; tile and box edges; shadows), `argus-render` (`light.rs`: the light map, stencil shadows, the composite, what glows, the normal pass; the semantic frame's `lighting`; `PostCrt`'s pass), `argus-post` (`PostCrt`), `argus-material` (`emissive = true`), `argus-assets` (`<name>.normal.png`; normal-mapped sprites off the pages), `argus-player`, the daemon, the game, and the page (the tile edges each frame; `obs.frame`'s counts), `argus-cli` (lighting as lines), the tuning panel and the daemon's tuning (emitters' numbers; colours and other fields that are not numbers), and the games. Hollow's caverns are dark, darker the deeper the room, and the hero carries a lantern whose light the rock casts shadows from; the hermit's fire, the gem, the exit, and the deep's crystals glow. Outpost's light follows its day, white at noon, orange at dusk, blue at midnight, and the stove and a campfire light the colony at night. Sixteen shadowed lights cost 0.19 ms at 1080p. Traces: `argus-light`'s, `argus-render`'s, `argus-post`'s, `argus-material`'s, `argus-assets`'s, `argus-particles`'s, `argus-cli`'s, the daemon's, the editor's.

- §12: "2D lighting as a render pass: point and cone lights, occluders derived from tiles and sprites, normal-mapped sprites optional; off by default and zero-cost when off. Hollow's caverns; Outpost's day-night cycle."
- §13: the `lit` material is one of the library's; several lights and shadows were left to this phase (the material's trace).
- §17: "Hollow post stack, 5 passes at 1080p < 1.5 ms GPU": lighting must not break it.
- §13: the CRT pass, and effect numbers in the tuning panel (both carried in). A light's colour, radius, and intensity are tune fields.
- §8: lights are visible to the agent: the semantic frame and `world.query` see them, and a screenshot shows them.

Decisions taken while building it:

- **Lights are components; lighting is a camera's.** A light is a `Light` on any entity, so it moves, is prefabbed, tuned, and saved as anything is; `Lighting` on the drawing camera turns the pass on, as a post pass is a camera's. With no `Lighting` the renderer makes nothing and draws as before, and no system runs in any tick: a game that does not light pays nothing.
- **A cone is a point light with an angle.** `angle` under 180 degrees makes it a cone, so a lamp tuned from all round to a beam is one slider, not a kind changed.
- **Shadows are the stencil's, from the far edges.** Each shadowed light draws its shadow into the stencil with its own number and then itself where the stencil does not hold it: no clear between lights, 255 a pass. A shadow is swept from the edges that face away from the light, so the rock the lantern shines on is lit and the shadow begins behind it; casting from the near edges would leave every wall black.
- **Tiles occlude by their collision.** The solid and slope cells of the collision layers the physics already reads cast shadows (one-way planks do not), as the long edges where solid meets open, so a cave's walls are a few dozen edges, not one per tile. A game says nothing to get them; `Lighting.tiles = false` turns them off.
- **The picture is lit before the post passes and the UI after.** A vignette or a flash sees the lit picture, and a HUD stays readable in the dark.
- **Glows are drawn after the light.** `Emissive` on a sprite, or `emissive = true` on a material for every sprite and spark drawn with it: an additive glow multiplied by the dark would vanish.
- **Normal maps are a file beside the image.** `<name>.normal.png`, the image's size; nothing to declare. A normal-mapped sprite stays off the release pack's shared pages so its colours and normals are one layout. The normal pass runs only in a frame that draws one.
- **Half floats where the device has them.** The light map is half floats where the GPU draws and blends into them; else sRGB bytes holding half the light, which still reach twice white and keep the dark steps fine. A test draws both and compares them.
- **Found: the test fixture's own `Light`.** The daemon's test project declared a script component named `Light`, which the engine's now is; every test on it stopped at a script error. An engine component's name is the engine's: the fixture uses the engine's `Light`, whose `intensity` is the tune field those tests set.
- **Found: the tuning panel could tune only numbers.** A light's colour, the ambient colour, and a camera's offsets showed as a number box at 0 that the daemon refused. A colour is now its swatch and its hex, sent when Enter is pressed (a half-typed colour is no colour); two numbers are two to drag; a switch is a box to tick.
- **Effect numbers (the tuning agent's).** An emitter's rate, burst, lifetime, speed, spread, drag, gravity, and curve values are tune rows (`tune.fields {asset}`); a slider plays from the next tick, particles already out included, and the session's end writes the sidecar with its comments, in an order where every step still parses. Colours and a curve's times are left out: a slider is a number, and a time dragged past its neighbour breaks the curve. A tune of an effect is a change between ticks, as a def row's is: the pools feed the world (an emitter's live count, `particles.done`), so it takes a keyframe no replay crosses.
- **Each game's light in its own idiom** (the game agent's). Hollow glides `Lighting.level` from room to room (the mouth 0.6 down to the deep's bottom row 0.1) under the room fade; its lantern is a child of the hero, inside the hero's collider (a light inside rock is shadowed on every side); its 394 deep crystals' lights came from `tools/deep.py`, run again. Outpost's ambient follows the HUD's own hour through eight keys, smoothed each tick, its colour strings stepped by 4 so the day interns a few hundred and not a new one each tick; its lamps fade in below a level and are written only while they change; the stress scene's colonists carry none (0.02 ms a tick). High contrast keeps both games bright enough to read.
- **Left as limitations.** Shadows are hard; a slope occludes as a whole tile; a sprite's occluder is a box; a light inside a solid tile is shadowed on every side; a child entity needs its `GlobalTransform` to be placed, lights too.
- **Found after it was done (2026-10-08): Outpost's HUD smudged "colonists" at night.** Kevin saw it with lighting on and off. Nothing was laid out wrong: every glyph had its own place, and the line was its words' width. It was the text's drawing, at 640 x 360 (the default `obs.frame`, and any window or viewport under the canvas's size), where the 15-pixel text is ten pixels and the t's stem is under a pixel wide. Two pixels either side of the stem both read the field just under the edge, and the edge's blend, `fwidth` of the field, saw no change between them (it rose and fell again), so it closed to a hard edge and the stem's middle vanished; " (night)" moved the t to where it fell that way. `sdf_text` now takes the blend from the field's slope at the pixel times the texels a pixel spans, which never closes; a test draws ls under a pixel wide a tenth of a pixel apart on the GPU and WARP, and every one keeps its stem.

## 4. Aseprite and Tiled importers, and prefab edits

**Status: done** (2026-10-08) in `argus-assets` (`aseprite.rs`: `.aseprite` and `.ase` read natively, layers composited, tags as clips, slices as pivots and hitboxes; `tiled.rs`: `.tsx`/`.tsj` tilesets as sprites, `.tmx`/`.tmj` maps as assets, embedded tilesets as sprites of their own), `argus-tiles` (`tiled.rs`: a Tiled map opened as the engine's own, in memory; runtime maps: `runtime = true`, cells the run's), `argus-luau` (`q:tiles`), `argus-player` (keyframes, replay files from version 5, saves from version 3 carry runtime maps' cells; `ScriptTiles`), `argus-scene` (`PrefabLibrary::set_field`; a prefab's change written only where it changed), the daemon (`edit.prefab`; Tiled and runtime maps refused to `edit.tiles`; their structure in `asset.get` and `scene.tiles.info`), the editor ("Change the prefab instead"), `argus-cli` (the `edit` tool's `prefab`), and Delve, whose floors are a runtime map now: the same floors from the same seeds (580 dumps compared), generated three times faster (0.34 ms against 1.06), and the semantic frame reads the map as rows. Traces: `argus-assets`'s, `argus-tiles`'s, `argus-anim`'s, `argus-luau`'s, `argus-player`'s, `argus-scene`'s, the daemon's, the editor's, `argus-cli`'s.

- §14: "Aseprite and Tiled files import natively with their internal structure exposed through the sidecar." The importer "watches the tree, produces a cache in `.argus/`, and reports every import as an event with warnings".
- §13: "Aseprite tags import directly as clips."
- §12: tilemaps "with auto-tiling rules declared in TOML, collision derived from tile properties, and chunked storage"; a Tiled map becomes the engine's map and its layers.
- §3: prefabs edited as transactions, with how many instances a change touched (carried in).
- §8: Delve's map as an engine tilemap, so the semantic frame reads it (carried in).

Decisions taken while building it:

- **Each format read natively, into what the engine already had.** An Aseprite file is an ordinary sprite asset once imported (frames, clips, properties), so the animator, the pages, and the release pack need nothing new; a Tiled map is an ordinary map once opened, so drawing, collision, navigation, shadows, and the semantic frame need nothing new. The structure the engine does not take (an Aseprite file's layers, tags, and slices; a Tiled map's layers, tilesets, and objects) is in `asset.get` and `scene.tiles.info`, so the agent writes a sidecar against it without opening either tool.
- **Aseprite (the importer agent's).** Every frame keeps the canvas size (a trimmed frame would need a pivot outside its rect); frames with the same pixels share a cell; the sheet is laid out for the smallest power-of-two texture. Tags are clips playing the file's own frame durations; a repeat count is the frames written out. A slice named `pivot` (or the first with a pivot) is the frames' pivot; every other slice is a frame property `slice.<name> = "x y w h"`. The sidecar picks layers (`layers = [...]`) and overrides what it writes. Checked read-only against 13 of Kevin's own Aseprite files: all read, and 6 of the 7 with an export beside them match it pixel for pixel (the seventh was saved after its export).
- **Tiled (the importer agent's).** A layer drawing from two tilesets becomes a layer per tileset; a group's layers become `group/layer`; a hidden layer collides but is not drawn (Tiled's hidden collision layer); `collision = true` and `z` are layer properties. Embedded tilesets are sprites named `<map>/<tileset>`. Objects are data, not entities. A Tiled map is Tiled's: an edit is refused with "edit it in Tiled", and a save in Tiled is read again. What does not carry over (flips, opacity, image layers, terrain sets, collision shapes) is a warning, never silent.
- **A prefab change is a transaction, and a running game keeps its state.** `edit.prefab` writes the prefab's file with its comments kept and resolves every instance again; the feed row names each instance touched. Found: the scene's reapply rewrote every field of every instance, so a prefab changed while a game ran put each instance back to its file's state (a moved bat home, a wounded one healed). Only what the change changed is written now. The editor's "Change the prefab instead" sends a field's change to the prefab rather than the entity's own file.
- **Runtime maps for procedural floors (the carried-in Delve row).** A map's layers, tileset, and collision are authored in its index; with `runtime = true` its cells are the run's: empty at the start, written by scripts (`q:tiles`, declared as a write of `Tilemap`), carried by keyframes, replay files, and saves, and never written to disk. Delve keeps its generator, the same random draws in the same order, and paints the floor in two calls where it spawned a few hundred wall entities.
- **Found: a run's digest missed its map** (the Delve agent). The world's digest, and so a mark's, a replay's, and a report's `same`, read only the ECS world, so two different floors digested alike. A run's digest is the world's and the runtime maps' cells, and is the world's alone when no runtime map holds a tile, so every digest recorded before is unchanged; a replay file ends with the cells, and a seek's verify compares them.
- **Found: a refused tile edit changed a cell.** An edit of a runtime map refused only by its save had already written the cell in memory; it is refused before it writes.
- **Found: two monsters of equal initiative can act in another order on Delve's later floors.** Delve breaks initiative ties by entity id, and ids are reused differently without the wall entities; the floors themselves are the same. Delve's rule is left as it is.
- **Left as limitations.** Scripts read tile properties but not a sprite frame's (an Aseprite hitbox is in `asset.get` and Rust, not Luau yet); a Tiled map cannot change at run time; a same-named `.wav` and `.ogg` (or `.ase` and `.aseprite`) keep one quietly, as before.

## 5. Parallel stage scheduling

**Status: done** (2026-10-08) in `argus-ecs` (`World` shared between threads: atomic column borrow flags, a string table behind a lock and sealed during a batch; the broadphase kept up to date instead of rebuilt), `argus-sim` (`par.rs`: parallel systems and their `ParCtx`; batches planned from the declarations and run on threads when it saves time; `SchedulerConfig::threads`; a capture's thread rows), and the daemon (`sim.threads`; `script.systems`' `parallel` and `batch`). Four heavy systems in one batch: 6.7 ms a tick on one thread, 2.0 ms on four, the same world. The world, every event, and the string table are the same tick by tick on 1, 3, and 4 threads. The broadphase's pass at 100k moving entities: 0.43 ms, where the rebuild was 2.2 ms. Traces: `argus-ecs`'s, `argus-sim`'s, `argus-physics`'s, the daemon's.

- §3: "Each system declares the components it reads and writes. The scheduler uses this for parallelism."
- §17: "one thread for sim unless the scheduler finds parallel stages."
- §6: determinism holds: the world at tick N is the same with one thread or many.
- The ECS's trace: column borrow flags are `Cell`s, so `World` is not `Sync`; parallel scheduling replaces them with static per-stage checks from `Access`. The Luau VM is not `Send`.
- §12: the broadphase rebuilt incrementally (carried in).

Decisions taken while building it:

- **A parallel system is its own kind, registered as one.** `Scheduler::add_parallel_system` takes a `Send` body that sees the world through a `ParCtx`: the components it declared, its RNG stream, and nothing it could race on. Script systems share one Luau VM, and the engine's own systems hold the daemon's stores in `Rc`s, so neither is `Send`; in the four games every system stays serial and the sim runs on one thread, which is §17's "one thread for sim unless the scheduler finds parallel stages". Rust plugin crates (item 6) are the first that can use batches.
- **The borrow flags stay, made atomic.** The ECS trace planned to replace the column flags with static checks from `Access`. The static checks are what plan a batch, but the flags also catch a system that fetches one column twice, which no declaration shows, so they stay, as atomics, and `World` is `Send` and `Sync` soundly. Field access costs about 2% more in the Luau per-entity benchmark (42.2 to 43.8 ms against 41.8).
- **A batch changes only its own columns while it runs.** Its systems' events, spawns, despawns, inserts, and removes are applied after the last of them, system by system in run order, and the string table is sealed (a new string's id would depend on which thread asked first; a spawn may carry one, since spawns are applied after). So a batch is the same on one thread or many, and whether it uses threads is only about time: it does when its systems' last runs promise to save 0.2 ms beyond its longest, so a light batch never pays for starting threads.
- **What a parallel system spawns does not keep it apart.** Spawns land after the batch, so `SystemDesc::spawns(c)` declares them separately from writes: a spawner and a system writing the same component's column can share a batch, and `who_writes` lists both.
- **A batch is consecutive systems in the stage's order.** The scheduler does not move systems to make bigger batches; the order is what `after` and registration declared, and it stays the order events and changes are applied in.
- **The thread count is not part of the run.** `sim.threads` makes no keyframe and a replay does not record it, since the world after a tick is the same for any number.
- **The broadphase is kept, not rebuilt.** Each pass writes positions into per-entity slots and touches the cells only for entities that changed cell, arrived, or left (refilling every cell in one sweep when more than an eighth did). Cells list entities by index, so a hash kept up to date answers every query in the same order as one built from nothing, which is what a restored snapshot builds. Swarm's headless rate rose from 40,400 to 49,100 ticks a second, Delve's turn p95 from 0.031 to 0.021 ms, and the physics test's 20,000 circles from 3.2 to 2.1 ms a tick.

## 6. Rust plugin crates with dylib hot reload

**Status: done** (2026-10-08) in two new crates, `argus-plugin` (the SDK a plugin is written with: the C boundary both sides share, `component!`, `System`, `plugin!`, `Ctx`) and `argus-plugins` (the engine's side: cargo builds with their errors at file and line, loading, checking against the game, installing, reloading, and every call a plugin makes), and in `argus-player` (`assemble` loads them; `Player::open_from`), the daemon (`script.reload` swaps them, `script.errors` and `script.systems` show them; two §17 gates), `argus-sim` (`split`, `source_of`, `tolerate_missing`), `argus-ecs` (`with_row_mut`, `DynColumn::as_ptr`), `argus-cli` (`argus build` ships them), `argus-game` (loads them from beside itself), and Swarm, whose chase is its plugin `swarm-hot` now. The Rust chase reaches the same world as the Luau one, bit for bit, every tick. Swarm's stress scene (50,000 enemies, release) ticked at 40.6 ms p95, 37 of them the Luau chase; it ticks at 2.5 ms now, the chase 0.15 ms, so the game itself meets §17's 6 ms. A one-line change to the plugin builds in 0.34 to 0.40 s and is running 38 to 43 ms after that (§17: under 2 s). Traces: `argus-plugin`'s and `argus-plugins`' (new), the daemon's, `argus-player`'s, `argus-sim`'s, `argus-ecs`'s, `argus-cli`'s.

- §7: hot-path systems "are Rust, either in the engine or in a project plugin crate, and are compiled to a dynamic library the daemon loads." "For Rust plugins, the daemon snapshots, unloads the old dylib, loads the new one, restores, and continues. The agent sees a compile error as a structured `script.errors` response with file:line and the compiler's message, never as a crash."
- §2: "Rust plugin systems are loaded as dynamic libraries; a panic in one is caught at the system boundary, the system is disabled, and the fault is reported through the protocol as an event."
- §7: the two rules hold for plugins: a system touches only components it declared, and holds no references across frames.
- §17: "Rust plugin reload < 2 s after compile."
- §19: "The scheduler's write-set declarations make it possible to promote a Luau system to Rust without changing anything around it."
- `argus.toml`'s `plugins` list and `plugins/` folder exist since phase 0. A reference game ships a plugin crate.

Decisions taken while building it:

- **The boundary is C, and nothing else crosses it.** A plugin is built by its own cargo, separately from the engine, so a `World`, a `Vec`, or a `String` of either side never crosses: the library exports one function returning its declaration (its systems and the component layouts it reads), and every call back into the world goes through a table of C functions the engine passes in. A plugin's struct for a component is `#[repr(C)]` and checked against the game's layout, field for field, before any of its systems runs; each fetch carries the layout's fingerprint, so a struct that drifts is refused, not misread.
- **The engine ships its SDK into the project.** A plugin depends on `argus-plugin` at `../../.argus/plugins/sdk`, which the engine writes (the SDK it was built with) before every build. So a plugin is always built against the engine that loads it, and a project copied anywhere (a test's temp copy, a player's mod folder) still builds. The boundary is versioned all the same.
- **A copy of the library is what loads.** Windows locks a loaded file, so cargo's output is copied to `.argus/plugins/loaded/` and the copy is loaded; the next build writes over cargo's while the game runs. The copy goes when its library does. A release build loads its library where it is.
- **The new library is checked before the old one goes.** §7's order is snapshot, unload, load, restore. Here the new library is loaded and checked first, then the world and the sim state are snapshotted, every system's body swapped (each keeping its place and its stream), the old library let go, and the snapshot restored. A library that does not load or check never costs the running one. The world's digest is the same before and after.
- **A plugin that does not build keeps its last good library.** Cargo leaves the last good build in place, and the engine loads it, at a session's open as at a reload, while `script.errors` lists the compiler's errors. A plugin that never built is missing, and a system ordered after one of its systems runs unordered there, a warning in `script.errors` rather than a session that does not open (found with Swarm: `sim.fire` runs after `sim.chase`, and a broken chase took the whole session down).
- **Changes land when the run ends.** A plugin's events, spawns, and despawns are kept and applied when its run returns, through the system's own context. So the engine's side never changes the world through two paths at once, and a plugin system behaves the same alone and in a parallel batch. A spawn does not return its entity.
- **Swarm's chase is the promotion §19 describes.** `sim.chase` moved out of `systems/chase.luau` with its declaration unchanged and its arithmetic matched step for step (vectors in f32, the scale in doubles, as Luau's numbers are). The game runs the same systems in the same order and reaches the same world, to the bit, every tick; all seven of its scenarios pass. The §17 Swarm tick row is now gated on the game's own stress scene rather than a Rust stand-in.
- **Built optimised, always.** A plugin exists for a hot path, so the daemon and `argus build` both build it with `--release`; Swarm's plugin keeps incremental compilation on in its release profile. A cold build of the SDK and Swarm's plugin takes under a second here, a no-op check about 30 ms.
- **Found: the daemon's project root is a `\\?\` path.** Canonicalising gives Windows' verbatim form, which cargo does not take everywhere; every plugin build in the daemon failed until the prefix was dropped for cargo.
- **Left as limitations.** The page cannot load a library, so a game with plugins has no web build (item 10's question); a plugin's own statics are not in a snapshot; forks keep the library they were made with; mods cannot carry plugins; a plugin is trusted code.

## 7. Optional rigid-body module

**Status: done** (2026-10-08) in a new crate, `argus-rigid` (`RigidBody`, `Joint`, and `RigidWorld` on Rapier 0.36; `physics.rigid` between the kinematic move and the overlap pass; contacts and broken joints as events; Rapier's own state in every keyframe), and in `argus-scene` (`argus.toml`'s `modules`), `argus-sim` (`order_after`), `argus-player` (the `rigid` feature, keyframes and replay files from version 6), the daemon, `argus-cli` (`argus build` passes the game's modules as features), `argus-game` and `argus-web` (the feature), and a new example, `examples/toybox`: a stack, a pendulum, a plank bridge pinned at both ends, and balls a Luau system drops, with three scenarios. A seek, a replay file, and a run from the same start all reach the same world. 500 boxes cost about 1 ms a tick. Traces: `argus-rigid`'s (new), `argus-physics`', `argus-player`'s, `argus-scene`'s, `argus-sim`'s, `argus-cli`'s, the daemon's.

- §12: "Dynamic: real rigid bodies with joints, from an optional Rapier module for the games that need a physics toy, not a default." Three tiers "chosen per entity, sharing one broadphase".
- §6: the same run reaches the same world.
- §12: costs nothing for a game that does not attach the components; a release without it does not carry it.

Decisions taken while building it:

- **Rapier, as the design says (Kevin, 2026-10-08).** Asked before downloading anything, Kevin chose Rapier over a solver of our own. It came as 29 crates and 6.6 MB of source (rapier2d 0.36, parry2d, nalgebra, simba, glam and glamx, spade, wide, and their dependencies), more than the 3 to 4 MB estimated in the question. Looking up its version first, `cargo info` downloaded rapier2d itself before Kevin had answered; he was told.
- **Optional is a line in `argus.toml`.** A game names the module (`modules = ["rigid"]`); the daemon installs it only then and `argus build` builds the executable and the page with the `rigid` feature only then. So the daemon and a release agree, a release of a game without rigid bodies has no Rapier in it, and a game that uses the module without naming it is refused with the reason. The daemon and the CLI always have it compiled in, since they run any game.
- **Rapier's own state is the run's.** Its contact caches and solver warm-start carry from tick to tick outside the ECS, and a run replayed without them parts from the original within ticks (a test shows it). Every keyframe clones the store (seek, fork, the recorder), and a replay file or a player's report carries it as bytes from version 6 of the format. Saves keep only the components: a loaded game goes on from the same positions and velocities, its contacts found again.
- **The same on every platform, as far as Rapier can make it.** Its `enhanced-determinism` feature (libm for every transcendental) is on, so a toybox run in the browser should reach the native world; item 11 measures it.
- **The three tiers in one world.** The kinematic tier's bodies are kinematic bodies in Rapier, moved to where `physics.move` put them, so platforms and characters push rigid bodies; every Tilemap's solid tiles are fixed colliders, merged into as few boxes as rows allow. The world's spatial hash indexes rigid bodies like any entity, and the overlap pass, ordered after this tier, reports them. A one-way tile is solid to a rigid body, and the kinematic tier does not see rigid bodies (a character pushes a crate but is not stopped by one); both are stated limitations.
- **Gameplay speaks through components.** A script throws a body by writing its velocity and teleports it by writing its Transform: the tier tells those from its own writes by what it wrote last tick. Joints are entities with a `Joint` component, so they are authored in TOML, edited live, and listed like anything.
- **Found: a joint's force was a quarter of the truth.** Rapier keeps the impulse of a tick's last substep, and it takes four; a joint's breaking force is worked out over the substep's time.
- **A new example, not a reference game.** The design gives no reference game a physics toy, so `examples/toybox` exercises the module end to end (its scenarios run in CI's games step with the four games), and none of the four games changed.

## 8. Skeletal 2D module

**Status: to do.**

- §13: "Skeletal 2D via a Spine-compatible runtime is a phase-5 module; the state machine and events are the same either way." Decided: our own runtime reading Spine's JSON export.
- §13: clip rates and events by frame stay tune fields and ordinary events.

## 9. Platform plugin with the Steam implementation

**Status: to do.**

- §15: "A platform plugin interface (achievements, cloud saves, rich presence, input glyphs, overlay) with a Steam implementation first and a null implementation for everything else, so game code calls one API and tests run without Steam present." Decided: tested against Spacewar (app 480).

## 10. Browser editor spike

**Status: to do.**

- §11 open: a browser editor served by the daemon. §19's open question: "Review requests over a phone during a couch playtest: worth a small web client early, or does that wait for the browser editor?"
- A spike answers the question: what the editor needs that only a native window has (the shared-memory frames, the local socket), and what a browser would use instead.
- Web audio in an AudioWorklet (carried in).

## 11. Cross-platform determinism spike

**Status: to do.**

- §6: "Floating point is f32 with the usual determinism caveats: reproducible on the same platform and build, not guaranteed across compilers or CPUs. Cross-platform bit-determinism (fixed-point math, or a constrained float subset) is a known path and an explicit non-goal for v1."
- A spike measures it across this laptop, klab's Linux, and the browser, for all four games, and says what a game would need for lockstep (§15 networking, preserved). macOS is recorded as not measured.

## The gate

**Status: to do.**

- All four reference games ship as example projects, each with its test suite green in CI on the reference laptop.
- An agent given any one of them can add a feature end to end without reading engine source: the project, the `argus` tools and their docs (§16), and `ARGUS.md` are enough.
- Every earlier gate's tests still pass.
