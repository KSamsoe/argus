# Shared code: checklist

Kevin, 2026-10-13: game systems cannot share code across files (GitHub issue 24). In Bring Your Own Game Master that shaped the whole game: the rules kernel that the hero's turn, the GM's ruling, and the monsters all need is one system file of about 3,500 lines; the base cell codes, tile tags, and condition names are written out again in the dungeon builder and the look; and each scenario file carries its own copy of the same test helpers. Art scripts can `require` a `*.lib.luau` module; game scripts cannot, and since 0.0.7 `require` in a game script is an error that says so.

Decided with Kevin, 2026-10-13: modules live in a `lib/` folder at the project's root, and a mod adds modules of its own but cannot replace the game's.

## 0. Modules for game scripts

- A **module** is a Luau file in `lib/` that returns a value, usually a table of functions and constants. `lib/rules.luau` is `require("rules")`, `lib/rules/checks.luau` is `require("rules/checks")`. It is outside `systems/`, whose every file is loaded as a declaration file.
- A module is evaluated once per load; every file that requires it gets the same value. The table it returns is frozen, so one system cannot change what another sees.
- It runs in the same sandbox as the systems, and its calls count against the calling system's instruction budget. It reads the world only through what it is handed: `rules.damage(q, e, n)`.
- A module's file-level locals are outside the run, as a system file's are: keep only constants and functions there.
- A missing module is an error naming `lib/<name>.luau` and the modules there; a module that requires itself through others is refused, naming the chain.

## 1. Reload

- The host records which modules each file required, directly or through another module. A changed module is evaluated again, and every file that uses it is reloaded with it, even if that file did not change; the reload's answer lists them, and why.
- A module with an error keeps its users on their last good version, as a system file with an error keeps its own.

## 2. Mods and builds

- A mod has its own `mods/<id>/lib/`, looked in first for that mod's files. A mod can require the game's modules, but not replace one: the game's files always get the game's.
- `argus build` compiles `lib/` into the packs as it does `systems/`; `tests/lib/` stays out with the rest of `tests/`.

## 3. Shared code for scenarios

- A scenario's `require` looks in `tests/lib/` first, then in the game's `lib/`, so a test can use the game's constants and pure helpers instead of copying them. `tests/lib/` is not read for scenarios.

## 4. Docs

- The scripting chapter: a section on sharing code. The scripting and scenario references: `require`. ARGUS.md: the modules, each with its leading comment.

## Gate

- Tests for each part: a module required from two systems and a scenario; a changed module reloading its users; a module error keeping them on their last version; a mod's own module and the game's; a release build carrying `lib/` and playing the same as the folder.
- One example uses a module the way a game would: shared constants and helpers that two or more of its systems wrote out each, with its scenarios passing on the same digests as before.
- Then Bring Your Own Game Master's own split (its kernel across modules, its scenario helpers in `tests/lib/`) is the game's to do.
