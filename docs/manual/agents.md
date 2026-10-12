# Working with AI agents

Argus is built so an AI agent can work on a game the way a person does: it reads the live game, changes it, plays it, and tests it, through the same daemon the editor and the command line use. This chapter is for the people who direct agents and for the agents themselves: connecting one, the tools it gets, and how it should work.

## Connecting an agent

`argus mcp setup <tool>` adds Argus to an AI tool's own settings: Claude Code, Claude Desktop, Cursor, VS Code, Codex CLI, or Gemini CLI. [docs/install.md](../install.md) has each one, and what to do next in it.

- **For one game:** run it in the game's folder. The agent opens that game when it connects.
- **For every game:** add `--scope user`. The agent starts with no game open, and its project tools list your games, open one, or make one from a template.
- **Claude Desktop without installing Argus:** install the `.mcpb` bundle from a release (Settings, Extensions, Advanced settings, Install Extension). It carries Argus inside it.

When an agent connects, the server tells it what Argus is and gives it the open game's summary (its entities, systems, frame, seed, and clock), so it starts oriented.

## What an agent can do

The tools, by what they are for. Each is also a shell command (`argus query entity=@player`), and `argus help <tool>` describes it.

| For | Tools |
|---|---|
| Choosing a game | `projects` (the ones this machine knows, and the templates), `open_project`, `new_project` |
| Reading the docs | `docs`: the manual's chapters, the reference, and the open game's own `ARGUS.md` |
| Looking | `summary`, `query` (entities, paths, strings, timelines, tuning), `describe` (a component or system), `observe` (the frame as data, or a picture), `events` (what happened, and why) |
| Reading and writing files | `read_file` (a file, or a folder's listing), `write_file` (a script, scene file, test, or any text file in the project, as an undoable edit) |
| Changing the game | `edit` (entities, components, tiles, strings, def rows, assets, prefabs, and files, in one transaction with a note), `propose` (a change for a person to try before it is kept) |
| Making art and sound | `generate` (a sound effect or a sprite from a kind and a seed, free and offline, written as an asset to change; a sprite's answer has its picture) |
| Moving time | `run` (steps, turns, until an event, play and pause, saves), `seek` (to any earlier frame) |
| Checking | `test` (the scenarios, with seeds, tuning values set, or a sweep across values), `profile` (cost per system against the performance targets) |
| The loop with a person | `reload` (take changed scripts and assets), `marks` (moments a player flagged, with notes; `action=open` reproduces one), `review` (questions put to a person, and their answers) |

### Files, and which tools to use

Agents with their own file access (Claude Code, Cursor, Codex, Gemini, VS Code) can edit the project's files directly and run `argus` in a shell. Agents without it (Claude Desktop, and any agent over MCP alone) use `read_file` and `write_file`. They stay inside the open project's folder, and a write is an ordinary edit: it is in the change feed with its author and note, and `edit undo=true` takes it back. A file changed directly works as well (the game takes it at `reload`, or an entity file at `scene load`), but the change feed does not have it, so prefer `edit` and `write_file` for a change a person will want to see or undo.

After writing a script or an asset, call `reload` so the running game takes it. A script with an error stays out, the last good version keeps running, and `reload` answers with the error's file, line, and message.

## How an agent should work on a game

1. **Read first.** `docs topic=project` (or the project's `ARGUS.md`) for the game's own parts, and the manual chapter for the task: [scripting](scripting.md), [scenes](scenes.md), [gameplay](gameplay.md), [presentation](presentation.md), [content](content.md).
2. **Look at the game as it is.** `summary`, then `query` and `observe` for the parts the task touches. Run it a little (`run step=120`) and look again.
3. **Make the change.** Components and systems are Luau files in `components/` and `systems/`. Scene entities are TOML files in `scenes/<scene>/`, and `edit` changes them with a note. Follow the patterns the game already uses.
4. **Take it in and watch.** `reload`, then `run` and `observe` or `query` to see that it does what it should. `events` with `why` explains a surprise.
5. **Pin it with a scenario.** A file in `tests/` that drives the game and checks the new behaviour ([testing](testing.md)). Run `test`. A change is done when every scenario passes, the old ones included.
6. **Leave a trail.** Every `edit` and `write_file` takes a note saying why. When a change is a matter of feel (a jump height, a colour), `propose` it and let a person try it.

### Things that trip agents up

- **Randomness comes from `q:rng()`**, never from anything else, or runs stop being the same from the same seed, and tests and replays stop meaning anything.
- **`q:each` hands out reused handles.** Keep `e:id()` past the loop, not `e`.
- **A press is a turn on a discrete clock** (a roguelike): each press of a bound key is one turn, so a menu that should cost nothing freezes the press that opens or closes it.
- **`run_until` that never sees its event does not fail the test.** Check its `hit`.
- **Positions in `q:within` and `q:nearest` are from the end of the last tick** for `sim` systems.
- **The docs are the reference.** When unsure what a function takes or a file allows, `docs find=<word>` before guessing.

## Directing an agent

- Ask for one feature at a time, with how you will know it works: "add a double jump; a scenario should show the hero clearing the gap at the cave's mouth".
- Point it at marks: play the game, flag the moments that feel wrong with a note (see [playtesting](playtesting.md)), and ask the agent to work through `marks`. It opens each one at the exact frame.
- Read the change feed (`argus edit feed`, or the editor's feed panel) to see what it changed and why, and undo what you do not want.

## Where to go next

- [Concepts](concepts.md) and [your first game](first-game.md), to see the loop an agent follows.
- [Reference](reference.md), for where each part of the reference is.
- [docs/install.md](../install.md), to connect each AI tool.
