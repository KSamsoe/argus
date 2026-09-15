# argus-cli: spec trace

Every statement in the design document (v0.3) that constrains the `argus` command (§2 clients, §5 "the method set is small", §16 "one schema", §9 and §10 mentions, §18 item 9), with where it lives and the test that pins it. Statuses as in the other traces: **Done**, **Partial**, **Deferred**.

## §2, §5 A client of the protocol

| Spec | Status | Where | Test |
|---|---|---|---|
| The editor, the CLI, the test runner, and the coding agent are all clients of the same daemon. There is no button that isn't also a command; the editor cannot do anything the CLI can't. | Done | The CLI is a thin client: `client.rs` speaks JSON-RPC 2.0 over the local socket that `argus_daemon::server::connect` opens; nothing in this crate touches a world. | `cli::the_cli_talks_to_a_daemon_over_its_socket` |
| The method set is mirrored one-to-one by CLI subcommands. | Done | `resolve` maps words to a method by joining them with dots and taking the longest match in `argus_daemon::METHODS`; `world query`, `world.query`, and `input log get` all resolve, and nothing resolves that is not in the table. The only non-method commands are `help` and `daemon start/stop/status/run`, which exist so a method has a process to reach. | `cli::every_method_is_a_subcommand_and_help_comes_from_the_same_table` |
| Clients connect over a local socket; one process per project. | Done | `project.rs`: the project is `--project`, `$ARGUS_PROJECT`, or the nearest `argus.toml` upward; its daemon is found through `.argus/daemon.json`. A method call with no daemon answering starts one (`argus daemon run`, detached, logging to `.argus/daemon.log`) and waits for it, unless `--no-start`. | `cli::the_binary_finds_starts_and_stops_a_project_daemon` |
| Every response is a summary by default, paginated (P2). | Done | Nothing is added or removed on the way through: `--json` prints the daemon's result as one line; without it `render.rs` prints `{columns, rows}` results as an aligned table under their scalar fields (`total`, `next`, `source`) and everything else as pretty JSON. `next` is shown so the human can page with `cursor=`. | `cli::key_values_become_the_params_object`, `cli::the_cli_talks_to_a_daemon_over_its_socket` |
| Server-initiated notifications for subscriptions. | Done | `--follow` (implied by `obs.subscribe`) keeps the connection open and prints each notification as a line: `event death #12 frame 40 {...}` or JSON with `--json`. | (manual; `Client::next_notification`) |
| Errors carry code, sentence, hint, `file:line`. | Done | `render::error` prints exactly those four on stderr and exits 1; `--json` prints the error object on stdout. Exit codes: 0 ok, 1 daemon error, 2 usage, 3 no daemon. | `cli::the_cli_talks_to_a_daemon_over_its_socket` |

## §16 One schema

| Spec | Status | Where | Test |
|---|---|---|---|
| Each tool's description is generated from the same schema the CLI help uses; the CLI mirrors the tool set exactly, so a human reading the agent's transcript can rerun any call. | Done | `help.rs` builds every help page from `MethodDoc { name, doc, params, returns }`: the overview is one line per method, a namespace page is its methods, a method page is the usage line, doc, params, and returns verbatim. `argus session docs world.query` returns the same text from the daemon. Parameter names for bare values and usage lines are parsed from the `params` text (`params::names`), so there is no second list to drift. | `cli::every_method_is_a_subcommand_and_help_comes_from_the_same_table` |
| `argus docs --for-agent` emits an ARGUS.md from the live daemon. | Deferred | Phase 2, with the prefab catalog and scene summaries it needs; `session.docs` is the phase-0 form. | |
| `argus mcp` starts a stdio MCP server. | Deferred | Phase 2. The tool descriptions will come from the same `METHODS` rows. | |
| `argus token` for remote access. | Deferred | Phase 4 with the tablet client. | |
| `argus build --release`. | Deferred | Phase 2. | |
| `argus test`. | Partial | Item 10 (the scenario runner) adds it as the one subcommand that is not a daemon method. | |

## §9, §10 Mentions

| Spec | Status | Where | Test |
|---|---|---|---|
| "show as command" prints the CLI equivalent of every editor action. | Deferred | Phase 3 with the editor; the CLI's `key=value` grammar is what it will print. | |
| The agent's `input.inject` says `{action: "jump"}`. | Done | `argus input inject jump` (a bare value goes to the first parameter, `action`). | `cli::key_values_become_the_params_object` |

## Parameters, stated so the grammar is a decision

- `key=value` words. A value is JSON when it parses as JSON (`2`, `true`, `"text"`, `[1, 2]`), else a string; bracketed values accept bare words (`[Enemy, @player]`, `{of: @player, r: 6}`). Dotted keys nest (`where.has=[Enemy]`); a repeated key becomes a list; `--params '{...}'` or `--params @file.json` supplies a whole object that the words layer on top of.
- One bare word goes to the method's first declared parameter (`argus world get @player`, `argus sim step 5`, `argus scene load delve`), read from the docs `params` text. A second bare word goes to the second parameter; more than the method declares is a usage error naming the method.
- A key that must stay a string when it looks like a number is quoted: `id='"42"'`.

## §18 Build plan

Phase 0 item 9, "CLI": this crate, plus `argus_daemon::launch` (shared with `argusd`) and `session.shutdown` in the daemon so `argus daemon stop` is a method call like everything else.

## Known limitations, stated so they are decisions

- A daemon started by the CLI outlives the shell; `argus daemon stop` ends it, and `.argus/daemon.json` is removed on a clean exit. A daemon killed otherwise leaves a stale record, which `argus daemon status` reports and a method call replaces.
- The socket name is derived from the project path, so two checkouts of one project at different paths get different daemons, and one path gets one daemon.
- Help is English prose from the method table; there is no per-parameter type schema yet. When the MCP tool descriptions land (phase 2) the `params` text becomes structured and both surfaces read the structure.
- No shell completion; the overview is the list.
