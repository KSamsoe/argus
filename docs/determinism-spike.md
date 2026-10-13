# The cross-platform determinism spike (phase 5 item 11)

The four reference games run bit for bit the same on this PC (Windows), on klab (Linux, a different CPU), and in Chrome. Every field of every entity, and every event, matched at every checkpoint of 13 long runs, 93,000 ticks in all. So did toybox's rigid bodies. So did every scenario run of their test suites on this PC and klab: 290 runs, each ending on the same world.

One thing breaks it, and it is in the engine, not the games. Rust code that calls `sin`, `cos`, `atan2`, `exp` or `pow` on an `f32` gets each platform's own math library. The three platforms' libraries disagree on up to 17% of inputs, by one unit in the last place (ulp). The skeleton runtime does this for every bone on every tick, so puppet differs between every pair of platforms from its first tick.

Luau's math is safe in practice. A script computes in doubles, and a component keeps `f32`, so the last-bit differences round away. Of 160,000 probe values, not one differed after that rounding, on any platform.

§6 calls cross-platform determinism a non-goal for v1 and names the known path: "fixed-point math, or a constrained float subset". The spike says the subset is enough, and the section below says what it is.

## Try it

```
node tools/determinism/runs.mjs
tools/determinism/native.sh windows
node tools/determinism/web.mjs target-web/det/hollow tools/determinism/runs/hollow-*.json
node tools/determinism/compare.mjs windows web
```

`runs.mjs` writes the run files: one long run a game, its input from a seeded walk. `native.sh <label>` plays every run with this machine's release build (the `determinism` example of `argus-web`). `web.mjs <site> <runs>` plays them in headless Chrome from a built site's packs (`argus build --target web --release --out target-web/det/<game>`). `compare.mjs` lines the labels up and gives the first checkpoint that differs, where `--rows <tick>` on either side dumps the world a line a field, to diff.

The other two tiers:

```
tools/determinism/scenarios.sh windows
node tools/determinism/compare.mjs --scenarios windows klab
target/release/examples/determinism --math 20000
```

`scenarios.sh` runs every game's suite; each run now ends with its world's digest (`test.run`'s rows carry `digest`). `--math` is the math-library probe.

On klab the copy is `~/argus-det`, with every crate vendored from this PC (`cargo vendor --offline`), so nothing came from the network there.

## The machines

| | This PC | klab | Chrome |
|---|---|---|---|
| CPU | Intel i9-12900KF | Intel Xeon E5-2699 v3 (Haswell) | the PC's |
| System | Windows 10 | Ubuntu 24.04.5 | Chrome 154, headless |
| Target | x86_64-pc-windows-gnu | x86_64-unknown-linux-gnu | wasm32-unknown-unknown |
| Math library | mingw-w64 and the UCRT | glibc 2.39 | musl's, in Rust's port (Rust) and wasi-libc (Luau) |
| Compiler | Rust 1.98.1, GCC 16.1 (WinLibs) for Luau | Rust 1.98.1, GCC 13.3 for Luau | Rust 1.98.1, wasi-sdk's clang for Luau |

macOS is not measured: there is no Mac here. See "Not measured" below.

## The long runs

Each run replays the same input log from the load and takes both digests every few ticks: the world's (every field of every live entity, floats by their bits, the runtime maps' cells) and the events'.

| Run | Ticks | Entities at the end | klab | Chrome |
|---|---|---|---|---|
| Hollow, seed 1 | 7,200 | 84 | same | same |
| Hollow, seed 2 | 7,200 | 135 | same | same |
| Delve, seed 1 | 500 | 28 | same | same |
| Delve, seed 2 | 500 | 27 | same | same |
| Delve, seed 3 | 500 | 27 | same | same |
| Delve, seed 4 | 500 | 27 | same | same |
| Delve, seed 5 | 500 | 30 | same | same |
| Delve, seed 6 | 500 | 29 | same | same |
| Swarm, seed 1 | 10,800 | 923 | same | no browser build |
| Swarm, seed 2 | 10,800 | 887 | same | no browser build |
| Outpost, seed 1 | 25,200 | 175 | same | same |
| Outpost, seed 2 | 25,200 | 171 | same | same |
| toybox, seed 1 | 3,600 | 35 | same | same |
| puppet, seed 1 | 3,600 | 23 | differs from tick 30 | differs from tick 30 |

Swarm has no browser build: its chase is a Rust plugin, and a page cannot load one (phase 5 item 6). Delve has six shorter runs, not one long one, because its hero dies on the first floors and the game has no restart, and each seed is a new dungeon.

## Every scenario

Every scenario of six suites over their seeds, on this PC and klab, each run's world digest when it ended. Each suite also ran twice on this PC, and all 293 runs gave the same digest both times, so a difference between machines is the machines'.

| Suite | Runs | Same world on klab |
|---|---|---|
| Hollow | 23 | 23 |
| Delve | 212 | 212 |
| Swarm | 9 | 9 |
| Outpost | 43 | 43 |
| toybox | 3 | 3 |
| puppet | 3 | 0 |

Puppet's three scenarios still pass on klab: what they check (the hero stands, walks, lands its feet, waves once) holds, but each ends on another world's digest, as the long run does.

**Since fixed (2026-10-12).** A pose's trig (`argus-skeletal`'s pose.rs: sin, cos, atan2, acos) is `libm`'s now, not the platform's. Puppet's three scenarios end on the same digests on Windows and on klab. A Mac also differed in Todd's test of 0.0.6, issue 2, and will be checked with the next release. Hollow's picture goldens are now kept per system and held to 1% against another system's (testing.md), so they pass on klab too.

Two of Hollow's runs fail on klab, and not because of the world: see "Pictures" below.

## Pictures

Two of Hollow's scenarios compare frames with saved pictures (image goldens). On klab they fail: 0.39 to 0.60% of each picture's pixels differ, where 0.1% is allowed, and the worlds are the same. The pictures are drawn with "the software rasteriser", which is wgpu's fallback adapter: WARP on Windows, lavapipe (Mesa 25.2) on Linux. They are two rasterisers, and they draw text and sprite edges a little differently; everything else in the pictures matches. So a golden is the same picture on machines of one system, not on any machine, as the harness's comment had it (it says so now). CI on a second system would keep its own goldens.

## The math libraries

Each function over the same 20,000 inputs, compared with the `libm` crate, a pure-Rust port of musl that gives the same bits on every platform. The table counts results that differ from it. Every difference is one ulp.

| Function | Windows f32 | klab f32 | Windows f64 and Luau | klab f64 and Luau | Chrome, all three | Luau kept as f32, all platforms |
|---|---|---|---|---|---|---|
| sin | 34 (0.2%) | 263 (1.3%) | 335 (1.7%) | 621 (3.1%) | 0 | 0 |
| cos | 28 (0.1%) | 254 (1.3%) | 341 (1.7%) | 618 (3.1%) | 0 | 0 |
| tan | 2,640 (13.2%) | 2,732 (13.7%) | 764 (3.8%) | 712 (3.6%) | 0 | 0 |
| atan2 | 3,279 (16.4%) | 38 (0.2%) | 3,476 (17.4%) | 3,478 (17.4%) | 0 | 0 |
| exp | 1,915 (9.6%) | 1,915 (9.6%) | 1,913 (9.6%) | 1,957 (9.8%) | 0 | 0 |
| ln | 617 (3.1%) | 621 (3.1%) | 589 (2.9%) | 593 (3.0%) | 0 | 0 |
| pow | 1,916 (9.6%) | 1,920 (9.6%) | 1,901 (9.5%) | 1,936 (9.7%) | 0 | 0 |
| sqrt | 0 (0.0%) | 0 (0.0%) | 0 (0.0%) | 0 (0.0%) | 0 | 0 |

`sqrt` is exact everywhere, as IEEE 754 requires, and so are `+`, `-`, `*` and `/`. Chrome's results are musl's exactly, through both Rust's port and Luau's C library. Windows and klab each differ from musl, and from each other (their digests differ for every function but `sqrt`). Luau's doubles call the same C functions Rust's `f64` does, so their counts match Rust's `f64` counts on each machine.

## Why the games agree anyway

- **Most game logic is exact.** Integers, comparisons, the RNG streams, and the four IEEE operations give the same bits on every x86-64 CPU and in wasm. Delve's and Outpost's scripts call no math function at all.
- **Luau computes in doubles; components keep `f32`.** Swarm takes `cos` and `sin` of a random angle for every enemy it spawns, and Hollow's light glides with `exp`. The double may differ in its last bit, but rounding it to `f32` (29 fewer bits) erases that unless the double sits within one ulp of an `f32` rounding boundary, about one value in 500 million. The probe measured it: 0 of 160,000 results differ after rounding, on all three platforms.
- **Rapier has its own math.** Rigid bodies use Rapier's `enhanced-determinism` feature (chosen in phase 5 item 7), which sends its math through `libm`. Toybox matches everywhere.
- **The games' Rust is plain arithmetic.** Swarm's plugin chase calls no math function.

## Where it breaks

Puppet, on the first tick: two skeleton parts' rotations (`Transform.rot` and `GlobalTransform.rot` of entities 8 and 22) differ by one ulp between Windows and klab. By tick 30, 15 fields differ. The skeleton runtime does Spine's arithmetic in `f32`, with `atan2`, `sin` and `cos` for every bone's IK and rotation, and `atan2` is where the libraries disagree most (16% of inputs on Windows).

The same would happen anywhere else the engine's Rust calls those functions on state: tweens with sine or power easing, the camera's follow, zoom, and shake (`exp`, `powf`), and particles' emission angles (`sin_cos`; particles are drawn, not part of the world's digest). The four games' runs did not reach them in a way that changed a field, and that is luck, not design.

## What a game needs for lockstep

§15 keeps networking possible: "Determinism plus an action log is most of a lockstep model". Measured here, the action log half is done and the determinism half needs four changes:

1. **One math library for the engine's `f32` functions.** Route `sin`, `cos`, `tan`, `atan2`, `exp`, `ln`, `powf` (and the rest) through `libm`, as Rapier already does, in the engine and in the plugin SDK. The browser already gives `libm`'s bits, so pages need nothing. It costs little: on this PC `libm` takes under 3 ns more a call than the platform's `sin`, `cos`, `tan`, `exp` and `ln`, about the same for `atan2`, and `pow` is about four times slower (30 ns against 8). A thousand such calls a tick would cost a few microseconds more (about 20 if they were all `pow`).
2. **The same for Luau, for strict lockstep.** The `f32` rounding protects what components keep, but a script's own state (a double in a local table kept across ticks) holds the full double, and a difference there can surface later. Strict lockstep would give Luau `libm`'s functions too, compiled into Luau's C++ in place of the system library. Replacing them from the host does not work: tested, a `math.sin` replaced before the sandbox goes on is ignored once it is (Luau's built-in fast path calls the C function), and `x ^ y` never goes through `math.pow`.
3. **No fused multiply-add in Luau built by clang on ARM.** Luau's C++ is built as ISO C++17 with the compiler's defaults. GCC does not fuse `a * b + c` in ISO mode, but clang does within an expression (`-ffp-contract=on`), where the CPU has the instruction. The browser's build is clang, but wasm has no such instruction, and x86-64's baseline has none either, which is why it did not matter here. An Apple-silicon Mac (Apple's clang, ARM) would fuse, so Luau needs `-ffp-contract=off` there. Rust never fuses on its own.
4. **A desync check cheap enough to run often.** The world's digest costs 0.1 ms for puppet's 23 entities, 0.9 ms for Outpost's 175 and 1.9 ms for Swarm's 900 on this PC (2.5 ms on klab), because it formats every field as text. Lockstep peers would compare a cheaper one, hashing columns' bytes rather than formatting fields, every second or so, and use the existing digest and `--rows` to find what differs.

What is already true and stays true:

- Inputs arrive only as per-slot actions in the input log, and a script has no clock or device (the sandbox exposes no `os` or `io`).
- Parallel systems give the same world on any number of threads (phase 5 item 5).
- Packs are content-addressed, and a mark or report names its code revision, so peers can check they run the same game.

Fixed point is not needed. The "constrained float subset" of §6 is: IEEE's four operations and `sqrt` as they are, and every other function from one software library.

## Recommendation

Nothing changes for v1: §6's non-goal stands. When multiplayer comes, do the four changes above first, then put this spike's tools in CI on two machines (this PC and klab) so a change that breaks agreement is caught where it lands. Moving the engine's math to `libm` changes results on Windows (up to 17% of a function's outputs), so state goldens that depend on them would be rewritten once, in that change.

## Not measured

- **macOS.** No Mac here. An Apple-silicon Mac is ARM: its math library differs again (changes 1 and 2 cover it) and Luau's C++ would fuse multiply-adds (change 3).
- **ARM Linux, other browsers.** In another browser the wasm module is the same module, with its own math, so it should match Chrome; its NaN bits could differ, but no run produced a NaN.
- **Other compiler versions.** All three builds used Rust 1.98.1. A Rust upgrade can change Rust's musl port, and so the browser's results; with change 1, the `libm` version in `Cargo.lock` decides instead.
