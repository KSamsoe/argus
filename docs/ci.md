# CI on the reference laptop

CI runs on this laptop, on a GitHub Actions runner Kevin installs once (phase 5 item 0). It runs when started by hand: open the repository's Actions tab, choose "ci", and click "Run workflow". Running it on every push was turned off (Kevin, 2026-10-07): a run takes about forty minutes, too slow for private work. It goes back on with public builds, by adding `push` and `pull_request` to the `on:` list in `.github/workflows/ci.yml`. `bash tools/ci/ci.sh` runs the same checks without the runner. The design's performance targets (section 17) are numbers on this laptop, so this is where they are checked.

The workflow is `.github/workflows/ci.yml`. It runs `tools/ci/ci.sh`, which can also be run by hand.

## What a run checks

| Step | What it does |
|---|---|
| fmt | The code is formatted as `cargo fmt` would format it. |
| clippy | No clippy warnings, in any crate or test. |
| test | The workspace's tests. Then the render tests again on the software rasteriser (WARP), as a machine with no GPU would draw. |
| games | The test suites of Swarm, Delve, Hollow, and Outpost. A failing run's clip is drawn with the software rasteriser. |
| perf | The release-build timing tests: every section 17 target except the web page's, which the next step checks. They run one at a time, since a timing test that shares the machine with another reads slow. The Outpost hand-tuning gate (21 minutes) is left to phase gates. |
| web | Hollow's web page is built and played in headless Chrome. This includes a cold load over a 20 Mbit link, which must finish in under 5 seconds. |

Every step runs even if an earlier one failed. The run fails if any step failed, and the Actions page shows the last 40 lines of each failing step.

Each run keeps an artifact named `ci-<run number>` for 14 days, holding:

- Every step's full log (`target/ci/`).
- The web check's report and screenshots.
- For a picture that no longer matches its golden, the picture drawn and the difference image (golden faded, differing pixels in red).
- For a failing scenario, its replay. Open it with `argus replay open path=<its folder>`.

## Installing the runner (once)

1. Open https://github.com/KSamsoe/argus/settings/actions/runners/new and choose Windows, x64.
2. In PowerShell, make the folder `C:\actions-runner` and run the page's Download commands there. A short path avoids Windows' long-path limits, since the runner's own checkout and build caches go under this folder (tens of GB).
3. Configure it with the token the page shows, adding the label the workflow asks for:

   ```powershell
   ./config.cmd --url https://github.com/KSamsoe/argus --token <the page's token> --name reference-laptop --labels argus-reference --work _work --unattended --replace
   ```

   `--unattended` answers its questions with the defaults: the default runner group, and not a service. Without it, config stops to ask, and whatever you type next becomes the answer. It should run as you, not as a service: a service runs as another Windows account, without your Rust, wasi-sdk, and Chrome, and with unreliable access to the GPU. `--replace` takes over a runner of the same name if an earlier attempt registered one. The token lasts an hour; if config refuses it, reload the page for a new one. Config ends with "Settings Saved."
4. When config has finished, start the runner:

   ```powershell
   ./run.cmd
   ```

   It says "Listening for Jobs". Leave the window open.
5. To have it start when you log in, add a logon task. This opens the runner's window each time you log in:

   ```powershell
   schtasks /create /tn "argus CI runner" /tr "C:\actions-runner\run.cmd" /sc onlogon
   ```

To check that it works, open the repository's Actions tab, choose "ci", and click "Run workflow".

## What the runner uses

These are already installed on this laptop:

- Git for Windows, at `C:\Program Files\Git`.
- Rust, with the GNU toolchain and the `wasm32-unknown-unknown` target.
- WinLibs gcc on the PATH.
- wasi-sdk 34 at `C:\Users\Kevin\tools\wasi-sdk-34.0-x86_64-windows`. Set `WASI_SDK` if it moves.
- wasm-bindgen-cli, at the version in `Cargo.lock`.
- Node 24 and Chrome.

## Good to know

- **The laptop must be on.** A run only happens while the laptop is on, you are logged in, and the runner is running. A push made while it is off waits. GitHub drops a job that has waited 24 hours; run it again from the Actions page.
- **The timing tests assume a quiet laptop.** They expect it plugged in and not busy with a game or another build. If a timing test fails while the laptop was busy, run it again before looking for a slowdown.
- **The runner has its own build caches.** Its checkout is `C:\actions-runner\_work\argus\argus`, with its own build caches. A run cleans everything else in that checkout first, and empties the build cache when it grows past 100 GB (a working cache, debug and release builds, is about 85 GB; the run that empties it builds from cold, which takes about forty minutes). Keep an eye on the disk: this cache and your own checkout's can each reach that size.
- **Keep the repository private.** While this runner is attached, a public repository would let a stranger's pull request run code on this laptop.
- **Running it by hand:** `bash tools/ci/ci.sh` runs every step. `bash tools/ci/ci.sh fmt clippy` runs only those steps.
