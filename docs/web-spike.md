# The web spike (phase 2 item 1)

Hollow runs in a browser. The player runtime, with no daemon, is compiled to wasm and draws into a canvas with WebGPU, or WebGL2 where WebGPU is missing. Keys move the hero and the footsteps play through the browser's audio. A run in the browser matches the same run on the desktop exactly. A mark taken in the browser is 239 bytes, and the desktop replays it to the same world.

This was a spike, not the build. It answers the questions the checklist asked, and it leaves notes for items 2 and 7. The build itself is item 7.

## Try it

The site is built into `target-web/site`. To play it in your own browser:

```
node tools/web/serve.mjs
```

Then open http://127.0.0.1:8765/ and click the game. Add `?webgl` to force WebGL2, `?seed=3` for another seed, and `?paused` to draw without running.

To rebuild it you need the wasm Rust target, wasi-sdk 33 or later, and wasm-bindgen-cli at the version in `Cargo.lock` (`tools/web/env.sh` says where each goes). Since phase 2 item 7, `build.sh` is `argus build --target web --release`, which writes the page with the project's release packs. Then:

```
tools/web/build.sh
node tools/web/check.mjs
```

The check opens the site in headless Chrome, with no window and the sound muted. It reports what was drawn, the run's digests, what the keys did, the audio, and the mark, and saves screenshots to `target-web/check`. Since phase 2 item 6 it also runs the hero into the hall's pit and takes a second mark during the hit-stop that follows, which the desktop must replay to the same world, flash and shake included.

## The answers

| Question | Answer |
|---|---|
| Does Luau run in wasm? | Yes. Luau is C++ and raises its errors as C++ exceptions, so it is compiled with wasi-sdk's clang and wasm's own exception handling, in its new form (exnref). Chrome refuses a module that mixes the old and new forms, so both Luau and wasi-sdk's C++ runtime use the new one. The C library's eight system calls are answered inside the module (`crates/argus-web/src/wasi.rs`). |
| Does the renderer work as it is? | Yes, unchanged apart from how it finds its surface. WebGPU is what Chrome picks. Forcing WebGL2 runs on ANGLE over Direct3D 11. Both draw the same picture, with the right colours. |
| Does audio work? | Yes, through cpal's Web Audio backend. A browser only lets a page make sound after a key press or a click, so the page opens the audio on the first one. In the check, 2.4 seconds of sound were mixed at 48 kHz, peaking at -24.7 dBFS from the footsteps. |
| Does input from a canvas work? | Yes. The browser names keys the way winit does (`KeyW`, `ArrowLeft`, `Space`), so a game's `input.toml` works unchanged. Holding the right arrow for 1.5 s moved the hero about 10.5 units, with 10 footsteps. |
| Are browser and desktop runs the same? | Yes, bit for bit. Every field of every entity and every event matched at every checkpoint (table below). §6 does not promise this, but for Hollow it holds. |
| What can a browser mark carry? | The run's identity and its input: scene, seed, code revision, tick, input log, the two digests, and a note. No snapshot is needed, because the desktop can replay the log from load and land on the same world. The digests say whether it did. |
| Does anything listen on a socket? | No. The web player has no daemon and no protocol (§15). |

## Determinism

Hollow's run scenario (hold right, jump the step, run into the hall), stepped in the page and natively. The world digest covers every field of every live entity, with floats compared by their bits. The event digest covers every event's frame, name and fields.

| Tick | World digest | Event digest | Chrome | Desktop |
|---|---|---|---|---|
| 0 | `ad3316ce03bfdfc4` | `9e2c805e1180ac29` | same | same |
| 118 | `e6d52015ddec8f83` | `f956d3c394945728` | same | same |
| 448 | `0937b8f2e8a3a252` | `164061bd77a8b874` | same | same |
| 1048 | `4690e3af36e5652f` | `057afa065e885fef` | same | same |

The desktop side is a debug build. The browser side was checked three ways: a normal release build, a build tuned for size, and wasm under Node. All matched. So optimisation doesn't change the answer, and neither does the graphics backend, which never touches the simulation.

The mark replays too. The check holds a key in real time in Chrome, takes a mark, and runs `mark_replay` on the desktop. In every run the desktop landed on the same digests at the same tick. The tick differed between runs, since real time decides when the key goes down.

A mark looks like this:

```json
{"scene":"hollow","seed":1,"tick":146,"code":"git:0a757c4+dirty",
 "digest":"02c569cab37c02da","events_digest":"e60657482b525a11",
 "log":[[35,1,"move",[1.0,0.0]],[127,1,"move",[0.0,0.0]]],
 "note":"held the right arrow for a second and a half"}
```

## Size and speed

| | Size | gzip | brotli |
|---|---|---|---|
| wasm, release as it is | 8.31 MB | 2.22 MB | 1.46 MB |
| wasm, tuned for size | 4.25 MB | 1.52 MB | 1.12 MB |
| JavaScript glue | 0.12 MB | 0.02 MB | 0.02 MB |
| Hollow's pack (35 files) | 30 KB | 10 KB | 10 KB |

The size-tuned build uses `opt-level = "s"`, fat LTO, one codegen unit, no debug info, and wasm-bindgen's `--remove-name-section`. wasm-opt was not tried.

| | |
|---|---|
| Page open to first frame, local server | 360 to 435 ms |
| One tick of Hollow in Chrome | 0.04 ms |
| One tick of Hollow in wasm under Node | 0.05 ms |

## What was built

- **`argus-os`**, a new crate. Every file and clock access in the eight runtime crates (sim, luau, scene, input, assets, tiles, render, audio) goes through it. On the desktop it is `std::fs` and `std::time`. In the browser it is an in-memory file system mounted from a pack, and the browser's clock.
- **`argus-web`**, a new crate. It holds the pack format, a `Player` that opens a project's scene the way the daemon does but without the daemon, and the page's bindings (`WebPlayer`, `open_renderer`). `Player` builds natively too, so the same code runs on both sides of the determinism check.
- **The renderer on the web.** `Renderer::web(canvas, webgl)` picks WebGPU and falls back to WebGL2, which runs with the downlevel limits.
- **Audio on the web.** argus-audio's device opens through cpal's Web Audio backend. The page opens it on the first key or click.
- **Marks from the browser.** `WebPlayer::mark(note)` and `Player::replay_log`, plus the `mark_replay` example that checks one on the desktop.
- **Tools.** `tools/web/`: `env.sh` (the wasm C++ toolchain), `build.sh`, the page (`index.html`, `main.js`), `serve.mjs`, and `check.mjs`.

## Found along the way

- **A test was rewriting Hollow's files.** The planks test moves the hero with `edit.commit`, and that wrote the new position into `player.toml`. The phase 1 gate commit carried it, so the Hollow you played started under the plank instead of at the cave mouth. Now `test.run` edits the live world only: a set on an authored entity changes the world, a despawn doesn't delete the file, and a spawn that would need a file is refused. The file is back to the cave mouth, and `argus-daemon/tests/test_run.rs` fails if a test writes a file again.
- **The first frame wasn't framed.** The camera settled on its first tick, so the frame before it showed whatever the authored camera position showed: past the room's wall in Hollow. On the desktop a real-time game ticks at once, so it lasted one frame. A turn-based game would show it until the first turn. The camera now also runs when a scene loads, so the first frame is framed.
- **The player copies the daemon's scene assembly.** `Player::open` repeats what the daemon's `open_scene` does. Item 7 should share one assembly between them.
- **A Windows DLL can't hold the web player.** Built natively as a cdylib it exports more symbols than a DLL allows. So `argus-web` is a plain library, and `build.sh` asks for the cdylib only for wasm.
- **Three renderer items are desktop-only.** They are now marked so the wasm build has no warnings.

## For the items after this

- **Item 2 (marks).** The browser mark is the model: the run's identity, the input log, the digests to check against, and a note. A desktop mark adds what §10 asks for: a snapshot, the last five seconds of input, and a clip. A web mark can skip the snapshot, since the desktop replays it from load.
- **Item 7 (the build).**
  - Ship the size-tuned profile.
  - Move audio mixing into an AudioWorklet. cpal's Web Audio backend mixes on the page's main thread, two 2048-frame buffers ahead (about 85 ms), so a slow frame can starve it.
  - Share the scene assembly with the daemon.
  - Make the pack content-addressed, as §15 asks.
  - Serve the site from klab, and post marks there.
- **The code revision.** A mark says which code made it (`git:<rev>`, with `+dirty` when the crates or the project were edited). The desktop can only replay a mark with the same code. A dirty build's mark can be replayed only against that working tree, so the build item should refuse to publish a dirty build.
- **Browsers.** Only Chrome was checked. The new wasm exception handling that Luau needs is recent. A browser without it will refuse the module, and the page will say "Could not start".
