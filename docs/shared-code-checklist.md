# Shared code: checklist

Kevin, 2026-10-13: game systems cannot share code across files (GitHub issue 24). In Bring Your Own Game Master that shaped the whole game: the rules kernel that the hero's turn, the GM's ruling, and the monsters all need is one system file of about 3,500 lines; the base cell codes, tile tags, and condition names are written out again in the dungeon builder and the look; and each scenario file carries its own copy of the same test helpers. Art scripts can `require` a `*.lib.luau` module; game scripts cannot, and since 0.0.7 `require` in a game script is an error that says so.

Decided with Kevin, 2026-10-13: modules live in a `lib/` folder at the project's root, and a mod adds modules of its own but cannot replace the game's.

## 0. Modules for game scripts

- A **module** is a Luau file in `lib/` that returns a value, usually a table of functions and constants. `lib/rules.luau` is `require("rules")`, `lib/rules/checks.luau` is `require("rules/checks")`. It is outside `systems/`, whose every file is loaded as a declaration file.
- A module is evaluated once per load; every file that requires it gets the same value. The table it returns is frozen, so one system cannot change what another sees.
- It runs in the same sandbox as the systems, and its calls count against the calling system's instruction budget. It reads the world only through what it is handed: `rules.damage(q, e, n)`.
- A module's file-level locals are outside the run, as a system file's are: keep only constants and functions there.
- A missing module is an error naming `lib/<name>.luau` and the modules there; a module that requires itself through others is refused, naming the chain.

**Built (2026-10-13).** `crates/argus-luau/src/modules.rs`: the host's own `require`, installed beside the sandbox's globals (the 0.0.7 stub that refused it is gone). A module's value and every table in it is set read-only. A refusal starts with the file and line of the `require`, so `script.errors` points there: a missing module lists the modules there are, and a circle reads "modules cannot require each other in a circle: lib/a.luau, which requires lib/b.luau, which requires lib/a.luau". An error inside a module is recorded once, at the module, and the files above it say "lib/x.luau did not load" with that error, instead of nesting it at each level (the first version did, three deep, and named the wrong file). Tests: `crates/argus-luau/tests/modules.rs`.

## 1. Reload

- The host records which modules each file required, directly or through another module. A changed module is evaluated again, and every file that uses it is reloaded with it, even if that file did not change; the reload's answer lists them, and why.
- A module with an error keeps its users on their last good version, as a system file with an error keeps its own.

**Built (2026-10-13).** A reload compares each evaluated module's bytes with the file's; one that changed is dropped, with every module and file whose requires reach it, and those files load again whether they changed or not. `script.reload` answers `modules` (the modules that changed) beside `reloaded` (every file evaluated again). A module that no longer compiles is left evaluated, with its error in `script.errors`, so its users keep running on the last good value; once it compiles, they reload. `script.modules` lists every module with its leading comment and the files that use it.

## 2. Mods and builds

- A mod has its own `mods/<id>/lib/`, looked in first for that mod's files. A mod can require the game's modules, but not replace one: the game's files always get the game's.
- `argus build` compiles `lib/` into the packs as it does `systems/`; `tests/lib/` stays out with the rest of `tests/`.

**Built (2026-10-13).** A mod's file resolves `mods/<id>/lib/<name>.luau` first, then the game's; a game's file only the game's (`Layers::owner` says whose a file is). `argus build` compiles `lib/` with the other script folders, and `argus build --mod` a mod's own `lib/`; `tests/` stays out as before. Tests: the mod case in `crates/argus-luau/tests/modules.rs`, the builds in `crates/argus-player/tests/release_modules.rs`.

## 3. Shared code for scenarios

- A scenario's `require` looks in `tests/lib/` first, then in the game's `lib/`, so a test can use the game's constants and pure helpers instead of copying them. The files in `tests/lib/` are not scenarios.

**Built (2026-10-13).** argus-test installs its own `require` in the scenario VM (it does not depend on argus-luau): `tests/lib/<name>.luau`, then `lib/<name>.luau`, evaluated once for the suite, frozen, circles refused. The `max_ms` cut-off now fires in any Luau a scenario runs but the harness, so a loop in a module is cut off as one in the scenario's file is. Test: `a_scenario_requires_its_helpers_and_the_games_modules` in `crates/argus-test/tests/runner.rs`.

## 4. Docs

- The scripting chapter: a section on sharing code. The scripting and scenario references: `require`. ARGUS.md: the modules, each with its leading comment.

**Done (2026-10-13).** The scripting chapter has "Sharing code", with Outpost's clock as the example; "Where scripts live" has `lib/`; the sandbox's `require` line and the "cannot share code" line are rewritten; reload says a module's users reload with it. The testing chapter has `tests/lib/`. Both references have a `require` entry; the file formats' introduction and the mod entry name `lib/`; the guide `argus new` writes mentions it. ARGUS.md has a Modules section (module, file, used by, its comment) when a game has any.

## Gate

- Tests for each part: a module required from two systems and a scenario; a changed module reloading its users; a module error keeping them on their last version; a mod's own module and the game's; a release build carrying `lib/` and playing the same as the folder.
- One example uses a module the way a game would: shared constants and helpers that two or more of its systems wrote out each, with its scenarios passing on the same digests as before.
- Then Bring Your Own Game Master's own split (its kernel across modules, its scenario helpers in `tests/lib/`) is the game's to do.

**Gate met (2026-10-13).** Each part has its test: two systems sharing a module, frozen, reloading together and keeping their last version through a module's syntax error, a missing module and a circle, and a mod's own module beside the game's (`crates/argus-luau/tests/modules.rs`); `script.modules` and the reload's answer through the protocol (`crates/argus-daemon/tests/reload_faults.rs`); a scenario requiring `tests/lib/` and the game's `lib/` (`crates/argus-test/tests/runner.rs`); Outpost built into packs carrying `lib/` compiled, leaving `tests/lib/` out, and playing a day and a half at the same digest as from its folder, and a mod's `lib/` in its pack (`crates/argus-player/tests/release_modules.rs`). Outpost is the example: its HUD and inspector each worked out the hour, and its menu and inspector each had `spawn` and `button`, which are now `lib/clock.luau` and `lib/ui.luau`; five of its scenario files had their own `one`, `set`, or `press`, now `tests/lib/helpers.luau`. Its 24 scenarios, 58 runs, all pass at the same digest and tick as Outpost as committed before the change. A full CI waits for the release, after the remaining tickets (Kevin, 2026-10-13). Bring Your Own Game Master's own split is the game's.
