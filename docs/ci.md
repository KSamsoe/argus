# CI on the reference laptop

CI is one script, run by hand on the reference laptop before a release:

```sh
bash tools/ci/ci.sh            # every step
bash tools/ci/ci.sh fmt clippy # only those
```

The design's performance targets (section 17) are numbers on this laptop, so this is where they are checked. A full run takes about an hour. Between releases, run the tests a change touches; the whole run is for a release, after all its work is in.

Until 0.2.0 a self-hosted GitHub Actions runner on this laptop ran the same script from the repository's Actions tab. It was removed before the repository went public (Kevin, 2026-11-13): on a public repository, anyone's pull request could add a workflow that runs code on the machine a self-hosted runner is on.

## What a run checks

| Step | What it does |
|---|---|
| fmt | The code is formatted as `cargo fmt` would format it. |
| clippy | No clippy warnings, in any crate or test. |
| test | The workspace's tests. Then the render tests again on the software rasteriser (WARP), as a machine with no GPU would draw. |
| games | The test suites of the seven examples: Swarm, Delve, Hollow, Outpost, toybox, puppet, and Duel. A failing run's clip is drawn with the software rasteriser. |
| perf | The release-build timing tests: every section 17 target except the web page's, which the next step checks. They run one at a time, since a timing test that shares the machine with another reads slow. The Outpost hand-tuning gate (21 minutes) is left to phase gates. |
| web | Hollow's web page is built and played in headless Chrome. This includes a cold load over a 20 Mbit link, which must finish in under 5 seconds. |
| det | Every example gives the same run everywhere: long played runs and every scenario, on this laptop and on klab, and the runs again in headless Chrome. A world that differs by one bit anywhere fails the step, since players of one match must compute the same world. It checks the last commit, so it refuses a tree with uncommitted changes. Its reports are in `target/determinism/`. |

Every step runs even if an earlier one failed. The run fails if any step failed, and prints the last 40 lines of each failing step. Each step's full log is in `target/ci/<step>.log`.

## What it needs

These are installed on this laptop:

- Git for Windows, at `C:\Program Files\Git`.
- Rust, with the GNU toolchain and the `wasm32-unknown-unknown` target.
- WinLibs gcc on the PATH.
- wasi-sdk 34 at `C:\Users\Kevin\tools\wasi-sdk-34.0-x86_64-windows`. Set `WASI_SDK` if it moves.
- wasm-bindgen-cli, at the version in `Cargo.lock`.
- Node 24 and Chrome.
- ssh to klab (`ssh klab`, from `~/.ssh/config`), for the det step. Set `ARGUS_DET_HOST` for another Linux machine; it builds in `~/argus-ci` there.

## On klab

The steps that only compile and test (fmt, clippy, test, games, perf) run on klab too, to spare this laptop: check out the commit there and run `CARGO_HOME=$HOME/.cargo bash tools/ci/ci.sh fmt clippy test games perf`. Some results there are not this laptop's: the timing targets are this laptop's numbers; the render tests and a few others draw on a GPU, and klab has none a headless test can use, so they hang or differ there; and a few tests assume Windows (a release for "the other platform", a Windows path). Run those on this laptop. The det and web steps always run here: det compares this laptop with klab, and web needs Chrome and wasi-sdk.

## Good to know

- **The timing tests assume a quiet laptop.** They expect it plugged in and not busy with a game or another build. If a timing test fails while the laptop was busy, run it again before looking for a slowdown.
- **Keep the laptop awake.** A run is about an hour; a laptop that sleeps partway makes slow games and failed timings.
- **Old test programs are pruned.** Each build of a test is its own program, named by a hash of what it was built from, so a version bump or a changed dependency leaves the old ones behind (a checkout here once held 1,172 of them, 160 GB, and the disk ran out). After every run, the run removes the ones a newer build replaced. Keep an eye on the disk all the same: a build cache, debug and release, is about 85 GB.
