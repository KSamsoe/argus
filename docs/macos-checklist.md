# Argus on the Mac: checklist

Kevin, 2026-10-12: a Mac build that Todd downloads, with a testing packet Todd gives their AI, which tests everything it needs to and reports what it found. There is no Mac here, so the build is cross-compiled on klab (Kevin's choice over GitHub's Mac runners). It is one universal build, for Apple Silicon and Intel Macs alike, since Todd's Mac could be either. Todd gets it from a release on the private repository, once Kevin has invited them. The packet is written for any agent with a shell (Claude Code, Cursor, Codex), plus the MCP part for whichever one Todd uses.

Nothing here has run on a Mac. Until Todd's report comes back, "built" means compiled, linked, signed, and checked on klab. It does not mean it works.

## 0. The cross-build toolchain on klab

- Rust's two Mac targets, `aarch64-apple-darwin` and `x86_64-apple-darwin`.
- Zig as the linker and C/C++ compiler, through `cargo-zigbuild`. Its `universal2-apple-darwin` target builds both architectures and joins them into one program.
- Apple's macOS SDK for the system frameworks' headers and stubs. Apple's license allows the SDK only on Apple hardware; Kevin chose this route knowing that.
- `rcodesign` (apple-codesign) to check signatures. Apple Silicon runs only signed code, and an ad-hoc signature is enough for that.
- libclang (klab has 18) for `coreaudio-sys`, which generates its bindings from the SDK's audio headers while it builds.

**Done (2026-10-12).** Everything is in klab's `~/tools`, nothing system-wide, nothing needing root:
- `rustup target add`;
- Zig 0.17.0 from ziglang.org, checked against its published SHA-256;
- `MacOSX15.5.sdk` from github.com/joseluisq/macosx-sdks release 15.5, checked against its `sha256sum.txt`;
- `cargo-zigbuild` 0.23.4 and `apple-codesign` 0.29.0, from crates.io.

`tools/release/macos-env.sh` sets the build up.

## 1. It compiles for macOS

- `argus`, `argus-editor`, and `argus-game` (standard, and with Steam), for both architectures, in the `ship` profile.
- Each program is a Mach-O for arm64 and x86_64, signed, and links only Apple's system frameworks and libraries. The Steam runtime also links Steam's library beside it.

**Built (2026-10-12).** All four programs compile for both architectures with no changes to make them compile. Two things the build got wrong, both fixed in the build rather than the code:

- **The ship profile's strip removes the linker's signature**, and a Mac kills an unsigned program on Apple Silicon. `stage.sh` signs each program again, ad hoc, after it is copied in.
- **Every program said it needed macOS 15.** cargo-zigbuild gives Zig a target with no macOS version in it and drops any other, so Zig took the SDK's. Zig reads the version only from the target (`aarch64-macos.11.0-none`; `-mmacosx-version-min` is ignored). `macos-env.sh` puts a `zig` in front of the real one that writes the version into the target.

`tools/release/macho_check.py` checks each file without a Mac: both architectures, every page of its code against its signature's hashes, macOS 11 or older as its oldest, and only Apple's libraries (or Steam's, beside it) loaded. rcodesign's own `verify` reports every ad-hoc signature as broken. A program with one changed byte fails the check.

**Found by testing (Todd, 0.0.5, macOS 27): every program crashed at launch** with `dyld: duplicate linked dylib`. Zig's linker writes a load command for every `-framework` it is given, and rustc gives one for each crate that links a framework, so 0.0.5's programs named Foundation up to five times, CoreGraphics up to four, CoreFoundation and AppKit twice. Apple's linker would have listed each once. macOS 27's loader refuses a program that names a library twice. The build had counted the repeats but not failed on them, expecting macOS to load them anyway. Now `macos-env.sh`'s `zig` gives Zig each `-framework` once, and `macho_check.py` fails a program that names any library twice. Relinked that way on klab, `argus` for Apple Silicon loads each of its 18 libraries once.

**Mac bugs found by reading the code (2026-10-12), fixed:**

- **A daemon could start once per project and never again.** A Mac's local socket is a file in /tmp (Linux's are abstract and leave none), and a daemon ends without removing it, so the next one met "address in use". Now a daemon takes the name over when nothing answers on it; a daemon that still answers keeps it.
- **`argus new` and `argus build` could not find the install's templates and runtimes.** A Mac names a program by the link it was started through (`~/.local/bin/argus`), so the install's folders were looked for beside the link. `argus_cli::exe()` resolves it (not on Windows, whose resolved paths begin `\\?\`), and so do the editor and the game runtime. Daemons are now started by the install's own path too, so `install.sh` finds them to stop.
- **`argus mcp setup claude-desktop` and `vscode --scope user` wrote to Linux's folders** (`~/.config`) on a Mac, where those apps read `~/Library/Application Support`.
- **`write_file` could reach `.git` through `.GIT`** on a disk that ignores case: a Mac's by default, and Windows'. The engine's folders are now matched as the disk reads a name: any case, Windows' trailing dots and spaces dropped, and its short names (`GIT~1`). This one was a hole on Windows too.
- **A "software" frame was an error on a Mac**, whose Metal has no software rasteriser, so Hollow's picture goldens could not run. A Mac draws it with its GPU, and says so (`software: false`).
- **The full-screen setting was applied every frame**, which undid a Mac's green button at once. It is applied when it changes.
- **The audio device was asked for 48 kHz.** On a Mac, CoreAudio changes the device's rate for every program to do that, and some devices refuse it; then the game was silent. A Mac's device is used at its own rate, and on any system a stream the device refuses is tried once more at the device's own rate.
- **git was run for every run's code version.** On a Mac without Apple's developer tools, `/usr/bin/git` only offers to install them, in a dialog, each time. `git_usable()` asks `xcode-select -p` first, without a dialog.

Left for later: a game released for the Mac is a bare executable beside its `data/`, not an `.app` (double-clicking it in Finder opens Terminal); Mac keyboards send F8 to F10 as media keys unless fn is held (marks and proposals use them); a Rust plugin's library reloaded is not unloaded on a Mac (memory only).

## 2. A macOS install, staged on klab

- `tools/release/stage.sh --target macos` on Linux stages `argus-<version>-macos`, laid out as the other installs are: `bin/`, `runtime/macos/standard` and `runtime/macos/steam` (with `libsteam_api.dylib`), `templates/`, `install.sh`, `manifest.json`.
- It cannot run what it built, so it ends by checking each program's architectures and signature instead of running `argus version`.

**Built (2026-10-12).** `stage.sh --target macos` builds each program for each architecture with cargo-zigbuild and joins the two with `lipo.py`. cargo-zigbuild's own `universal2` target once made an editor whose arm64 part was 384 KB, from a 9.5 MB arm64 build, on an incremental build. Signing kept only its x86_64 part, and the check caught it. Then it signs the programs and checks every file. The stage is 200 files, 136 MB, and every program passes `macho_check.py`.

## 3. The release has the Mac

- `release.sh` stages the Mac install on klab beside the Linux one.
- Every install carries every platform's runtimes, so any of them can release a game for the Mac: `argus build --target macos` puts the game's packs beside the Mac runtime.
- The archives:
  - `argus-<version>-macos-universal.tar.gz`, with the programs, Steam's library, and the installer executable (`tarball.py`);
  - `example-games-<version>-macos-universal.tar.gz`: Hollow, Delve, and Outpost, released for the Mac from the Windows install. Swarm is not in it: its Rust plugin is compiled on the machine it runs on, and the build says so.
- `argus-<version>-macos.mcpb`, the Claude Desktop bundle for the Mac. Its tool list comes from the Windows program, which serves the same tools.
- `install.sh` on a Mac:
  - picks the Mac archive;
  - checks it with `shasum -a 256` when there is no `sha256sum`;
  - stops running daemons without `/proc`;
  - clears the quarantine flag a browser download puts on the files (Gatekeeper would refuse the programs, which are signed but not notarised).

**Built (2026-10-12)**: release 0.0.5, from 071b316. klab staged the Mac install, and every program passed `macho_check.py`. The Windows install released Delve, Hollow, Outpost, Puppet, and Toybox for the Mac, each checked, and refused Swarm with the plugin reason. Taken back out of the archives, the programs are mode 755 and pass the check again; Hollow and Outpost have Steam's library beside them. The Mac Claude Desktop bundle declares `darwin` and keeps `bin/argus` executable. The release's last step, that bundle, first failed: `mcpb.py` ran the Windows `argus.exe` from its own folder by a relative path. It runs it by an absolute one now, and the bundle and `SHA256SUMS` were made by hand from the same stages.

## 4. The testing packet

- One document, `docs/mac-testing.md`, attached to the release. It covers:
  - what to download and how;
  - each check as commands with what should come back, in order, cheapest first, each with its own pass or fail;
  - the report to send.
- What it covers:
  - the programs start;
  - `argus new` with every template, and the scenarios pass;
  - the example projects' scenarios, with each run's digest written down;
  - a frame drawn with Metal, and `obs.frame` pictures;
  - sound through the Mac's audio device;
  - an art script and a generated sound, against the pinned outputs;
  - the daemon and its socket, the CLI, `argus mcp`, and `argus mcp setup` for Todd's agent;
  - `argus build` for the Mac, and the built game run headless;
  - the editor and a game window, which Todd looks at, with what to look for;
  - `argus self-update` and `argus uninstall`.
- The digests the four games' scenarios end on, from Windows and klab, so the report says which differ. The games played back identically on Windows, Linux, and in Chrome.
- The report is a GitHub issue on the repository (with `gh`), or a file Todd sends. It has every check's result, the machine (chip, macOS version), and anything the agent saw that the packet did not ask about.

**Written (2026-10-12):** `docs/mac-testing.md`: sections 0 to 12, from the machine to Rust plugins, then the report. Every command in it was run on Windows first, with what it answered. The expected results are `mac-expected.json`, made by `tools/release/mac_expected.py`:
- each example game's digests, scenario by scenario and seed by seed, from Windows;
- what `generate` makes of a laser seed 3 and a creature seed 2: the recipe's and the script's bytes, the sheet's and the preview's, and the sound's description.

## 5. The release Todd downloads

- A version bump, and a draft release with every platform's files, the packet, and the digests. Kevin publishes it as a pre-release, and invites Todd to the repository (a draft is visible only to people who can push).

## The gate

- Todd's AI works through the whole packet on Todd's Mac, and every check passes or its failure is fixed and checked again.

**Met (2026-10-12), but for two checks that Todd will do later.** The report is GitHub issue 2: Argus 0.0.6 on an Apple M4 Max, macOS 27.0.1, arm64, with Apple's developer tools and no Rosetta.

- **The 0.0.5 blocker is fixed.** 0.0.5 crashed at launch because each program named some system libraries more than once. 0.0.6 installs and runs, from a download marked by a browser too.
- **Every automated check passes:** the templates, the daemon started a second time, frames with Metal, the art and sound recipes against Windows' (every hash and every field the same), a release for the Mac and from it for the other platforms, the MCP server, updating and removing, and Swarm with its Rust plugin built on the Mac.
- **Digests:** 338 of 338 runs end on Windows' digest, across Hollow, Delve, Outpost, Toybox, and Swarm. So the games play the same on Apple Silicon as on x86-64 Windows and Linux and in Chrome. Puppet differs, as expected (its skeletons use the platform's sin and cos).
- **Hollow's two picture goldens differ by 0.40 to 0.61% of pixels,** at text and sprite edges, against 0.10% allowed. That is a GPU against Windows' software rasteriser, about Linux's size.
- **Todd's look and listen:**
  - sharp on a 4K screen, with music and sound;
  - keys, the menu, and VoiceOver work, and a gamepad works;
  - the green button's full screen stays;
  - the F8 report works with fn;
  - the editor works;
  - a released game opens.
- **Still to do:** 10.2 (the agent's own host connected over MCP) and the Claude Desktop bundle's install (10.3), which Todd will do later.
- **The report also noticed, none of it blocking:**
  - released Steam games print Steam's three warning lines to stderr when Steam isn't running;
  - `argus mcp` run in a game's folder leaves that game's daemon running after the pipe closes;
  - the packet's 6.1 promised a flag that is off the screen, 8.2 didn't say `argus editor` returns at once, and 9 stopped a daemon that a build doesn't leave. These three are fixed in the packet.
