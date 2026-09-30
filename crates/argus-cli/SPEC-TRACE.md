# argus-cli: spec trace

Every statement in the design document (v0.3) that constrains the `argus` command (§2 clients, §5 "the method set is small", §16 "one schema", §9 and §10 mentions, §18 item 9), with where it lives and the test that pins it. Statuses as in the other traces: **Done**, **Partial**, **Deferred**.

## §2, §5 A client of the protocol

| Spec | Status | Where | Test |
|---|---|---|---|
| The editor, the CLI, the test runner, and the coding agent are all clients of the same daemon. There is no button that isn't also a command; the editor cannot do anything the CLI can't. | Done | The CLI is a thin client: `client.rs` speaks JSON-RPC 2.0 over the local socket that `argus_daemon::server::connect` opens; nothing in this crate touches a world. | `cli::the_cli_talks_to_a_daemon_over_its_socket` |
| The method set is mirrored one-to-one by CLI subcommands. | Done | `resolve` maps words to a method by joining them with dots and taking the longest match in `argus_daemon::METHODS`; `world query`, `world.query`, and `input log get` all resolve, and nothing resolves that is not in the table. The only non-method commands are `help`, `daemon start/stop/status/run`, which exist so a method has a process to reach, and `test`, which is `test.run` with the §9 report. | `cli::every_method_is_a_subcommand_and_help_comes_from_the_same_table` |
| The binary starts from any shell. | Done | `.cargo/config.toml` links the C and C++ runtimes into the binary on windows-gnu (`+crt-static`, and `CXXSTDLIB` for Luau's libstdc++), so it loads no MinGW DLL from PATH. Found by the phase-1 gate: from a shell whose PATH had an older `libwinpthread-1.dll` first (PostgreSQL ships one), `argus.exe` stopped at an "entry point not found" dialog. | `objdump -p target/debug/argus.exe` lists no `libstdc++`, `libgcc`, or `libwinpthread`; Hollow's tests pass run from the plain system PATH |
| Clients connect over a local socket; one process per project. | Done | `project.rs`: the project is `--project`, `$ARGUS_PROJECT`, or the nearest `argus.toml` upward; its daemon is found through `.argus/daemon.json`. A method call with no daemon answering starts one (`argus daemon run`, detached, logging to `.argus/daemon.log`) and waits for it, unless `--no-start`. | `cli::the_binary_finds_starts_and_stops_a_project_daemon` |
| Every response is a summary by default, paginated (P2). | Done | Nothing is added or removed on the way through: `--json` prints the daemon's result as one line; without it `render.rs` prints `{columns, rows}` results as an aligned table under their scalar fields (`total`, `next`, `source`) and everything else as pretty JSON. `next` is shown so the human can page with `cursor=`. | `cli::key_values_become_the_params_object`, `cli::the_cli_talks_to_a_daemon_over_its_socket` |
| The semantic frame is a screen-reader feed (§8, §12 accessibility; item 12). | Done | A result with `format: "semantic"` prints as sentences in the frame's order: the camera, the HUD, each entity with its name and text, the groups, the effects, the tile rows. | `cli::a_semantic_frame_reads_as_lines` |
| Server-initiated notifications for subscriptions. | Done | `--follow` (implied by `obs.subscribe`) keeps the connection open and prints each notification as a line: `event death #12 frame 40 {...}` or JSON with `--json`. | (manual; `Client::next_notification`) |
| Errors carry code, sentence, hint, `file:line`. | Done | `render::error` prints exactly those four on stderr and exits 1; `--json` prints the error object on stdout. Exit codes: 0 ok, 1 daemon error, 2 usage, 3 no daemon. | `cli::the_cli_talks_to_a_daemon_over_its_socket` |

## §16 One schema

| Spec | Status | Where | Test |
|---|---|---|---|
| Each tool's description is generated from the same schema the CLI help uses; the CLI mirrors the tool set exactly, so a human reading the agent's transcript can rerun any call. | Done | Phase 2 item 8: the schema is structured (`argus_daemon::docs`: each method's and each tool's parameters as `Param {name, ty, required, doc}`, and the dozen tools as `ToolDoc {name, doc, params, calls}`). `help.rs` builds every help page from it, `params::names` and `params::build_tool` read the declared order for bare values, `mcp.rs` gives the same `tool_description` and a JSON Schema made from the same parameters, and `argus <tool>` runs the same code the MCP server runs (`tools.rs`). An MCP answer starts with `argus <tool> --params '<args>'`, which reruns it from a shell and gives the same answer. | `cli::every_method_is_a_subcommand_and_help_comes_from_the_same_table`; `mcp::argus_mcp_serves_the_tools_of_the_one_schema` |
| `argus docs --for-agent` emits an ARGUS.md from the live daemon: the component schemas with doc strings, the system list with stages and write sets, the prefab catalog, and the current scene summaries. | Done | `agentdoc.rs`: from `session.status`, `world.summary` (each scene, the others from the index), `script.systems`, `world.prefabs` (new, phase 2 item 8), `world.defs`, and `world.describe`; the game's components before the engine's, with fields, defaults, ranges, tune flags, docs, and who writes and reads each. Written to `<project>/ARGUS.md` (or `--out`); the same project gives the same file. It opens with how to work with the project and the tools. | `mcp::argus_docs_writes_argus_md_from_the_live_daemon_and_the_reference_from_the_schema` |
| The engine's own reference documentation ships in the same shape. | Done | `argus docs` (without `--for-agent`) prints the tools and every method with their parameter tables, from the same schema. | same |
| `argus mcp` starts a stdio MCP server exposing about a dozen tools: summary, query, describe, edit, run, seek, observe, events, test, reload, marks, propose. | Done | `mcp.rs`: MCP over stdin and stdout, newline-delimited JSON-RPC 2.0 (`initialize` with protocol 2025-06-18, 2025-03-26, or 2024-11-05; `tools/list`; `tools/call`; `ping`); it finds or starts the project's daemon like every command. The twelve tools are `docs::TOOLS`; the five that only read say so (`readOnlyHint`). A tool's failure is an answer with `isError`, and a lost daemon is found again on the next call. `observe` with `format: "png"` returns the picture as an image. | `mcp::argus_mcp_serves_the_tools_of_the_one_schema` |
| (§5) The MCP shim runs `world.summary` on connect. | Done | `initialize` runs the summary tool and puts it in the server's instructions, after a paragraph on the tools, so the agent starts oriented. | same |
| `argus token` for remote access. | Deferred | Decided with Kevin at phase 4's start (docs/phase4-checklist.md): tokens, TLS, and a phone page for review requests wait for the first remote client; the editor connects on the local socket. | |
| `argus build --release`. | Done | Phase 2 item 7; see its section below. | |
| `argus test`. | Done | `argus test [name] [--seeds 1..50] [--file tests/x.luau] [--update] [--json]` calls `test.run` and prints the compact report (`render::test_report`): PASS/FAIL per scenario, a line per failing seed and expectation, a summary; exit 0 only when every run passed. `argus test run` still reaches the method directly. | `cli::the_cli_talks_to_a_daemon_over_its_socket` |

## §9, §10 Mentions

| Spec | Status | Where | Test |
|---|---|---|---|
| "show as command" prints the CLI equivalent of every editor action. | Done (phase 4 item 0) | `argus_client::command::line` writes any call in this CLI's grammar, and the words reader moved to `argus-client` beside it, so the two directions are one crate's; every feed row carries its line. A session's feed typed into this CLI against a fresh copy makes the same world. `argus editor` opens the editor (`project::editor_command`). | `feed::every_change_is_a_command_the_agent_could_have_issued`; `argus-client` `command` tests |
| The agent's `input.inject` says `{action: "jump"}`. | Done | `argus input inject jump` (a bare value goes to the first parameter, `action`). | `cli::key_values_become_the_params_object` |

## Parameters, stated so the grammar is a decision

- `key=value` words. A value is JSON when it parses as JSON (`2`, `true`, `"text"`, `[1, 2]`), else a string; bracketed values accept bare words (`[Enemy, @player]`, `{of: @player, r: 6}`). Dotted keys nest (`where.has=[Enemy]`); a repeated key becomes a list; `--params '{...}'` or `--params @file.json` supplies a whole object that the words layer on top of.
- One bare word goes to the method's first declared parameter (`argus world get @player`, `argus sim step 5`, `argus scene load delve`), read from the docs `params` text. A second bare word goes to the second parameter; more than the method declares is a usage error naming the method.
- A key that must stay a string when it looks like a number is quoted: `id='"42"'`.

## §18 Build plan

Phase 0 item 9, "CLI": this crate, plus `argus_daemon::launch` (shared with `argusd`) and `session.shutdown` in the daemon so `argus daemon stop` is a method call like everything else.

## Phase 2, item 7: argus build

| Spec | Status | Where | Test |
|---|---|---|---|
| (§15) `argus build --release --target windows` produces a single directory. | Done | `build.rs`: `argus build [--target windows|linux|macos|web] [--release] [--out dir] [--since dir]`, a local command like `daemon` (a release needs no running game). Packs from `argus_player::pack::build`; the executable is `argus-game` built with cargo from the engine's tree (`ARGUS_ENGINE`), `--release` in the `ship` profile (optimised, no debug info), copied as the game's name; the web target builds the wasm module with wasi-sdk's toolchain and wasm-bindgen and writes the page and `data/packs.json`. `--since` makes a patch beside the old packs. A desktop build runs on its own platform. The build checks itself: the game from its packs, and the executable run headless, reach the project's world after 300 ticks, or the build fails. `build.json` records it. | the builds of Hollow for Windows (debug and release) and the web |

## Phase 3, item 1

| Spec | Status | Where | Test |
|---|---|---|---|
| (§8 L4) The semantic frame's UI, read aloud. | Done | `render.rs`: `UI <id> button, focused: <words> at <bounds>` lines, in the frame's order. | checked by hand on Hollow's and Delve's menus |
| (Phase 2 item 7) `argus build --target web` builds on this machine. | Done | The wasm build's `CARGO_TARGET_DIR` goes to cargo without Windows' `\\?\` prefix: a build script links with MinGW's `ld`, which cannot open such a path. It showed with the first new build script since the web target came (`libm`, through rustybuzz). | the web build |

## Phase 3, item 2

| Spec | Status | Where | Test |
|---|---|---|---|
| (§16) The tools mirror the protocol, a dozen of them. | Done | `query` takes `path: {from, to, grid?}` and answers with `nav.path`: `argus query path.from=@player path.to=[10,10]`. The `nav.*` methods are the namespace's own subcommands. | `mcp::argus_mcp_serves_the_tools_of_the_one_schema`, `schema::the_tools_call_methods_that_exist` |

## Phase 3, item 3

| Spec | Status | Where | Test |
|---|---|---|---|
| (§8, §16) Aggregates through the `query` tool and the CLI. | Done | `query` passes `group_by`; a bare word keeps a call glued to it whole, so `argus query select=[count(), sum(Stockpile.food)] group_by=Job.kind` reads as written (the brackets broke on the parentheses). | `cli::key_values_become_the_params_object` |
| (§9) `argus test` runs what it is asked for. | Done | `argus test` takes `name=`, `file=`, `seeds=` (and the flags) as `key=value` too, and refuses a key it does not know. It dropped them before, and a dropped filter ran every scenario: six minutes of Outpost's seeds for one scenario. | `cli::key_values_become_the_params_object` (the refusal); by hand, a filtered run |

## Phase 3, item 4

| Spec | Status | Where | Test |
|---|---|---|---|
| (§16) The dozen tools reach strings. | Done | `query` takes `strings` (a language's code, or `{lang, prefix, missing}`) and answers with `strings.list`; `edit`'s ops take `text {lang, key, text}`. `argus strings list lang=fr missing=true` from the command line. | `schema::the_tools_call_methods_that_exist` |

## Phase 3, item 5

| Spec | Status | Where | Test |
|---|---|---|---|
| (§16) The dozen tools reach timelines. | Done | `query` takes `timelines` (true, or a name) and answers with `timeline.list`; `seek` takes `timeline` and `time` and scrubs with `timeline.seek`. `argus seek timeline=outro time=1.5` from the command line. | `schema::the_tools_call_methods_that_exist` |

## Phase 3, item 8: mods

| Spec | Status | Where | Test |
|---|---|---|---|
| (§15) Mod packs. | Done | `argus build --mod <id>`: the mod's pack in `dist/mods/` (or `--out`), after checking that it loads with the game (the mods it requires first), nothing left out, and that the game plays the same with the mod from its pack as from its directory. | Hollow's `lanterns` and `brighter` built and checked |
| (§16) About a dozen tools. | Done | No new tool: `query {mods: true}` reads `mods.list`, `reload {mods: [...]}` loads the scene again with those mods. | `tools.rs` |

## Phase 4, item 6: the profile tool

| Spec | Status | Where | Test |
|---|---|---|---|
| (§5, §16) The agent enforces §17: "a perf regression is a query result with a system name on it". | Done | The `profile` tool: §17's targets beside what is measured (`prof.targets`) by default; `systems: true` (`prof.systems` and `prof.alloc`), `worst: n`, `calls: true`, `capture: n`. `argus profile`, `argus prof targets` from the command line. | `schema::the_tools_call_methods_that_exist`, `mcp` |

## Phase 4, item 5: the scrubber's calls

| Spec | Status | Where | Test |
|---|---|---|---|
| (§16) The run's range and the saved replays, for the agent. | Done | The `seek` tool: no frame reads `sim.ring`; `replays: true` calls `replay.list`, `replay: path` calls `replay.open`. `argus sim ring`, `argus replay list`, and `argus seek replays=true` from the command line. | `schema::the_tools_call_methods_that_exist` |

## Phase 4, item 4: review requests

| Spec | Status | Where | Test |
|---|---|---|---|
| (§10) The agent asks for judgement. | Done | The `review` tool: `question` (with `frames`, `options`, `note`) calls `obs.review`; `list` (or no question) calls `review.list`, answered ones too. `argus obs review question=...` and `argus review list all=true` from the command line. Thirteen tools. | `schema::the_tools_call_methods_that_exist`, `mcp` (the tool count) |

## Known limitations, stated so they are decisions

- A daemon started by the CLI outlives the shell; `argus daemon stop` ends it, and `.argus/daemon.json` is removed on a clean exit. `stop` returns once the daemon is gone (its record removed and its socket silent), not when it agreed to go: a command right after it used to reach the daemon on its way out and fail with "connection lost" (found after the phase-1 gate; `cli::the_binary_finds_starts_and_stops_a_project_daemon`). A daemon killed otherwise leaves a stale record, which `argus daemon status` reports and a method call replaces.
- The socket name is derived from the project path, so two checkouts of one project at different paths get different daemons, and one path gets one daemon.
- Parameter types are JSON's (string, integer, number, boolean, array, object, any); what an array holds or an object's keys are is said in the parameter's doc, not in the schema. The MCP input schemas are as precise as that.
- `argus test` keeps its own flags (`--seeds`, `--file`, `--update`) as well as the tool's `key=value` form.
- No shell completion; the overview is the list.
