# Concepts: how Argus works

This chapter explains the ideas the rest of the manual builds on: what a project is, the program that runs your game, and how entities, components, systems, clocks, and the change feed fit together. Read it first if you have never used Argus, whether you are a person or an AI agent.

## The big picture

Argus is a 2D game engine. The engine is written in Rust; your game is written in Luau scripts and TOML files. The game does not run inside an editor. It runs in a background program called the **daemon**, which holds the game's live world. Everything that works on the game talks to the daemon in the same language, the **protocol**:

```
  argus (the command line)    argus editor    AI agents (argus mcp)    argus test
            \                       |                  |                  /
             +----------------------+---------+--------+-----------------+
                                              |
                        the protocol: JSON-RPC, one message a line,
                        over a local socket (a named pipe on Windows)
                                              |
                               the daemon: one per project
                 the world, the clocks, the scripts, the renderer, audio,
                 the index of the files, the change feed and undo
                                              |
                                     the project's files
          argus.toml  components/  systems/  scenes/  prefabs/  assets/  tests/ ...
```

Three things follow from this:

- **There is no button that is not also a command.** The editor has no private way into the game. Anything you do in it, you could type, and an agent could do.
- **You and an agent work on the same world at the same time.** A change by either one shows up for the other at once, with a note saying who made it.
- **You ask the world, you do not read it all.** Every answer is a short summary by default, with a total and a way to page through the rest.

## A project and its folders

A project is a folder with an `argus.toml` in it. `argus new` makes one from a template (see [Your first game](first-game.md)). The folders you will meet:

| path | what it holds |
|---|---|
| `argus.toml` | The project: its `name`, its `clock`, its `default_scene`, and a few settings. |
| `components/` | Luau files that declare the game's components (its kinds of data). |
| `systems/` | Luau files that declare the game's systems (its rules). |
| `defs/` | Data tables: items, recipes, enemy stats, dialogue, timelines. A table's schema is Luau, its rows are TOML. |
| `prefabs/` | Reusable entity templates, one TOML file each. |
| `scenes/<name>/` | A scene: `scene.toml` and one TOML file per entity, or rooms of them. |
| `assets/` | Art, sound, fonts, and the small `.toml` "sidecar" files that describe them. |
| `input.toml` | The player's actions (`move`, `jump`) and the keys and buttons bound to them. |
| `tests/` | Scenarios: the game run headless with scripted input, and what must be true after. |
| `strings/`, `ui.toml`, `audio.toml`, `platform.toml` | Player-facing text, the UI theme, sounds and music, achievements. Optional. |
| `mods/`, `plugins/` | Mods laid over the game, and Rust plugins for hot paths. Optional. |
| `ARGUS.md`, `AGENTS.md`, `CLAUDE.md` | Guides for AI agents. `ARGUS.md` is written from the live game by `argus docs --for-agent`. |
| `.argus/` | The engine's own: the index, the daemon's log, frames, replays, marks, saves. Never edit it, and keep it out of git. |
| `dist/` | What `argus build` makes. Also kept out of git. |

Every file the engine reads is text you can open, diff, and review, except the binary chunks behind large tilemaps, which the engine writes for you.

## The daemon

There is one daemon per project, and it holds exactly one world. You rarely start it yourself: the first `argus` command that needs it starts one and says so.

```sh
argus test
# argus: starting a daemon for ...\my-game (pid 6612, log in .argus/daemon.log)
argus daemon status
# running: pid 6612 on argusd-2a1004d8cef11dec.sock (version 0.0.1)
argus daemon stop
```

The daemon keeps running after a command ends, so the next one is fast and the world is where you left it. Stop it when you are done with `argus daemon stop`. `argus daemon start --window` starts it with a game window you can play in.

A script error never takes the daemon down. A system that fails is switched off and the failure is reported, and the rest of the world stays there for you to look at. See [Scripting](scripting.md#how-errors-show-up).

## The protocol, three ways

Every protocol method has a name like `world.summary` or `edit.commit`, and every method is also a shell command: `argus world summary`. On top of the methods sit the **tools** (`summary`, `query`, `describe`, `edit`, `run`, `test`, `reload`, and the rest), the ones AI agents get through MCP. Each tool is a shell command too. So the same question can be asked three ways:

| who | how |
|---|---|
| you, in a shell | `argus summary` (the tool) or `argus world summary` (the method) |
| an AI agent | the `summary` tool over MCP (`argus mcp` serves them) |
| any program | `{"jsonrpc":"2.0","id":1,"method":"world.summary","params":{}}` on the socket |

Parameters are `key=value` words. Values that parse as JSON are JSON (`2`, `true`, `[1, 0]`), bare words are strings, and dotted keys nest:

```sh
argus query 'select=[id, Transform.pos]' 'where.has=[Player]'
argus run step=60
argus help query            # one tool's parameters
argus world                 # the methods in a namespace
```

Errors come back the same way to every client: a code, a sentence, a hint, and the file and line that caused it.

```
error 4203: @gem could be any of @east/gem, @west/gem
  hint: In a room scene an id is room/id; name the room.
```

## Entities

An **entity** is a thing in the world: the player, a coin, a wall, a bullet, the camera. An entity is only an id; everything it is comes from its components. There are two kinds:

- **Authored entities** are placed in a scene, one TOML file each. The file's name is the entity's string id: `scenes/main/player.toml` is `@player`. Authored ids are stable, so tests, scripts, and other files can name them.
- **Runtime entities** are made by systems while the game runs: bullets, sparks, generated monsters. They have only a number, written `e:12`, and they are never written to a file.

An entity can have a parent, so its position is relative to another entity: a flag on a pole, a button in a menu. Children declared inside an entity's file get ids like `@spikes/tooth1`. [Scenes](scenes.md) covers all of this.

## Components

A **component** is a named bundle of fields: plain data, with no code. `Transform` has `pos`, `rot`, and `scale`; a game's `Health` might have `cur` and `max`. The engine declares about sixty (Transform, Sprite, Camera, Collider, UiText, and the rest), and the game declares its own in Luau:

```lua
component "Player" {
  doc = "The one player: walks by the move action",
  speed = field.f32 { default = 6.0, range = {0, 50}, tune = true, doc = "Units per second" },
}
```

Every field has a type, a default, and a doc string, and may have a range and a `tune` flag (which puts it in the editor's tuning panel). A field outside its range is refused. A component with no fields is a **tag**: `tags = ["player"]` in an entity file makes one just by using it.

`argus describe component=Player` answers what a component is, which file declares it, and which systems read and write it.

## Systems

A **system** is a rule: a Luau function the engine calls every tick with a view of the world. Each system declares the components it reads and writes, and the engine holds it to that. A system that writes something it did not declare is stopped with an error naming the line.

```lua
return system "sim.move" {
  stage = "sim",
  reads = { "Actions", "Player" },
  writes = { "Transform" },
  doc = "Walks the player by the move action.",
  run = function(q)
    for _, a, p, t in q:each("Actions", "Player", "Transform") do
      local m = a.move
      if m.x ~= 0 or m.y ~= 0 then
        t.pos = t.pos + m * (p.speed * q.dt)
      end
    end
  end,
}
```

That is `systems/move.luau` from the blank template. Because every system says what it touches, "what could have changed the player's health?" is a query (`argus world who_writes component=Health`), not a search through code.

Systems run in five **stages**, always in this order:

| stage | what runs there |
|---|---|
| `input` | Devices become actions (`input.actions`); a pause menu that must see every press. |
| `sim` | The game's rules: movement, physics, pickups, damage. |
| `late` | After the sim settles: the camera, HUD text, animation, tweens, UI layout. |
| `hierarchy` | The engine works out where every child entity is in the world. |
| `present` | Drawing. It is read-only: a system there may not write. |

Inside a stage, `after = { "physics.overlap" }` orders one system after another. The engine's own systems (`physics.move`, `camera.update`, `ui.layout`, and more) sit in the same list as yours. `argus script systems` lists them all in the order they run.

Systems also talk through **events**: `q:emit("coin.taken", {...})` adds an event to the log, later systems in the same tick can read it, and you can see it with `argus events name=coin.taken`. [Gameplay](gameplay.md) has more on events.

## The two clocks

Time moves in **ticks**. Each scene picks one of two clocks:

| clock | written | how time moves | made for |
|---|---|---|---|
| continuous | `clock = "continuous@60"` | 60 ticks a second (or any rate), whether or not anything happens | action games, platformers, simulations |
| discrete | `clock = "discrete"` | one tick (a turn) each time the player presses a bound action, or when asked | roguelikes, puzzles, card and tactics games |

The default comes from `argus.toml` and a scene's `scene.toml` can change it. A continuous clock also has a speed: `argus sim speed x=3` runs three ticks every frame, which is how a colony sim fast-forwards. Gameplay reads time from the tick (`q.time`, `q.dt`), never from the computer's clock. `examples/delve` is a discrete game; `examples/swarm`, `hollow`, and `outpost` are continuous.

## Determinism: same seed, same run

Given the same scene, the same seed, the same input, and the same code, Argus produces the same world, tick for tick, down to the last bit, on the same platform and build. Randomness comes from named streams seeded from the run's seed and the stream's name, so adding a system never changes the numbers another system draws.

This is what the rest of Argus is built on:

- **Tests never flake.** A scenario run with seed 1 does the same thing every time. Each run reports the world's digest (16 hex digits); the same digest means the same world.
- **Replays work.** The daemon records the input and keeps snapshots ("keyframes"), so `argus seek frame=740` rebuilds any earlier moment of the run.
- **Marks reproduce.** When a player presses F8 on something that feels wrong, the mark carries what is needed to play that moment again, here or on another machine.
- **Comparisons mean something.** A proposal tried as A against B, or a tuning sweep over many seeds, differs only by the change.

You keep this true by following three rules in scripts: read time from `q`, draw random numbers from your system's stream (`q:rng()`), and keep every bit of state in components. [Scripting](scripting.md#the-sandbox) explains why each matters.

## Play and stop

There is no separate edit mode. The world is always there; "playing" only means the clock is running.

```sh
argus run play=true     # snapshot the world, then tick in real time
argus run pause=true    # stop the clock where it is
argus run stop=true     # go back to the snapshot taken at play
argus run step=60       # run 60 ticks now, playing or not
```

`stop` throws away what happened while playing (where the player walked, what got picked up) and puts the world back as it was. It does not undo edits to files; undo does that. The editor's Play, Pause, Stop, and Step buttons are these same calls.

## The change feed and undo

Changing the game's files through the daemon is an **edit**: one transaction with an author and a note. It applies all at once or not at all, writes only the keys that changed (comments and layout are kept), and updates the live world at the same moment.

```sh
argus edit author=human note="bigger coins" 'ops=[{op: prefab, prefab: coin, path: Sprite.size, value: [0.8, 0.8]}]'
argus edit feed               # every change, who made it, and why
argus edit undo=true author=human
```

The **change feed** lists every change from every client: commits with their notes, and also commands like play, a seek, or a step. Each row carries the `argus` command that would make it again, so the feed doubles as a shared language between you and an agent. Undo is per author: your undo never takes back the agent's work, and the agent's never takes back yours. The feed and the undo history live in the running daemon; when it stops they start fresh, while the files keep every change.

The editor marks its changes as `human` and the MCP server as `agent:claude`. The `edit` tool's own `author` defaults to `agent:claude`, even from a shell, so pass `author=` with your name when you edit by hand.

You can also change files in a text editor. Then tell the daemon: `argus reload` takes in scripts and assets, and `argus scene load` starts the scene again from its files. Hand edits do not appear in the feed and cannot be undone through it.

## What is yours and what is the engine's

| the project provides | the engine provides |
|---|---|
| its components (`Player`, `Coin`, `Health`) | about sixty components: Transform, Sprite, Camera, Collider, KinematicBody, CharacterController, UiNode, Light, Tween, NavAgent, ... |
| its systems (the rules of the game) | systems for input, physics, navigation and jobs, animation, tweens, particles, the camera, UI, dialogue, timelines, room streaming |
| scenes, prefabs, data tables, strings | the `dialogue` and `timeline` data tables, the UI, the renderer, the mixer |
| art and sound in `assets/` | importers for PNG, Aseprite, Tiled, WAV, fonts, Spine, shaders |
| scenarios in `tests/` | the test runner, replays of failures, goldens |
| `input.toml` | devices, rebinding, the input log |

A good habit: before writing a system, look at what the engine already does. The platformer template's player runs, jumps, and collides with no script at all, from its `CharacterController` and `KinematicBody`. `ARGUS.md` lists every component and system the game has, the engine's included.

## Where to go next

- [Your first game](first-game.md): make a project and add a feature, end to end.
- [Scripting](scripting.md): components, systems, and the `q` handle in detail.
- [Scenes](scenes.md): entity files, prefabs, rooms, and tilemaps.
- [Testing](testing.md): scenarios, seeds, and replays.
- [Reference](reference.md): how to reach every part of `argus docs`.
