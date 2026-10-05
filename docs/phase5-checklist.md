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

**Status: to do.**

- §17: "Each reference game contributes the rows it stresses, and all of them are perf assertions the engine's own test suite runs." 23 rows; three have no measure yet (web cold load, release incremental build, Rust plugin reload, the last item 6's).
- §17: "Numbers on a mid-range laptop core (one thread for sim unless the scheduler finds parallel stages)": the bounds are this laptop's, in a release build.
- The gate: "each with its test suite green in CI on the reference laptop". All four games' suites, the workspace tests, and the release timing tests, on every push to main.
- §2: headless rendering "with a software adapter in CI". The render tests run on the software rasteriser in CI as they can locally.
- §9: image goldens with a perceptual threshold (carried in): a picture expectation in the harness, failing with the difference image.
- §17: "Web build, cold load < 5 s on a 20 Mbit link" and "Release build, incremental < 30 s": measured and gated.
- A failure in CI is the same report the agent gets locally: the scenario, the seed, the replay, the system to look at.

## 1. The release build: recorder, reports, and packs

**Status: to do.**

- §15: "The recorder stays, in a bounded form: the last thirty seconds of actions and a keyframe, so that a player pressing a report key or hitting a crash produces a replay file the team can open with `sim.seek`. A player bug report becomes a mark." Built in phase 2; what is left: a desktop report reaching the team as the page's does.
- §15: "save migrations stay, because they are how a patched game loads an old save." Built; kept under CI.
- §15: "A release pack is built from the import cache, not the source tree: compressed textures in atlases, audio in the platform's preferred codec, defs and scenes in a binary form of the same schema. Packs are content-addressed so a patch ships only changed chunks. Localization packs are separate." The first three are carried in.
- §10: a mark's note "spoken or typed": the spoken half needs a speech-to-text model, Kevin's choice.

## 2. Accessibility pass

**Status: to do.**

- §12: "Rebinding comes with the action map; text scaling and high-contrast palettes are UI theme tokens; input-hold and toggle alternatives are action-map options; and the semantic frame (§8) is, without modification, a screen-reader feed: the same call the agent uses to know what's on screen can drive a player's narrator." Built; this item checks each in every game.
- §12: "A colour-blind simulation post pass (§13) lets the agent and the human check readability without guessing." Not built.
- Settings that last between launches (the UI's and the game's traces): a store outside the world, since a replay must not change with them.
- §12: "every game's pause menu" (carried in): Swarm and Outpost get one, with the settings.

## 3. 2D lighting

**Status: to do.**

- §12: "2D lighting as a render pass: point and cone lights, occluders derived from tiles and sprites, normal-mapped sprites optional; off by default and zero-cost when off. Hollow's caverns; Outpost's day-night cycle."
- §13: the `lit` material is one of the library's; several lights and shadows were left to this phase (the material's trace).
- §17: "Hollow post stack, 5 passes at 1080p < 1.5 ms GPU": lighting must not break it.
- §13: the CRT pass, and effect numbers in the tuning panel (both carried in). A light's colour, radius, and intensity are tune fields.
- §8: lights are visible to the agent: the semantic frame and `world.query` see them, and a screenshot shows them.

## 4. Aseprite and Tiled importers, and prefab edits

**Status: to do.**

- §14: "Aseprite and Tiled files import natively with their internal structure exposed through the sidecar." The importer "watches the tree, produces a cache in `.argus/`, and reports every import as an event with warnings".
- §13: "Aseprite tags import directly as clips."
- §12: tilemaps "with auto-tiling rules declared in TOML, collision derived from tile properties, and chunked storage"; a Tiled map becomes the engine's map and its layers.
- §3: prefabs edited as transactions, with how many instances a change touched (carried in).
- §8: Delve's map as an engine tilemap, so the semantic frame reads it (carried in).

## 5. Parallel stage scheduling

**Status: to do.**

- §3: "Each system declares the components it reads and writes. The scheduler uses this for parallelism."
- §17: "one thread for sim unless the scheduler finds parallel stages."
- §6: determinism holds: the world at tick N is the same with one thread or many.
- The ECS's trace: column borrow flags are `Cell`s, so `World` is not `Sync`; parallel scheduling replaces them with static per-stage checks from `Access`. The Luau VM is not `Send`.
- §12: the broadphase rebuilt incrementally (carried in).

## 6. Rust plugin crates with dylib hot reload

**Status: to do.**

- §7: hot-path systems "are Rust, either in the engine or in a project plugin crate, and are compiled to a dynamic library the daemon loads." "For Rust plugins, the daemon snapshots, unloads the old dylib, loads the new one, restores, and continues. The agent sees a compile error as a structured `script.errors` response with file:line and the compiler's message, never as a crash."
- §2: "Rust plugin systems are loaded as dynamic libraries; a panic in one is caught at the system boundary, the system is disabled, and the fault is reported through the protocol as an event."
- §7: the two rules hold for plugins: a system touches only components it declared, and holds no references across frames.
- §17: "Rust plugin reload < 2 s after compile."
- §19: "The scheduler's write-set declarations make it possible to promote a Luau system to Rust without changing anything around it."
- `argus.toml`'s `plugins` list and `plugins/` folder exist since phase 0. A reference game ships a plugin crate.

## 7. Optional rigid-body module

**Status: to do.**

- §12: "Dynamic: real rigid bodies with joints, from an optional Rapier module for the games that need a physics toy, not a default." Three tiers "chosen per entity, sharing one broadphase".
- §6: the same run reaches the same world.
- §12: costs nothing for a game that does not attach the components; a release without it does not carry it.

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
