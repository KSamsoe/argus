# argus-client: spec trace

Every statement in the design document (v0.3) that constrains how a client reaches the daemon and speaks its language (§2 process model, §5 protocol, §11 "show as command", §16 one schema, P1), with where it lives and the test that pins it. Phase 4 item 0. Statuses as in the other traces: **Done**, **Partial**, **Deferred**.

## §2, §5, §11 A client, and only a client

| Spec | Status | Where | Test |
|---|---|---|---|
| (§11) "The editor is a separate process that speaks only the protocol." The crate a client is built on holds no world. | Done | This crate depends on `serde_json` and `interprocess` and nothing of the engine. The editor depends on it alone (`argus-editor/tests/protocol_only.rs` checks the tree); the CLI and the daemon use it for the socket and the grammar. | `argus-editor` `protocol_only` |
| (§5) JSON-RPC 2.0 over a local socket, one line per message, with server notifications. | Done | `socket.rs`: a project's socket name (a Windows named pipe or a Unix socket, a function of the root) and `.argus/daemon.json`. `client.rs`: `Client` blocks on each call and sets notifications aside (the CLI); `Conn` never blocks, a thread reading every line and handing answers and notifications over a channel, waking its owner (the editor, drawn a frame at a time). Moved here from `argus-daemon` and `argus-cli`, unchanged in behaviour. | `argus-daemon` `transport`; `argus-editor` `editor` |

## §11 "Show as command", §16 "rerun any call"

| Spec | Status | Where | Test |
|---|---|---|---|
| (§11) "Show as command" prints the CLI equivalent of every action the human takes, "so the human learns the agent's language by using the editor and can paste a command into a chat instead of describing a click". | Done | `command::line(method or tool, params)`: `argus sim step n=1`, `argus edit author=human note=... ops='[...]'`. Values in the CLI's own grammar (bare words, `[a, b]`, `{k: v}`, dotted keys for nested objects), JSON where that is the only form that reads back the same, single quotes for a POSIX shell where a word needs them, and `--params` for keys the words cannot carry. | `command::tests::calls_are_written_as_the_cli_types_them` |
| (§16) "A human reading the agent's transcript can rerun any call": a written line reads back to the same call. | Done | `command::read_line` (with `split`, a POSIX shell's word splitting) is the inverse: `read_line(line(t, p)) == (t, p)`, checked over strings that look like JSON, brackets, quotes of both kinds, `$`, backquotes, backslashes, newlines, accents, empty strings, nested lists and objects, dotted keys, and keys with dots in them. `words.rs` is the CLI's reader, moved here so both directions live together. | `command::tests::every_value_reads_back_the_same`, `a_shell_splits_the_words_as_written`; `argus-cli` `feed` |

## Known limitations, stated so they are decisions

- The line is for a POSIX shell (bash, and Git Bash on Windows), where the tests and the docs run. PowerShell reads single quotes the same way, but not `'\''` inside them.
- Bare values (a method's first parameter without its key) are the CLI's convenience: it knows the method's parameters from the daemon's docs, this crate does not. A written line always names its keys.
