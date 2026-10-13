# Playtesting

This chapter is about playing the game and judging it: the game window, the editor and its panels, play and stop, marks (a moment a player flags with a note), reports from players' builds, the phone review page, proposals and review requests between an agent and a person, and the change feed that records who did what.

Tests say whether the game does what it should. Playtesting is how a person says whether it *feels* right, and how that judgement reaches the code. Argus records a person's play as data, so a flagged moment can be reproduced exactly, and an agent's change can be felt before it is kept.

## Playing in a window

```sh
argus daemon start --window
```

This starts the project's daemon with a window to play in, and the game plays at once. Give a size as `--window 1600x900` (1280 x 720 by default). Other flags: `--scene <name>` and `--seed <n>` choose what loads, and `--mods a,b` lays mods over the game (see [Releasing](releasing.md)).

Closing the window stops the daemon. If a daemon is already running for the project, `daemon start` says so and opens nothing: run `argus daemon stop` first. (An agent can open a window on a running daemon with `render.window`; closing that one only closes it.)

Three keys work in the window, whatever the game binds:

| Key | What it does |
|---|---|
| F8 | Drop a mark here. The game pauses and asks for a note: type a line and press Enter, or Escape to take the mark back. The key is `[marks] key` in `argus.toml`. |
| F9 | Try the oldest waiting proposal (below), then switch to its next variant (A, B, C, ...), at the same moment of play. |
| F10 | Keep the variant on screen. Keeping A turns the proposal down. |

## The editor

```sh
argus editor
```

The editor is its own program. It opens on the project's daemon (starting one if none is running) and reaches the game only through the protocol, as the agent does. So there is nothing the editor does that the command line or an agent cannot do, and everything it changes is a row in the change feed (below). In the feed the person is `human`, or the name in `ARGUS_AUTHOR`.

Its layout:

| Part | What it is |
|---|---|
| The bar, at the top | The project and scene; Play or Pause, Stop, Step (one tick), 1x, 3x, 10x; the tick, frame, and clock; **Show as command**; who you are; and what the agent is waiting on you for ("The agent asks: 1 question, 1 proposal"). |
| The hierarchy, on the left | The scene's entities, fifty at a time with the total. Open a parent for its children. Filter by a component or a room. |
| The viewport, in the middle | The game itself. |
| The scrubber, under the viewport | The run so far, to drag back and forth. |
| The tabs, on the right | Inspector, Tuning, Agent, Profile, Tiles, Animator, Timeline, and Change feed. |

### The viewport

The daemon draws the game into the viewport as a window would. Click it, and your keys, mouse, and typing go to the game through its own bindings: F8, F9, and F10 work here too. Click while the game is not playing, or Alt and click while it is, to select what is under the pointer; the inspector shows it. **See as** draws the game as a colour-blind eye sees it (protanopia, deuteranopia, tritanopia, or achromatopsia), to check what reads.

### The inspector

The selected entity, every component and field. Each field's name is coloured by where its value comes from: the entity's own file, a prefab, or the default. Hover a name for the prefab, file, and line, the field's doc, and its range.

Change a field, or add or remove a component, and it is one transaction by you, written to the entity's file and listed in the feed as an `argus edit` line. A drag counts as one change when you let go. Undo and Redo (or Ctrl+Z and Ctrl+Shift+Z) take back only your own changes, never the agent's. Right-click a name to lock it, with an optional note ("camera feel is mine"): an agent's write into it then fails with your name and note.

### The scrubber

A strip that draws the run from the oldest frame you can go back to, to the furthest the run went. It shows the changes made between ticks, your marks, the agent's questions, and any game event you name under Mark events.

- Drag it and the game follows. A drag while playing pauses, and plays on when you let go.
- Frame -1, Frame +1, To the end, and the arrow keys over the strip move a frame at a time.
- **Try from here** makes a world of its own from this moment to play in, keeping the run. The World list switches back.
- **Replays** lists the saved replays, a failing test's among them with its failure. Open one to stand at that moment (see [Testing](testing.md)).

Seeking moves the world, not your files: an edit already written stays written. Undo takes it back.

### The other tabs

| Tab | What it does |
|---|---|
| Tuning | A slider for every tune field while the game runs, with Keep and Put back. See [Tuning and performance](tuning-and-performance.md). |
| Agent | What the agent is waiting on you for (below): its questions, its proposals, your open marks, and what it did, newest first. |
| Profile | The tick drawn against the game's budget, the performance targets, every system's cost, the slowest ticks (Go there seeks to one), and captures. See [Tuning and performance](tuning-and-performance.md). |
| Tiles | Paint a tilemap in the viewport: pick a map and a layer, then Pencil (B), Rectangle (R), Pick (I), or Eraser (E), with a tile or an auto-tiling terrain. A stroke shows at once and becomes one change when you let go. It can also make a new map. See [Scenes](scenes.md). |
| Animator | The selected entity's animator live (its state, frame, and events), and its state machine to edit: states, clips, rates, wind-ups and holds, events by frame, transitions. See [Presentation](presentation.md). |
| Timeline | A timeline's tracks as lanes against time. Drag a cue to move it, its right edge to change its length; add and remove cues; Play it with an actor. See [Content](content.md). |
| Change feed | Every change from every client (below), filtered by author. |

Each change in the Tiles, Animator, and Timeline tabs is one transaction written to its file with the file's comments kept, so Ctrl+Z puts back the file's exact text.

### Show as command

Tick **Show as command** and a panel lists everything you did as `argus` lines, each with Copy. It is how you learn the agent's language: paste a line into a chat instead of describing a click. The lines are written for bash (Git Bash on Windows).

## Play and Stop

Play takes a snapshot and runs the game in real time. Pause holds it where it is. Step runs one tick.

Stop goes back to the snapshot taken at Play, like a game engine's play mode. What you kept while playing stays kept: if you tuned a value and pressed Keep during play, the file already has it, and Stop goes back and then makes the kept change again in the world, so the two agree.

The same calls from a shell are `argus sim play`, `argus sim pause`, `argus sim stop`, `argus sim step n=1`, and `argus sim speed x=3`.

## Marks

A mark is a moment a person flags while playing, with a note: "jump feels floaty". Press F8 in the window or the viewport, or from a shell:

```sh
argus mark drop note="jump feels floaty"
```

A mark keeps, in `.argus/marks/<id>/`: the frame as a picture, a two-second clip replayed from the run, the last two seconds of sound, the five seconds of input and events before it, the world's digest, and a replay file that reproduces the moment after the daemon is gone. On a discrete clock the seconds are turns. The input it keeps includes the host's messages (see [Talking to the host](gameplay.md#talking-to-the-host)), so a turn decided by an outside service replays without asking it again.

Marks wait in a queue for the agent:

```sh
argus marks                                        # the open marks
argus marks action=open id=1                       # reproduce one
argus mark note id=1 note="the jump, not the fall"  # reword the note
argus marks action=close id=1 note="fixed: jump gravity"
```

Opening a mark loads its scene with its seed, restores its keyframe, plays the five seconds before it, and stops paused at the mark's frame. It then compares the world with the one the mark saved, field by field. Before a fix they should match (`digest.same` is true); after a fix, the difference is the fix's effect at that frame. A mark records the code version it was made with: the git revision, and when there were uncommitted changes, `+dirty.` and eight hex digits naming what the changed files held (`git:8940ef6+dirty.3fa2c1d0`). Changes to files no run reads leave it as it was: anything under a folder whose name starts with a dot, `dist/`, `tools/`, the notes at the top (`README.md`, `ARGUS.md`, ...), and what `[build] exclude` names. A mark replays faithfully only with the same code, so `mark.open` returns `code.mark` and `code.now`, and `code.same`: false, with a sentence in `code.said`, when a script or data file has changed since the mark was made, committed or not.

In the editor's Agent tab your open marks show with their pictures and notes, to reword, Reproduce, Close, or Take back.

## Reports from players

A player's build has no daemon, but it keeps the last thirty to sixty seconds of play. When a player presses F8 (or the game's `[marks] key`) and types a note, or when the game crashes, it saves a report: a replay file in the save folder's `reports/`, named `<time>-report.argr` or `<time>-crash.argr`.

Turn a report into a mark and open it like any other. The path is relative to the project, or absolute:

```sh
argus mark import path=C:/Users/you/Downloads/1791598765-report.argr
argus marks action=open id=2
```

To collect reports without asking players to send files, name a playtest server in `argus.toml`. A desktop build then posts each report there at once; one that cannot go waits and goes the next time the game starts (sent ones move to `reports/sent/`). From `examples/hollow/argus.toml`:

```toml
[reports]
url = "http://100.70.114.55:8765/"
```

The build speaks plain HTTP, so use a LAN or tailnet address. The web page's F8 posts its report to the server it was loaded from; if that fails, it offers the player the file to save instead. Fetch what a server collected into the mark queue with:

```sh
argus mark pull url=http://100.70.114.55:8765/
```

Each report is pulled once; the project remembers which in `.argus/marks/pulled.json`.

The playtest server is `tools/web/serve.mjs` in the engine's repository, run with Node: `node tools/web/serve.mjs <site> [port] [host] [marks-dir]`. It serves a web build and takes reports at `POST /marks`. It is not part of an Argus install yet. [docs/playtest.md](../playtest.md) describes the one Kevin runs. Building the page is in [Releasing](releasing.md).

## Two web pages

Two different pages run in a browser. Do not confuse them.

| | The web playtest page | The phone review page |
|---|---|---|
| What it is | The game itself, built with `argus build --target web` | A small client of your running daemon |
| Who uses it | Players, anywhere they can reach it | You, on a phone or tablet on this machine's tailnet |
| What it does | Plays the game; F8 sends a report | Answers review requests, watches the game, moves sliders, reads the feed |
| Served by | Any web server (the playtest server collects reports) | The daemon, with `--web` |
| Needs | A browser with WebGPU or WebGL2 | A token |

## The phone review page

Start the daemon with its web listener, then make a token and open its link on the phone:

```sh
argus daemon start --web tailnet
argus token issue scope=review hours=4 label=phone
```

The second command prints a `url` with the token in it (`http://<address>:7878/#t=argus-...`). The page takes the token from the link and clears it from the address bar. The token is shown once; the project keeps only its hash.

`--web` decides where the page listens:

| Value | Listens on |
|---|---|
| `--web` or `--web local` | This machine only (127.0.0.1), port 7878. |
| `--web tailnet` | This machine's Tailscale address, port 7878, for your other devices on the tailnet. |
| `--web :7891` | This machine, on another port. |
| `--web 100.x.y.z:port` | That address, if it is this machine's or its tailnet's. |

Any other address is refused when the daemon starts: the page has no TLS, and relies on Tailscale to encrypt the link.

A token's scope says what the page may do:

| Scope | It may |
|---|---|
| `observe` | Read: the status, frames, the review queue, the feed. |
| `review` (the default) | Also answer review requests, and drop and note marks (the page has no button for marks yet). |
| `edit` | Also play, pause, tune, and edit, as the editor does. |

No token can open or close a session, stop the daemon, open a window, or make tokens. A token lasts 12 hours unless `hours=` says otherwise, at most 720 (30 days). `argus token list` shows them; `argus token revoke id=<id>` ends one at once, and a page using it is cut off within a second.

The page has four tabs: **Reviews** (the questions waiting, each with its clip and choices), **Watch** (the game's frames, and Play and Pause with an edit token), **Tune** (the tune fields' sliders, with an edit token), and **Feed**. An answer given there is recorded as `web:<label>`, here `web:phone`.

Limits: the page sends no game input, so you cannot play from it. Frames come one picture at a time, about 88 a second at 480 x 270. There is no TLS beyond the tailnet.

## Proposals: a change to feel before it lands

An agent can offer a change instead of making it. A proposal has variants: A is the game as it is, and B, C, and so on are the agent's alternatives.

```sh
argus propose note="a floatier jump" ops='[{op: set, entity: "@player", path: CharacterController.jump_time, value: 0.5}]'
argus propose proposal=1 note="a snappier jump" ops='[{op: set, entity: "@player", path: CharacterController.jump_time, value: 0.32}]'
```

Nothing changes in the world or the files until a person picks. To try it, press F9 in the window or viewport (it tries the oldest waiting proposal), or use Try it in the editor's Agent tab. Each variant becomes a world of its own, all fed the same input at the same tick, so F9 switches between them at the same moment of play. F10 keeps the variant on screen. From a shell:

```sh
argus proposal list
argus proposal try id=1
argus proposal show variant=C
argus proposal pick id=1 variant=C
argus proposal reject id=1 note="neither, the jump is fine"
```

Picking is an ordinary commit by whoever picked: it is written to the files, it appears in the feed, and undo takes it back. Picking A turns the proposal down. A seek or a scene load ends a trial, and `proposal stop` ends it with the proposal still waiting.

A proposal holds entity edits only: `set`, `add`, `remove`, `spawn`, and `despawn`.

## Review requests: a question for a person

The reverse direction: the agent asks a person to judge a moment by feel.

```sh
argus review question="does the landing feel heavy enough?" options='[yes, "too heavy", "too light"]' note="raised fall_gravity"
```

The question goes to the editor's Agent tab (and the phone page) with a clip of the last two seconds, replayed from the run (`frames=[from, to]` picks others, at most ten seconds). The person scrubs the clip and answers with a key (1 to 9 for the choices) or a sentence. Each request is answered once. Read the answers with:

```sh
argus review list=true
```

The queue lives in `.argus/reviews/` and outlives the daemon, so the agent can ask, keep working, and read the answers later.

## The change feed and undo

Every call that changes something, from any client, is a row in the change feed: edits, play and pause, speed, seeks, slider moves, picks, notes, and answers. Lookups are not in it, and neither are key presses (the input log has those). Each row has its author, the frame, and the `argus` command that would do the same.

```sh
argus edit feed limit=10
```

A shortened example, after a mark, a proposal, and a pick:

```text
author        command                                         frame  id  kind
------------  ----------------------------------------------  -----  --  --------------
cli           argus mark drop note='jump feels floaty'        631    3   command
agent:claude  argus propose author=agent:claude note=...      631    6   {"proposed":1}
human         argus proposal pick id=1 variant=C              631    10  commit
```

A commit's row also has its note, the entities it touched, and the files it wrote. A drag of the scrubber or the timeline ruler is one row, not forty.

Undo is per author: it takes back that author's latest commit, as a new commit, so a person's undo never unwinds the agent's work.

```sh
argus edit undo=true author=human
```

With nothing of that author's to undo, it says so (`error 4705: nothing to undo for agent:claude`). `argus edit redo author=human` redoes. Locks (`argus edit lock`) fence entities, fields, assets, or def rows for one author; see [Working with AI agents](agents.md).

## Who did what

Each connection tells the daemon who it is, and the feed names that author:

| Who | Author in the feed |
|---|---|
| The editor | `human` (or the name in `ARGUS_AUTHOR`) |
| An agent over `argus mcp` | `agent:claude` |
| The `argus` command line | `cli` (or `ARGUS_AUTHOR`) |
| The phone page | `web:<the token's label>` |

A call's own `author` parameter wins over the connection's. Some calls have an author of their own with a fixed default, whoever makes them: the `edit` and `propose` tools (and `write_file`) default to `ARGUS_AUTHOR` when it is set, else `agent:claude`, and `tune set` (the session's author), `mark drop` (the mark's), and `proposal pick` (the commit's) default to `human`. From a shell, pass `author=` when it matters who is named.

## Limits

- Notes are typed. A spoken note was planned and not built.
- The phone page cannot play the game, and has no TLS beyond the tailnet.
- The playtest server is not shipped with an install.
- A mark or report replays faithfully only with the code that made it.

## Where to go next

- [Testing](testing.md): turning a mark into a scenario that keeps it fixed.
- [Tuning and performance](tuning-and-performance.md): the Tuning and Profile tabs, and sweeps.
- [Working with AI agents](agents.md): how an agent reads marks, proposes, and asks.
- [Releasing](releasing.md): builds that send reports, and the web page.
- [Reference](reference.md): `mark.*`, `proposal.*`, `review.*`, `token.*`, and `edit.feed` in `argus docs`.
