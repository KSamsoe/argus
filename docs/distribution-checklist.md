# Distribution checklist

After phase 5, Argus runs only where it was built: a clone of the private repository, Rust on the GNU target, and a C++ compiler for Luau. This plan makes it something a person installs on a machine and connects to the AI tools they already use. The order and four decisions were agreed with Kevin on 2026-10-09.

## Decided with Kevin (2026-10-09)

- **Who first: Kevin's machines, then people he invites.** Releases live on the private GitHub repository. A collaborator installs with a GitHub token, and no license or public listing is needed yet. Going public later is a switch (a public releases repository, a license, the MCP registry), not a rebuild.
- **Windows and Linux first.** Both are built and tested here: this laptop, and klab as the Linux machine. macOS is recorded as not built, as in phase 5 item 11. (From 0.0.5 a Mac build is cross-built on klab and tested on a Mac by Todd: [macos-checklist.md](macos-checklist.md).)
- **Every MCP client:** Claude Code and Claude Desktop, Cursor and VS Code, Codex CLI and Gemini CLI, and remote MCP for the web apps (claude.ai, ChatGPT).
- **Releasing a game needs no Rust.** The install carries the game runtime for each platform and the web player, and `argus build` packs a game around them. A game with Rust plugins still needs Rust, for the plugin only.

## What there is now

- `argus.exe` and `argus-editor.exe` start from any shell: the C and C++ runtimes are linked in (`.cargo/config.toml`).
- `argus mcp --project <dir>` is a stdio MCP server, with tools generated from the one schema (phase 2 item 8). `argus docs --for-agent` writes ARGUS.md, which now carries the scripting and scenario APIs (the phase 5 gate).
- `argus build` compiles a game's executable from the engine's crates: it needs the source tree (`ARGUS_ENGINE`, or the path the CLI was built at) and cargo. A web build also needs the wasm target, wasi-sdk, and a pinned wasm-bindgen-cli.
- There is no version (`argus --version` is an unknown flag), no `argus new`, and no installer.

## 0. Versions, and what a release holds

- `argus version`, and the version in `session.status`, ARGUS.md, a release's `build.json`, and marks and reports (they carry the code revision already).
- One version for the engine, the CLI, the editor, and the runtimes. A release is a version tag and an archive per platform with a manifest listing every file and its SHA-256.

**Built.** `argus version` (also `--version`, `-V`, `--json`) says the workspace's version, the revision, and the platform: `argus 0.0.1 (dev, windows x86_64)` from a checkout, the commit when `tools/release/stage.sh` built it (`ARGUS_REVISION`, with `-dirty` for uncommitted changes). The same `engine` object is in `session.status`; ARGUS.md's first line names it; `build.json` has it; saves, reports, and marks record the engine's version beside the game's code revision. The staged install's `manifest.json` lists every file with its size and SHA-256. Tags and archives come with item 5.

## 1. Game runtimes in the install

- A prebuilt game runtime for each platform: `argus-game` with every module but Steam, and a Steam variant. A game without Steam must not need Steam's library.
- The prebuilt web player: the wasm module, its glue, and the page.
- `argus build` puts a game's packs beside the right runtime, with no compiler. The source path stays for engine work, behind a flag or `ARGUS_ENGINE`.
- The checks it makes now (the build plays and looks as the project does) run the same way.
- A release for the other platform (Linux from Windows) takes that platform's runtime from the install, so an install carries the runtimes of every supported platform, or fetches them on demand.
- Plugins: the runtime loads a game's plugin libraries as the daemon does, built with the installed SDK. Rust is needed for those alone, and `argus build` says so.

**Built.** One game executable serves any game: both runtimes (`argus-game` and the web module) now take the game's code revision from its packs, not from a value compiled in, and every engine module but Steam checks at run time whether the game's `argus.toml` asks for it, so a runtime with the rigid-body module compiled in plays a game without it unchanged. An install's `runtime/` (beside `bin/`, or `$ARGUS_RUNTIME`) holds `<os>/standard/` (every module but Steam), `<os>/steam/` (with Steam's library beside it), and `web/`, each with a `runtime.json` (version, revision, variant, modules) that `argus build` checks against itself: a runtime from another version is refused with a reinstall hint, a game that names a module the runtime lacks is refused, and a missing variant says which ones are there (`crates/argus-cli/src/runtime.rs`). From an install, `argus build` compiles nothing for the game; from a checkout, with `--from-source`, or with `$ARGUS_ENGINE`, it compiles as before. A release for the other desktop platform is made from that platform's runtime, and its executable is not run here (the packs' check stands for it, and `build.json` says so); a game with Rust plugins is released on its own platform. `tools/release/stage.sh` builds an install for this platform (the programs, both runtimes, the web runtime, the manifest), and `tools/release/check.sh` releases every example from it with cargo off the PATH, Swarm included: without cargo it must say that its plugin needs Rust, and with cargo it is released. Fetching the other platform's runtimes is item 5's.

## 2. `argus new`, and a project an agent can start in

- `argus new <dir> [--template blank|platformer|roguelike|arena|colony]`: a working project from a template (the four games trimmed to a start), its first scenario passing.
- Each new project gets its ARGUS.md, and an `AGENTS.md` and a `CLAUDE.md` that point at it and at `argus docs`, so any agent knows where to read first.

**Built, with the blank template.** `argus new <dir> [--template t] [--name n] [--no-git]` copies a template from the install's `templates/` (or `$ARGUS_TEMPLATES`, or the checkout's), names the game, makes it a git repository unless it is inside one, and runs its scenarios in a daemon in its own process: it says done only when they pass, and no daemon is left running. Then it writes ARGUS.md from that daemon, an `AGENTS.md` (what Argus is, read ARGUS.md first, `argus docs`, where things live, `argus test` before done, `argus build`), and a `CLAUDE.md` that imports it (`@AGENTS.md`). Five templates, each under 35 KB, with no plugins and no modules:

| Template | From | The start | Scenarios |
|---|---|---|---|
| blank | | a square that walks over a floor | 1 |
| platformer | Hollow | run, jump a pit and spikes, three coins, climb a ledge to a flag; the pit and spikes send the hero back | 4 |
| roguelike | Delve | a turn a key press on a hand-drawn map, rats and goblins that chase by the hero's flow field and bite, bump to attack, death and a new run | 4 (8 runs) |
| arena | Swarm | walk and auto-fire, waves from the edges that chase (in Luau: Swarm's own chase is a plugin), touch damage with grace, game over and restart | 4 (8 runs) |
| colony | Outpost | four colonists take jobs over the navigation grid (berries, wood, hauling, eating), a day a minute, a pause and three speeds | 4 (8 runs) |

Four AI agents (Opus 5.5) wrote the game templates in parallel from the games, each in its own folder with the game read-only, and each broke its own rules in a copy to see its scenarios fail. `argus-cli`'s `new` test makes a project from every template and runs its scenarios, so a template that stops working fails CI. **The gaps the agents met, fixed (2026-10-09).** All four named the same missing piece: nothing in `argus docs` described the files a project is made of. A fifth agent wrote that reference (`crates/argus-daemon/src/formats.rs`): every file a project can have (argus.toml, scenes, rooms, entity files and their children, prefabs, input.toml, defs, dialogue, timelines, strings, ui.toml, audio.toml, platform.toml, every asset sidecar, tilemaps and Tiled files, navgrids, mods, plugins, settings.toml, template.toml), each key with its type, default, and meaning, and the engine's components with their fields and which systems write and read them, generated from an empty game assembled with every module. It is in `argus docs` and in every ARGUS.md, and 14 tests fail when a parser takes a key the reference does not name. The rest:

| Gap | Now |
|---|---|
| A scene's child with a Transform drew in world space without a GlobalTransform | the scene loader gives it one (`tests/parent_global.rs`); the Parent doc says when a script must |
| Closing Delve's menu with its own key spent a turn | the press that opens or closes the menu is frozen too (`menu_key.luau`, which fails on the old code) |
| `observe` took no `path`, and its default disagreed with `obs.frame`'s | `path` passed on; both docs say both defaults, and where the picture goes |
| `run`'s `max_ticks`, `events:first()`, `run_until` that misses | the docs say where `max_ticks` goes, that `first()` is the event row (data under `.fields`), and that a miss returns `hit = false` without failing |
| `q:within`/`q:nearest` saw last tick's positions; `q.time` ran while frozen; `q:each`'s handles are reused; how turns pass; vec2 defaults; events name a despawned entity `e:N` | each in the scripting API, with what to do |
| A NavAgent with no goal walks to (0, 0) | its doc says so and what to set |
| A sweep's failing candidate had an empty reason | it names the failed expectations |
| ARGUS.md said "rooms ." for a scene without rooms | no rooms, no line |
| Found writing the reference: release packs carried the plugins' Rust source; `chunk = 32.0` became 64 and `tile_size = 0` was taken; a non-string input `type` became a button; `pointer` missing from the input errors; "axis1" in input.bind's doc; UiRoot's doc named argus.toml for ui.toml | plugins/ left out of packs; whole numbers or an error, `tile_size` more than 0 (`tests/numbers.rs`); the type refused; the messages and docs corrected |

Left for later, each needing a closer look: the semantic frame lists a described entity whose sprite is hidden; events name a despawned authored entity by its handle (a fix that remembers ids must be bounded, since a scene reload despawns every authored entity); the clear colour is fixed; `pixels_per_unit` has no effect; a sound sidecar beside its own `.wav` says there is nothing to play; the dialogue box wants a `button` colour ui.toml does not define by default; and argus-player built without the rigid module trips three clippy lints (the workspace build has it).

## 3. MCP for the local clients

- `argus mcp setup <client>`, with `--project` and `--scope project|user`, for claude-code, claude-desktop, cursor, vscode, codex, and gemini. It writes that client's own file and key, merging with what is there, idempotent, with a backup:

  | Client | Project file | User file | Key |
  |---|---|---|---|
  | Claude Code | `.mcp.json` | `~/.claude.json` (or `claude mcp add`) | `mcpServers` |
  | Claude Desktop | (none) | `claude_desktop_config.json` | `mcpServers` |
  | Cursor | `.cursor/mcp.json` | `~/.cursor/mcp.json` | `mcpServers` |
  | VS Code | `.vscode/mcp.json` | the profile's `mcp.json` | `servers` |
  | Codex CLI | `.codex/config.toml` | `~/.codex/config.toml` | `[mcp_servers.argus]` |
  | Gemini CLI | `.gemini/settings.json` | `~/.gemini/settings.json` | `mcpServers` |

- `argus mcp setup` with no client lists the ones it finds installed. Each file's location is checked against the client's own docs when the item is built, since these move.
- A project-free MCP mode for the user-level clients: `argus mcp` with no project offers the projects it knows (recent, or under a folder), opens one, and makes a new one, so one global entry serves every project.

**Built.** Each client's file was checked against its docs on 2026-10-09 (the table above held, with two additions: Claude Desktop installed from the Microsoft Store reads a copy inside its package folder, which setup writes too, and VS Code now also reads a "portable" `.mcp.json`, which is newer than `.vscode/mcp.json`, so setup keeps to the latter, which every VS Code with MCP reads). `argus mcp setup <client>` edits the file as text, putting in or replacing the one `argus` entry and leaving the rest as it was, its order, spacing, and comments; a second run changes nothing; a changed file is kept beside it as `<file>.argus-backup`. The command is this program's own path, since an agent host may not start from a shell. A project entry opens that project; a user entry opens none unless `--project` names one, and Claude Code's user entry goes through `claude mcp add-json` when `claude` is here. A Gemini project also gets a `GEMINI.md` that imports AGENTS.md. With no project, `argus mcp` starts on none, and three more tools (beside the engine's) list the projects (`projects`: the open one, the recent ones, the templates, those under a folder), open one (`open_project`), or make one from a template and open it (`new_project`); the engine's tools then work on it. A project is remembered when any `argus` command connects to it or `argus new` makes it, in Argus's own data folder (`projects.json`), unless it is in the temp folder. `argus projects` lists them from a shell. Still to check: Claude Code connected for real (the gate).

## 4. Claude Desktop in one click

- `argus.mcpb`, an MCP Bundle (manifest_version 0.3, server type `binary`), built with each Windows release. It asks for the project folder (a `directory` setting) and runs the bundled `argus mcp`. Double-clicking it installs it.

**Built; installing it in Claude Desktop is the gate's.** `python tools/release/mcpb.py` makes `argus-<version>.mcpb` from a staged install, to the MCPB schema as published on 2026-10-09: `bin/argus.exe` and `templates/` (14 MB; the runtimes stay out, since no MCP tool builds a release), a manifest with the server's own tools (asked of the staged argus's `tools/list`), and an optional `directory` setting for the project. Left empty, it reaches the server as `--project ""`, which now means no project, so the server starts on none and offers them. Run as Claude Desktop runs it (unpacked, its `${__dirname}` and `${user_config.project}` filled in), it started on no project, made a game with `new_project` from its own templates (the scenario passing), and answered `query` on it.

Double-clicking it does nothing on this PC (Kevin, 2026-10-09). Claude Desktop here is the Microsoft Store build (2.31226), whose package declares no file types (only the `claude:` link); `.mcpb` opens with its command alias because "Open with" was once pointed at it, and the running app logs `second-instance: suppressing duplicate argv` and drops the path. The way in is Settings, Extensions, Advanced settings, Extension Developer, Install Extension…; `docs/install.md` says so. No policy blocks extensions here. Installed that way it worked (2026-10-09): Claude Desktop unpacked it to `Claude Extensions\local.mcpb.kevin.argus`, ran the server once to check it, then connected (protocol 2025-06-18, 17 tools), the server starting on no project. Then a chat in Claude Desktop made a project through it with `new_project`, its scenarios passing (Kevin, 2026-10-09): the gate's Claude Desktop row is met.

## 5. The release pipeline

- `tools/release/`: builds the Windows archive here and the Linux one on klab, checks them (each runtime builds and runs a game; the editor starts headless), writes the manifest and checksums, and makes a GitHub release on the private repository with `gh`.
- klab as a second self-hosted runner, so a release is one workflow. Installing the runner is a download, so it is Kevin's to allow.

**Built, and the first release made (a draft, 2026-10-09).** `tools/release/release.sh` makes a release from this machine with klab over ssh, so klab needs no GitHub access and no runner (Kevin chose that over installing one): it stages and checks the Windows install here, sends the commit to klab as a git bundle (`~/argus-release`), stages and checks the Linux install there, gives each install the other's runtimes and the web one (rewriting the manifests, `tools/release/manifest.sh`), and writes the Windows zip, the Linux tar.gz (`tools/release/tarball.py`, the modes set by path), the Claude Desktop bundle, the two installers, and `SHA256SUMS` to `target/release/v<version>/`; with `--publish`, a draft GitHub release on the private repository, which a person publishes.

v0.0.1 is a draft at 82889e0 with all six files. Its checks found three things first, each fixed before anything was published:

| Found | Fix |
|---|---|
| Hollow's and Outpost's Linux builds did not start from their folders: Linux does not look beside a program for `libsteam_api.so` | `argus-game` links with `-rpath $ORIGIN` on Linux (271cb4d) |
| Hollow's Linux build drew 6 of 230,400 pixels differently: a row of the feather's quill on a texel boundary, which klab's GPU rounds the other way | the release check forgives an edge moved by a pixel, not a changed colour (`Image::compare_near`, 82889e0); image goldens stay strict |
| The Linux archive, put together on Windows, had lost its execute bits, and `install.sh` refused it | `tarball.py` (e24a75d); the draft's archive replaced, then installed on klab into a scratch folder: new, test, build from the installed runtime, uninstall |

v0.0.2 (5003d91) was drafted with no fault found, its six files checked against `SHA256SUMS` and the Linux archive's programs executable. From 0.0.3 a release also carries the four example games for people to play with nothing installed (2026-10-10, for the people Kevin means to invite): `check.sh --keep` keeps the releases it already builds, and `release.sh` packs them as `example-games-<version>` for Windows and Linux with a `README.txt` of the controls (`tools/release/games-readme.txt`). The name does not start with `argus-`, which both installers look for (0.0.2's too, which `self-update` runs).

0.0.3's first run stopped on klab: Outpost's Linux build played as the project did, but 5 of 230,400 pixels of its picture differed, the same on every run. Both pictures come from the same renderer, one from the project's sprites and one from the packs' pages: two highlight texels of the new ground (a grass one and a dirt one) came out as their neighbours' colour from the packs. The ground is on a lossless page (only a 56 x 20 page of three sprites is BC7), so it is most likely the texel rounding Hollow's quill met, between a sprite's own image and its place on a page, here two texels from a highlight's edge. (The commit, 24e74e7, blames BC7; that was a guess before the build's report was read.) The check now forgives specks: groups of at most 4 differing pixels, at most one pixel in 10,000 in all (`Comparison::groups`, `SPECK` in `crates/argus-cli/src/build.rs`), and says so ("drawn the same but for 5 pixels in specks"). A sprite gone or a colour changed is a group bigger than that, and still fails.

## 6. Installing and updating

- `install.ps1` and `install.sh`: fetch the latest release from the private repository with a GitHub token (`gh auth token`, or `GITHUB_TOKEN`), check the checksums, install for the user alone (`%LOCALAPPDATA%\Argus`, `~/.local/share/argus` with `~/.local/bin`), and put `argus` on the PATH.
- `argus self-update` (the same, in place, keeping the old version until the new one starts) and `argus uninstall`.
- What a machine needs, said up front: a GPU driver with DX12 or Vulkan, or the software rasteriser (WARP is in Windows; Linux needs Mesa's lavapipe). On Linux, ALSA and X11 or Wayland libraries. Kept as few as the build allows.

**Built; the download and the PATH are checked on clean machines (item 7).** `tools/release/install.ps1` and `install.sh` install from a release (the GitHub API with a token, the archive checked against `SHA256SUMS` before anything moves) or from an archive already here (`-Archive`, `--archive`). They unpack beside the install, check the new `argus version` starts, swap `app` for it (the old one is put back if it does not start, and removed once it does), stop daemons running from the old one, and put `app\bin` on the user PATH (`~/.local/bin` links on Linux), writing the PATH only to add or remove that one entry. The staged install carries its installer, and `argus self-update [version]` and `argus uninstall` run it: on Windows the installer waits for this argus to exit, since a running program holds its folder; a checkout's argus says it updates with git pull instead. Tried here on a staged archive in a scratch folder: install, then with only the system folders on the PATH (no git, no cargo), `argus new`, `argus test`, and `argus build` from the installed runtime, the release checked and run; an upgrade over it; `argus uninstall` (gone in 1.5 s, the project kept, the user PATH unchanged).

## 7. Clean machines

- Install on machines that have never built Argus: a fresh Windows user profile here, and WSL's Ubuntu on this PC (which has no Rust). From there: `argus new`, the tests, an agent's session, `argus build`, and the built game running.
- klab as an installed (not built) Linux machine too.

**Skipped for now (Kevin, 2026-10-09); then, for the people Kevin means to invite, a fresh user's environment on each platform (2026-10-10).** Not new machines: a home folder of its own and nothing of this machine's on the PATH. On klab, v0.0.2's Linux archive installed into a throwaway home with `env -i` (PATH only the system's: no cargo, rustc, or gh), then `argus new --template platformer` (4 of 4 runs), `argus test`, `argus build` for Linux and the web (both checked), `argus mcp setup claude-code --scope user` (written into that home), and `argus mcp` answering `initialize` and `tools/list` (20 tools). The only noise was libwayland's "XDG_RUNTIME_DIR is invalid or not set" during the builds' checks, from `env -i` taking that variable away; a login has it. Here, `install.ps1 -Archive -NoPath` into a scratch folder, with USERPROFILE, APPDATA, and LOCALAPPDATA inside it and the PATH only Windows' own folders (no cargo, rustc, gcc, or gh): `argus new --template arena` (8 of 8), the tests, a Windows and a web build (both checked, so the release's own program ran headless with that PATH), `mcp setup` for Claude Code and Claude Desktop (both written inside the scratch home, so nothing of MinGW's is needed at run time), and `argus mcp` with 20 tools. Both scratch homes were removed. A fresh Windows profile, WSL, and an agent's session on a clean machine are still to do.

## 8. Remote MCP for the web apps

- MCP over Streamable HTTP from the daemon's web listener (phase 5 item 10), with the scoped tokens. Local clients that speak HTTP (Claude Code, VS Code, Codex) can use it over the tailnet as is.
- claude.ai and ChatGPT connect from their own cloud, not from the user's machine, so a server they use must be reachable from the public internet. That means a tunnel (Tailscale Funnel is the natural one here), and authorization they support: OAuth 2.1 with dynamic client registration and PKCE, found through protected-resource metadata (RFC 9728).
- Item 10 kept the daemon to this machine and the tailnet on purpose, so opening it to the internet is Kevin's decision, taken at this item, with what it exposes written down.

## 9. Docs for someone new

- `docs/install.md`: installing, the first project, connecting each client, releasing a game.
- The README's top says what Argus is and how to install it, before the build history.

**Written.** `docs/install.md`: what a machine needs, installing (from a release with the GitHub CLI, or from an archive), a first game and the templates, connecting each AI tool (with what to do next in each), releasing a game, updating and uninstalling, and what to do when something is wrong. The README now starts with how to use it, and its history has a Distribution section. Both describe the release commands as they will be once the first release exists (item 5).

**Then a front page and a manual (Kevin, 2026-10-09).** The README is a front page now: the four games' pictures, what Argus is and what it can do, a short install, connecting an AI, and links on; its build history moved whole to `docs/history.md` and its notes for engine work to `docs/developing.md`. `docs/manual/` is the guide the reference never was, fourteen chapters (concepts, a first game end to end, scripting, scenes, gameplay, presentation, content, testing, playtesting, tuning and performance, agents, releasing, the reference's map), three of the four groups written by Opus agents that ran what they wrote on scratch copies.

Kevin asked whether what ships with the MCP server is enough for an AI to understand Argus. It was not: an agent over MCP alone (Claude Desktop) got the server's instructions and the tool descriptions, about 11 KB, and was told to read ARGUS.md, which it had no way to open; and it had no way to write a script. Now:

- The manual is compiled into `argus`, and the `docs` tool (and `argus docs <topic>`, `argus docs list`, `find=`, `section=`) gives any chapter, any part of the reference, or the open game's ARGUS.md, matched to the installed version, with no network. The server's instructions say to read `docs topic=agents` first.
- `read_file` and `write_file` (Kevin's choice: inside the project only) reach the project's files for an agent with no file system. A write is an edit (`edit.file`): in the change feed with its author and note, and `edit undo=true` takes it back. Over MCP alone, a test made a game, wrote a system, reloaded it, saw it run, and undid it.
- `AGENTS.md` (written by `argus new`) points at the manual too.

Writing the manual found and fixed, most important first: scripts could read `os` (a clock, the date) and an unseeded `math.random`, so a seek or a replay could differ from the run (both gone; `math.random` names `q:rng()`); release packs carried the project's README, ARGUS.md, and agent files to players; the Linux build needs glibc 2.39 (Ubuntu 24.04), not 22.04 as `docs/install.md` said; `q:within` and `q:nearest` refused the entity the reference said they took; `q:save` and `q:load` were missing from the scripting reference; `$ARGUS_AUTHOR` did not name the author of an edit from a shell; the plugin SDK's own docs gave a path into the engine's checkout; `argus help editor` and `argus help version` had nothing; `--web` was missing from `argus help daemon`; the docs said `reload` takes hand-edited entity files (it is `scene load`).

Left for later: a script's file-level `local` keeps state a seek does not restore (the manual says to keep state in components; detecting it is not built); the semantic frame lists a described entity whose sprite is hidden; events name a despawned entity by its handle; a mark dropped from the shell has two authors; the room scenes' ARGUS.md counts only loaded rooms; scene issues show the first problem per file; `asset move` leaves `@id`s in Luau; the engine's dialogue box cannot be replaced; Describe text is not translated; a script cannot stop a non-spatial sound loop; `particles.burst` and the shader inputs are undocumented; the serve script for web reports is not in the install; the Linux build could be made on an older Ubuntu to reach older machines.

Fixed since (2026-10-10), for the people Kevin means to invite: an f32 written back to a file (an undo, a tuning session's end, a sidecar value) is its shortest text, `0.36`; `t:run_until` refuses a key it does not take, naming it (`speed = "max"` is taken, and changes nothing); a script's `map:get` reads the map in memory instead of looking at its file's time each call (800 cells: 12.3 ms before, 29 us after, a release build); a reload's message and its hint are two sentences; and a `Background` on the drawing camera sets the colour behind everything, which was a fixed dark grey (Delve's band, every template's border). The art passes drew Swarm's HUD as UI, so its players see the wave and HP, and found that Outpost's HUD hour was the share of the day times 24 (dawn read 00:00, night fell at 15:36); it now reads dawn as 05:00 and nightfall as 19:00. Scripts spawn from prefabs (GitHub issue 1, Kevin asked for it 2026-10-10): `q:spawn_prefab(name, fields)` with a system's `spawns` naming the prefabs it may, which grants their components; the arena template spawns its enemies and bullets that way, the same run to the tick as before.

Then the art passes' engine findings (Kevin asked for all of them, 2026-10-11):

- A sprite repeats its frame: `Sprite.repeat = [14, 7]` draws a floor of 14 by 7 tiles in one sprite, normal map and all (the vertex shader runs the uv on past the frame, `tiled_uv` brings it back in; branch-free, since WARP lost its device when a branch came before the SDF text material's derivatives).
- A `UiPanel` takes a `frame` of its image, so a panel can be one frame of a sheet.
- A system declares `before`, for a system it cannot name itself in (an engine one). `argus script systems` now shows `before`, `first`, and `last`. The finding's example was already ordered: `particles.update` is `last` in its stage, so a late look system's burst lands the same tick; the listing did not say so.
- Animator conditions take `or` (`and` binds tighter), and `face = "<field>"` mirrors the sprite while the field is below zero, keeping its facing at zero.
- Flash, fade, and vignette amounts are shares of what a person sees: the passes mix in sRGB, not linear light. The examples' and templates' values were fitted to their own frames to look as before (Hollow's flash 0.45 became 0.68 and the platformer's 0.45 became 0.42, the same number having looked different on a dark cave and a bright sky; Swarm's 0.015 became 0.11, the arena's 0.035 became 0.16; vignettes 0.25 to 0.65 became 0.13 to 0.42 in Delve, 0.42 became 0.24 in Swarm, 0.32 became 0.17 in Outpost).
- The "custom material over a higher-z sheet" was not a z-order fault: measured, the colony's material trees were darkened by the night sheet exactly as everything else was (white to 172 either way); the untinted art was only brighter than the tinted trees beside it. What made the sheet look weak is that see-through colours blend in linear light (black at half alpha leaves white at 188 of 255). Sprite blending stays linear, as the lighting needs; the manual says so and points a dimming at `PostFade`. A test now pins z order across materials.

## The gate

- On a clean Windows profile and on WSL's Ubuntu, with no Rust:
  - install from a release on the private repository with a token;
  - `argus new`;
  - `argus mcp setup` for each local client (each file checked, and Claude Code connected for real);
  - an agent adds a feature through MCP;
  - `argus build` makes a release of it, which runs.
- `argus.mcpb` installs into Claude Desktop and opens a project.
- A claude.ai connector reaches a daemon through the chosen tunnel and its authorization, or this is recorded as not done, with why.
- Every earlier gate's tests still pass.
