# Your first game

This chapter is a tutorial. You make a game from a template, run its tests, look at it from the command line, and then add one small feature end to end: gems the player picks up. Along the way you write a component, a system, an entity, and a scenario, and finish with a release build. It is for anyone new to Argus; it assumes Argus is installed ([docs/install.md](../install.md)) and nothing else.

The commands below were run on a real copy of the `blank` template, and the outputs are what they printed (the two that open windows, in step 3, are described instead). Long paths in the output are shortened to `...\my-game`.

## 1. Make a project

```sh
argus new my-game
cd my-game
```

```
made my-game from the blank template in ...\my-game: 1 of 1 scenario runs pass

  cd ...\my-game
  argus test                    the scenarios in tests/
  argus editor                  the editor window
  argus mcp setup <client>      connect an agent (claude-code, cursor, vscode, codex, gemini, claude-desktop)

An agent starts with AGENTS.md (CLAUDE.md for Claude), which points at ARGUS.md.
```

`argus new` copies a template, names the game after the folder, runs the template's scenarios (they must pass), and makes the folder a git repository. Pick another template with `--template`:

| template | what you start with |
|---|---|
| `blank` (the default) | a player that walks over a floor, and one scenario: the smallest start |
| `platformer` | a side-on platformer: run, jump a pit and spikes, climb a ledge, reach the flag |
| `roguelike` | a turn-based dungeon: a hero, a hand-drawn map, monsters that chase and bite |
| `arena` | a top-down arena shooter: walk, auto-fire, waves from the edges chase you |
| `colony` | colonists gather berries and wood, haul them home, and eat: a colony sim start |

`argus new --help` and `argus projects` list them too. The blank game is these files:

```
my-game/
  argus.toml            name, clock, default scene
  input.toml            the move action, on WASD, the arrows, and a stick
  components/game.luau  the Player component
  systems/move.luau     sim.move: walks the player by the move action
  scenes/main/          scene.toml, player.toml, floor.toml
  tests/move.luau       one scenario: the player walks right
  ARGUS.md  AGENTS.md  CLAUDE.md   for AI agents
```

## 2. Run the tests

```sh
argus test
```

```
argus: starting a daemon for ...\my-game (pid 30860, log in .argus/daemon.log)
PASS  the player walks right  (main, seed 1, 6 ms)
1 scenario, 1 run, 0 failed, 6 ms
```

The first command that needs the game starts its daemon, the background program that holds the game's world ([Concepts](concepts.md#the-daemon)). It stays running, so later commands answer at once.

## 3. Play it

```sh
argus editor
```

The editor opens on the same daemon. The game is in the middle. Across the top are Play, Pause, Stop, Step, and the speed buttons: press Play, click the game, and the keys go to it (WASD or the arrows walk the yellow square). Stop puts the world back as it was when you pressed Play. On the left is the scene's hierarchy, and on the right the inspector, the tuning sliders, the change feed, and more. Everything you do there is also a command: the Show as command panel lists them. [Playtesting](playtesting.md) covers the editor in full.

To play without the editor, open a plain window on the running daemon and press play:

```sh
argus render window
argus run play=true
```

Click the window to give it the keys. When you are done, `argus run stop=true` puts the world back where it started, so the outputs below match yours.

## 4. Look at the game from the shell

You do not need a window to see what the game is doing. These are the same tools an AI agent uses.

**A one-screen orientation.** `argus summary` lists what is in the world (as JSON): entity counts by their components, the named entities, the systems in run order, the frame, the seed, and the clock.

```sh
argus summary
```

Shortened, it says:

```
{
  "archetypes": [
    [["Transform", "PlayerSlot", "Actions", "Sprite", "Camera", "Player", "player"], 1],
    [["Transform", "Sprite"], 1]
  ],
  "authored": 2,
  "clock": "continuous@60",
  "entities": 2,
  "named": [ ["@floor", ...], ["@player", ...] ],
  "scene": "main",
  "seed": 1,
  "systems": [ "post.hitstop", "rooms.stream", "input.actions", ..., "sim.move", "physics.move", ... ],
  ...
}
```

**What the player sees, as text.** `argus observe` describes the frame: the camera, and each thing on screen with its place in pixels.

```sh
argus observe
```

```
Tick 0, 640x360 px. Camera @player shows x -17.78 to 17.78, y -10 to 10.
@floor  at (50,0)-(590,360)  rect #2B3A42
@player  at (311,171)-(329,189)  rect #F2C14E
(392 bytes)
```

`argus observe format=png` draws the frame instead and writes it to `.argus/frames/frame-<tick>.png`.

**One entity, or a few fields of many.** `argus query entity=@player` prints every component and field of the player, and what its own file sets. With `select` and `where` it prints rows:

```sh
argus query 'select=[id, Transform.pos]'
```

```
frame: 0
total: 2
truncated: false
@floor   [0.0,0.0]
@player  [0.0,0.0]
```

**Make something happen.** Hold the move action to the right for 60 ticks, run 60 ticks (one second), and ask again:

```sh
argus input inject action=move 'value=[1, 0]' frames=60
argus run step=60
argus query 'select=[id, Transform.pos]'
```

```
frame: 60
total: 2
truncated: false
@floor   [0.0,0.0]
@player  [5.999996662139893,0.0]
```

Six units in one second: `Player.speed` is 6. Nothing ran in real time; `run step=60` ran sixty ticks as fast as it could.

## 5. Add a feature: gems

The plan: a gem lies on the floor three units right of the player. Walking over it takes it, adds its value to the player's score, and makes it vanish. That needs:

1. two components: `Gem` (on the gem) and `Score` (on the player),
2. a system that takes gems within reach,
3. a gem in the scene and a Score on the player,
4. a scenario that pins the behaviour, so it keeps working.

### The components

Make `components/gems.luau`:

```lua
-- Gems to pick up, and the score they add to.

component "Gem" {
  doc = "A gem on the floor: the player takes it by walking over it",
  value = field.u32 { default = 1, range = {1, 100}, doc = "Points it is worth" },
}

return component "Score" {
  doc = "What the player has picked up",
  points = field.u32 { default = 0, doc = "Points from the gems taken (written by sim.gems)" },
  reach = field.f32 { default = 0.8, range = {0.1, 5}, tune = true, doc = "How close a gem must be to be taken, in world units" },
}
```

Each field has a type, a default, and a doc. `reach` has `tune = true`, so it shows as a slider in the editor's tuning panel.

### The system

Make `systems/gems.luau`:

```lua
-- The player takes every gem within reach: its value goes into the
-- player's Score, the gem goes away, and a gem.taken event says so.
return system "sim.gems" {
  stage = "sim",
  after = { "sim.move" },
  reads = { "Transform", "Gem" },
  writes = { "Score" },
  doc = "Takes every Gem within Score.reach of the player: adds its value to Score.points, despawns it, and emits gem.taken {value, points}.",
  run = function(q)
    for _, score, t in q:each("Score", "Transform") do
      -- Collect first, despawn after the loop.
      local taken = {}
      for gem, g in q:within(t.pos, score.reach, "Gem") do
        score.points += g.value
        q:emit("gem.taken", { value = g.value, points = score.points })
        table.insert(taken, gem:id())
      end
      for _, id in ipairs(taken) do
        q:despawn(q:entity(id))
      end
    end
  end,
}
```

What it says:

- It runs in the `sim` stage, after the player has moved this tick.
- It reads `Transform` and `Gem` and writes `Score` (a system may read what it writes). Touching anything else is an error.
- `q:each("Score", "Transform")` visits every entity with both: here, the player.
- `q:within(pos, r, "Gem")` visits the gems within `r` of a point, nearest first.
- `q:emit` adds an event to the log. Gems are removed after the loop, by id, because the loop's handles move from entity to entity.

[Scripting](scripting.md) explains each of these.

Tell the running daemon to take the new files:

```sh
argus reload
```

```
{
  "assets": { "failed": [], "imported": [], ... },
  "errors": { "rows": [], "total": 0 },
  "scripts": {
    "errors": 0,
    "plugins": [],
    "reloaded": [ "components/gems.luau", "systems/gems.luau" ],
    "removed": []
  }
}
```

No errors. A mistake would be listed here with its file and line.

### Give the player a score

You could open `scenes/main/player.toml` and add a line. Instead, change it through the daemon, so the change has an author and a note and can be undone:

```sh
argus edit author=human note="the player keeps a score" 'ops=[{op: add, entity: "@player", component: Score}]'
```

```
{
  "author": "human",
  "command": "argus edit author=human note='the player keeps a score' ops='[{component: Score, entity: @player, op: add}]'",
  "files": [ "scenes/main/player.toml" ],
  "kind": "commit",
  "note": "the player keeps a score",
  "touched": [ "@player" ],
  ...
}
```

The file now ends with an empty table, which means "this component, at its defaults":

```toml
[Camera]
height = 20.0

[Score]
```

`argus edit undo=true author=human` would take it back, and `argus edit feed` lists every change with who made it.

### Put a gem in the scene

Make `scenes/main/gem.toml`. The file name is the entity's id, so this is `@gem`:

```toml
# A gem three units to the right of where the player starts.
[Transform]
pos = [3.0, 0.0]

[Sprite]
kind = "circle"
size = [0.6, 0.6]
color = "#5BC0EB"
z = 1.0

[Gem]
value = 5
```

A new entity file needs the scene loaded again, which starts the scene over from its files:

```sh
argus scene load
argus query 'select=[id, Transform.pos, Gem.value, Score.points]'
```

```
frame: 0
total: 3
truncated: false
@floor   [0.0,0.0]
@gem     [3.0,0.0]  5
@player  [0.0,0.0]     0
```

An empty column means the entity does not have that component.

### Try it

Walk right for a second, then ask what happened:

```sh
argus input inject action=move 'value=[1, 0]' frames=60
argus run step=60
argus events name=gem.taken
```

```
last_seq: 304
total: 1
truncated: false
by        fields                  frame  name       seq
--------  ----------------------  -----  ---------  ---
sim.gems  {"points":5,"value":5}  21     gem.taken  303
```

On frame 21 the player came within 0.8 of the gem and `sim.gems` took it. The score agrees:

```sh
argus query 'select=[id, Transform.pos, Score.points]'
```

```
frame: 60
total: 2
truncated: false
@floor   [0.0,0.0]
@player  [5.999996662139893,0.0]  5
```

The gem is gone from the list. It is gone only from the running world: `gem.toml` is still there, and `argus scene load` brings it back. (So does Stop, when you took the gem while playing.)

### Pin it with a scenario

A scenario runs the scene headless with scripted input and checks what must hold. Make `tests/gems.luau`:

```lua
-- The gem (scenes/main/gem.toml) is taken when the player walks over it.
return scenario "the player picks up the gem" {
  scene = "main", seed = 1,
  run = function(t)
    t.expect:count({ has = { "Gem" } }):eq(1)

    t.input:hold("move", { 1, 0 }, 60)
    local r = t:run_until { event = "gem.taken", max_frames = 120 }
    assert(r.hit, "the player never reached the gem")

    t.expect:events("gem.taken"):count():eq(1)
    t.expect:field("@player", "Score.points"):eq(5)
    t.expect:count({ has = { "Gem" } }):eq(0)
  end,
}
```

```sh
argus test
```

```
PASS  the player picks up the gem  (main, seed 1, 7 ms)
PASS  the player walks right  (main, seed 1, 8 ms)
2 scenarios, 2 runs, 0 failed, 15 ms
```

Each scenario loads the scene fresh, so it never depends on what you did in the shell before. To see what a failure looks like, change the expected points to 6 and run just this scenario:

```sh
argus test name=gem
```

```
FAIL  the player picks up the gem  (main, seed 1, 668 ms)
    seed 1: @player Score.points: 5 == 6 failed
    seed 1: replay at frame 22, with a clip: argus replay open path=...\my-game\.argus\replays\the_player_picks_up_the_gem\seed1
1 scenario, 1 run, 1 failed, 668 ms
```

A failing run saves a replay of the moment it failed, with a picture and a short clip, so you or an agent can look at exactly what happened. Put the 5 back and the tests pass again. [Testing](testing.md) covers seeds, goldens, and sweeps.

### Tell the agents

`ARGUS.md` describes the game's components, systems, and scenes for AI agents. It does not update itself; write it again after a change like this one:

```sh
argus docs --for-agent
```

```
wrote ...\my-game\ARGUS.md (2109 lines)
```

It now lists `Gem`, `Score`, `sim.gems`, and `@gem`.

## 6. Build a release

```sh
argus build
```

```
built my-game for windows (release) in ...\my-game\dist\windows
  packs: 12 files, 179174 bytes; this build ships 12 new blobs, 179174 bytes
  release forms: 0 sprites on 0 pages (0 BC7), 3 entity files in 1 bundles, 0 def tables, 0 sounds as Vorbis
  executable: ...\my-game\dist\windows\my-game.exe
  check: the build plays and looks as the project does (the same world after 300 ticks, drawn the same)
```

`dist/windows/my-game.exe` is the game, with its data in `dist/windows/data/`. The build checks itself before it says done: the release runs headless and must reach the same world as the project. `argus build --target web` makes a web page instead. [Releasing](releasing.md) covers targets, patches, and mods.

## 7. Stop the daemon

```sh
argus daemon stop
```

The world is gone; the files keep everything you changed. The next `argus` command starts a fresh daemon. Commit your work with git as usual: `.argus/` and `dist/` are already in `.gitignore`.

## What you did

| step | you used |
|---|---|
| made a game | `argus new` |
| ran its tests | `argus test` |
| looked at it | `argus summary`, `argus observe`, `argus query`, `argus events` |
| moved time on | `argus input inject`, `argus run step=` |
| added code | a component and a system in Luau, then `argus reload` |
| changed a scene | `argus edit` (one entity), a new entity file, then `argus scene load` |
| pinned it | a scenario in `tests/` |
| shipped it | `argus build` |

## Where to go next

- [Concepts](concepts.md): the ideas behind what you just did.
- [Scripting](scripting.md): more patterns: timers, spawning, events, pauses.
- [Scenes](scenes.md): prefabs, children, rooms, and tilemaps.
- [Gameplay](gameplay.md): input, physics, and the engine's other systems.
- [Working with AI agents](agents.md): `argus mcp setup`, and letting an agent do steps like these.
