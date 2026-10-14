# Scripting

This chapter is a guide to writing a game's Luau: declaring components, declaring systems, using the `q` handle a system gets, the patterns most games need, what the sandbox forbids, and how errors and reloads work. It is for anyone writing or reviewing an Argus game's code. Every function is listed in the reference, under "Scripts: the Luau API" in `argus docs` (and at the end of your project's `ARGUS.md`); this chapter shows how they fit together.

## Where scripts live

| folder | what its files declare |
|---|---|
| `components/` | components: the game's kinds of data |
| `systems/` | systems: the game's rules |
| `defs/` | data table schemas (their rows are TOML in `defs/<table>.toml`; see [Content](content.md)) |
| `lib/` | modules: code the other files share, which they `require` (see [Sharing code](#sharing-code)) |

Each file returns one declaration, or a table of them. Calling `component` is what declares a component, so a file can declare several and return the last, as the platformer template's `components/level.luau` does with `Hero`, `Hazard`, `Coin`, and `Goal`. Names are global across the project: a component's name is its type (`Health`), a system's name is its id (`sim.move`).

Systems in one file share its local functions and constants: `return { system "input.menu" {...}, system "sim.menu" {...} }` lets two systems that belong together use the same helpers. Code that files in different places need goes in a module in `lib/`, which each of them `require`s: see [Sharing code](#sharing-code).

Luau is Lua with a few additions: `+=` and `-=`, optional types, and a built-in `vector` type that Argus uses for every `vec2` field.

## Declaring components

```lua
component "Health" {
  doc = "Hit points; zero is dead",
  cur = field.i32 { default = 10, range = {0, 100000}, doc = "Current" },
  max = field.i32 { default = 10, doc = "Maximum" },
}
```

That is from `examples/swarm/components/swarm.luau`. Each field is `name = field.<type> { ... }`, and fields keep the order they are written in.

| field type | holds | in Luau | in an entity file |
|---|---|---|---|
| `bool` | true or false | boolean | `true` |
| `u8`, `u16`, `u32`, `u64` | whole numbers from 0 | number | `3` |
| `i8`, `i16`, `i32`, `i64` | whole numbers, signed | number | `-3` |
| `f32`, `f64` | numbers | number | `0.5` |
| `vec2` | a 2D vector | `vector` (`.x`, `.y`) | `[1.0, 2.0]` |
| `str` | text | string | `"hello"` |
| `entity` | a reference to another entity | an entity handle | `"@door.east"` |
| `ref` | a data table row or a project path | string | `"item:bread"`, `"door/locked"` |
| `list` | a list of any of these but `entity` | a read-only array | `["oiled", "burning"]` |

A field takes four options, all optional:

| option | what it does |
|---|---|
| `default` | A new component's value. A `vec2`'s is `{x, y}`: `field.vec2 { default = {3, 1} }`. |
| `range` | `{lo, hi}`. A write outside it is refused, in an entity file and in a script. |
| `tune` | `true` puts the field in the editor's tuning panel as a slider. |
| `doc` | What the field is. Agents read it in `ARGUS.md` and `argus describe`; write it for them. |

A component itself takes `doc`, and `version` and `migrate` for save games made before its fields changed (see [Gameplay](gameplay.md) on saves).

### List fields

A list field holds any number of values of one type, in order: what is on a tile in the order it came, a room's monster pool, an item's loot floors. `of` says the type of its elements:

```lua
component "Tile" {
  on = field.list { of = "str", doc = "What is on it, in the order it came" },
}
```

`of` is any type above but `entity`; a list takes `default` (`default = { "dry" }`) and `doc`, not `range` or `tune`, and holds at most 1,024 elements. In an entity file it is an array, `on = ["oiled", "burning"]`, a `vec2` element as `[x, y]`.

A list is one value, like a string. Reading it gives a read-only array: `#t.on`, `t.on[1]`, `ipairs(t.on)`, and `table.find(t.on, "oiled")` work. Changing it is assigning a new one:

```lua
local on = table.clone(t.on)
table.insert(on, "burning")
t.on = on
```

Writing into the array read (`t.on[1] = "wet"`) fails with "attempt to modify a readonly table", since it would change nothing in the world. A table that is not an array, or an element of the wrong type, is refused, naming the element. A `{Tile.on}` hole in a text reads the elements with commas between ("oiled, burning"), and an event can carry a list: `q:emit("ruled", { tags = { "fire", "oil" } })`.

A component with no fields is a **tag**. Most tags need no declaration at all: `tags = ["enemy"]` in an entity file makes one by using it, and `q:each("enemy")` finds them.

## Declaring systems

```lua
return system "sim.bullets" {
  stage = "sim",
  after = { "sim.fire" },
  reads = { "Bullet", "Velocity", "Transform" },
  writes = { "Bullet", "Transform" },
  doc = "Bullets fly straight and expire after ttl ticks.",
  run = function(q)
    ...
  end,
}
```

From `examples/swarm/systems/bullets.luau`. The keys:

| key | required | what it is |
|---|---|---|
| `stage` | yes | `input`, `sim`, `late`, `hierarchy` (the engine's transform step), or `present` (drawing, read-only). Game rules go in `sim`; a camera or HUD text in `late`; a pause menu in `input`. |
| `run` | yes | `function(q) ... end`, called once a tick (once a turn on a discrete clock). |
| `reads`, `writes` | | The components it touches. A component in `writes` may be read too. Anything else is an error when it happens. |
| `spawns` | | The prefabs it spawns with `q:spawn_prefab` (`spawns = {"bullet"}`). Naming one grants all its components, so they need not be in `writes`. |
| `after` | | Systems in the same stage this one runs after. |
| `before` | | Systems in the same stage this one runs before: for one you cannot change, such as an engine system. A look system that sets an animator's inputs says `before = { "anim.update" }`, so the frame changes the same tick. (`argus script systems` shows the order, and `last` on the engine systems that end their stage, such as `particles.update`, which already run after yours.) |
| `doc` | | What it does. Shown in `ARGUS.md`, `argus script systems`, and the editor. |
| `rng` | | The name of its random stream, for `q:rng()`. |
| `clock` | | `continuous` or `discrete`, in a scene that has both; by default the scene's own clock. |
| `unfrozen` | | `true` runs it on ticks that `q:freeze()` stopped (a pause menu, the UI). |
| `between_turns` | | `true`: in a turn-based scene it also runs on the idle ticks between turns, and takes no time on a turn (`q.dt` 0). For what should move while the game waits. |
| `on_load` | | `true` also runs it once when the scene loads, before the first tick (a generated floor). |

Two rules about declarations catch people out:

- **Spawning counts as writing.** `q:spawn({"Spark", "Transform", "Sprite"})` needs all three in `writes`. Spawning from a prefab needs the prefab in `spawns` instead.
- **Systems with no `after` between them have no promised order.** If one system must see what another did this tick, say so with `after`. `argus script systems` shows the order now, the engine's systems included, so you can find the right one to follow (`physics.overlap` for touches, `input.actions` for input).

## The q handle

`run` receives `q`, the system's view of the world. Its fields are read with a dot and its methods called with a colon.

| what for | fields and methods |
|---|---|
| time, and who is asking | `q.tick`, `q.time` (sim seconds), `q.dt` (seconds this tick covers), `q.day`, `q.day_time`, `q.day_length`, `q.name` (this system's id) |
| finding entities | `q:each("A", "B")`, `q:count("A")`, `q:entity("@id")`, `q:get(e, "A")` |
| near a point | `q:within(pos, r, "A")`, `q:nearest(pos, r, "A")` |
| making and removing | `q:spawn(...)`, `q:spawn_many(n, {...}, fn)`, `q:spawn_prefab(name, fields)`, `q:despawn(e)` |
| events | `q:emit(name, fields)`, `q:events(name)`, `q:send(channel, text)` (text for the host; see [Talking to the host](gameplay.md#talking-to-the-host)) |
| randomness | `q:rng()`, then `rng:next()`, `rng:below(n)`, `rng:range(lo, hi)`, `rng:chance(p)` |
| time control | `q:freeze()`, `q:speed()`, `q:speed(x)` |
| saves | `q:save(name, summary)`, `q:load(name)`, `q:saves()`, `q:delete_save(name)` |
| presentation | `q:tween(e, "Comp.field", opts)`, `q:play(sound, fields)`, `q:text(key, args)`, `q:languages()` |
| data tables | `q:def("item:bread")`, `q:defs("recipe")` |
| maps and paths | `q:tiles(map)`, `q:path(from, to)`, `q:walkable(pos)`, `q:distance(field, pos)`, `q:downhill(field, pos)` |
| the platform | `q:achieve(id)`, `q:presence(key, value)`, `q:overlay(page)` |

The entities `q:each`, `q:entity`, and `q:spawn` hand you have their own methods: `e:id()`, `e:name()`, `e:alive()`, `e:has("A")`, `e:get("A")`, `e:insert("A", fields)`, `e:remove("A")`, `e:despawn()`. A component from `e:get` or a loop reads and writes its fields directly: `hp.cur -= 1`.

Things worth knowing:

- **`q:each` reuses its handles.** The loop moves one handle along from entity to entity. To remember an entity past its turn in the loop, keep `e:id()` and get it back with `q:entity(id)`. To despawn inside a loop, collect ids first and despawn after it.
- **Handles last one run.** A handle kept in a variable until the next tick is an error ("handle used outside its system's run"). Ask again each tick.
- **Points are vectors, tables, or entities.** `q:within` and `q:nearest` take `t.pos`, `{0, 0}`, or an entity, which searches from its place (its GlobalTransform when it has one, else its Transform): `q:within(me, 3, "Crumb")`.
- **Vectors.** Make one with `vector.create(x, y, 0)`, read `.x` and `.y`, and add, subtract, and scale them: `t.pos + v.v * q.dt`.
- **`q:events` is this tick only.** It sees what systems before it emitted this tick (order them with `after`). Keep anything that must last in a component.

## Patterns

Each of these was run as shown. The files named are in the repository; the others are small enough to copy.

### A player that moves

From `templates/blank/systems/move.luau`. Input arrives as the `Actions` component (one field per action in `input.toml`), never as keys:

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

A button action `jump` gives three fields: `jump` (held), `jump_pressed`, and `jump_released` (this tick). For a side-on character, use the engine's `CharacterController` instead of moving it yourself, as `templates/platformer/scenes/level/player.toml` does; [Gameplay](gameplay.md) explains it.

### A timer

Keep the time left in a component and count it down by `q.dt`. The next three patterns share these components, in one file in `components/`:

```lua
component "Spawner" {
  doc = "Makes a spark every few seconds, somewhere near itself",
  every = field.f32 { default = 2.0, range = {0.1, 60}, tune = true, doc = "Seconds between sparks" },
  left = field.f32 { default = 0.0, doc = "Seconds until the next spark (written by sim.spawner)" },
  spawned = field.u32 { default = 0, doc = "Sparks made so far" },
}

component "Spark" {
  doc = "A short-lived spark, made at run time",
  ttl = field.f32 { default = 1.5, range = {0, 60}, doc = "Seconds left to live" },
}

return component "Tally" {
  doc = "Counts what happened",
  sparks = field.u32 { default = 0, doc = "Sparks seen (written by late.tally)" },
}
```

and one entity in the scene with a Spawner and a Tally:

```toml
[Transform]
pos = [5.0, 5.0]

[Spawner]
every = 0.5

[Tally]
```

This system makes a spark every `every` seconds:

```lua
return system "sim.spawner" {
  stage = "sim",
  reads = { "Transform" },
  writes = { "Spawner", "Spark", "Transform", "Sprite", "Tween" },
  rng = "sparks",
  doc = "Every Spawner.every seconds, makes a Spark within 3 units of the spawner (spark.spawned {spark, n}).",
  run = function(q)
    for _, s, t in q:each("Spawner", "Transform") do
      s.left -= q.dt
      if s.left <= 0 then
        s.left += s.every
        local rng = q:rng()
        local spark = q:spawn({ "Spark", "Transform", "Sprite" })
        spark:get("Transform").pos = t.pos + vector.create(rng:range(-3, 3), rng:range(-3, 3), 0)
        local sp = spark:get("Sprite")
        sp.kind = "circle"
        sp.size = vector.create(0.4, 0.4, 0)
        sp.color = "#FFD866"
        q:tween(spark, "Sprite.size", { from = {0.1, 0.1}, to = {0.4, 0.4}, duration = 0.2, easing = "out_back" })
        s.spawned += 1
        q:emit("spark.spawned", { spark = spark, n = s.spawned })
      end
    end
  end,
}
```

This one also shows randomness (its own stream, `rng = "sparks"`) and a tween, which is an entity of its own, so the system declares `Tween` in `writes`.

Watch a timer that counts down a field with a `range`. A write below the range stops the system, so `ttl -= q.dt` on a field with `range = {0, 60}` fails on the tick it would go negative ("Spark.ttl must be in range [0, 60]; got -0.0166"). Clamp it:

```lua
s.ttl = math.max(0, s.ttl - q.dt)
```

A timer counted in ticks works the same way with a whole-number field and `-= 1`, as `examples/swarm/systems/fire.luau` does with `Player.cooldown`.

### Spawning and despawning

The sparks above live for `Spark.ttl` seconds. This system counts them down and removes them, collecting ids first:

```lua
return system "sim.sparks" {
  stage = "sim",
  after = { "sim.spawner" },
  writes = { "Spark" },
  doc = "Counts down every Spark's ttl and despawns it at zero (spark.gone).",
  run = function(q)
    local done = {}
    for e, s in q:each("Spark") do
      s.ttl = math.max(0, s.ttl - q.dt)
      if s.ttl <= 0 then
        table.insert(done, e:id())
      end
    end
    for _, id in ipairs(done) do
      q:entity(id):despawn()
      q:emit("spark.gone", {})
    end
  end,
}
```

For many at once, `q:spawn_many(n, {"Transform", "Enemy"}, function(e, i) ... end)` makes all of them in one call, handing each to the function through one reused handle; `examples/swarm/systems/waves.luau` spawns whole waves with it. Spawned entities are runtime entities: they are never written to a scene file, and only `q:spawn("id")` gives one a string id. An id another entity has is an error that names it; `q:find("@id")` returns the entity, or nil when there is none, so a script can ask first.

### Spawning from a prefab

When what a system makes has a look, a body, and numbers worth tuning, keep it in a prefab (a file under `prefabs/`, see [scenes](scenes.md#prefabs)) and spawn that. The system names the prefab in `spawns`, and gives only what differs for this one:

```lua
return system "sim.fire" {
  stage = "sim",
  reads = { "Transform", "Gun" },
  writes = { "Gun" },
  spawns = { "bullet" },
  doc = "Each gun fires a bullet when it is ready.",
  run = function(q)
    for e, tr, gun in q:each("Transform", "Gun") do
      gun.left -= q.dt
      if gun.left <= 0 then
        gun.left = gun.every
        q:spawn_prefab("bullet", { ["Transform.pos"] = tr.pos, ["Bullet.dir"] = gun.aim })
      end
    end
  end,
}
```

The bullet comes out with every component and field `prefabs/bullet.toml` gives it (its chain of base prefabs included), then the fields in the table. Its children come too, each a runtime entity under it, with a `Parent` and a `GlobalTransform`. Naming `"bullet"` in `spawns` grants all its components, so the system lists none of them in `writes`, and a component added to the prefab later needs no change to the system. The prefab is read as it is now: an edit to it (`argus edit` with a `prefab` op, or the editor) applies to the next bullet, though not to the ones already flying. A field in the table must belong to one of the prefab's own components, and a prefab's entity field may name an authored entity (`"@player"`) that is in the world when it spawns.

### Switching entities off

An entity can be switched off without despawning it: it stays in the world, keeping everything it had, but it is out of the game. `q:each` and `q:count` pass it by, it is not drawn, physics, the UI, and every engine system skip it, it is near nothing, and the semantic frame does not show it. Everything under it (its children, through `Parent`) is off with it. It is still saved, in the digest, and put back by a seek, so switching it on again is exact.

```lua
for e in q:each("room_b") do e:set_active(false) end   -- park room b
for e in q:inactive("room_a") do e:set_active(true) end -- bring room a back
```

`e:set_active(on)` switches one, `e:active()` says whether it is in the game (not off, and not under one that is), and `q:inactive("A", ...)` goes over the entities with those components that are off, or under one that is, handing out each entity and then its components as `q:each` does (`for e, mo in q:inactive("Monster")`). Switching writes the engine's `Inactive` component, so the system lists it: `writes = { "Inactive" }`. An entity file can start off with `tags = ["Inactive"]`.

This is how a game keeps what it is not using now: a pool of bullets switched on as they are fired and off as they land instead of spawned and despawned, a menu built once and shown when opened, or the rooms of a floor a game makes at run time, each room's entities tagged with it and every room but the player's switched off. A child switched off by itself stays off when its parent comes back on.

Agents and the editor see inactive entities: the protocol's `world.query` includes them (`"where": {"not": ["Inactive", "InactiveUnder"]}` leaves them out), and `InactiveUnder` marks the children of a switched-off entity. A web page's `player.query` includes them too, and `!Inactive,!InactiveUnder` leaves them out ([Releasing](releasing.md)).

`world.query`'s rows come in entity order unless `order` says otherwise. Entity order is by each entity's number, which switching entities off and on does not change, but despawning and spawning does (a slot is reused). A tool or a scenario that draws at random from the rows, and must play the same after such a change, asks for `"order": "ref"`: by how the protocol spells each entity, `@id`s by name and then the rest by number.

### Reacting to an event

One system emits; another, later in the tick, reads `q:events`. This one runs in `late`, so it sees every `sim` system's events:

```lua
return system "late.tally" {
  stage = "late",
  writes = { "Tally" },
  doc = "Counts this tick's spark.spawned events into every Tally.",
  run = function(q)
    local n = #q:events("spark.spawned")
    if n == 0 then
      return
    end
    for _, tally in q:each("Tally") do
      tally.sparks += n
    end
  end,
}
```

The engine's events work the same way. Two colliders that start to touch give an `overlap.begin` event with `entity` and `other`, from the `physics.overlap` system, so a pickup runs after it. From `templates/platformer/systems/pickups.luau`:

```lua
return system "sim.pickups" {
  stage = "sim",
  after = { "physics.overlap" },
  reads = { "Hero", "Coin" },
  writes = { "Hero", "Coin", "Sprite" },
  doc = "Takes a coin the hero touches (Coin.taken, Hero.coins, coin.taken {entity, coins}) and draws only the coins not taken.",
  run = function(q)
    local hero = q:entity("@player")
    local h = hero:get("Hero")
    for _, ev in ipairs(q:events("overlap.begin")) do
      local other = nil
      if ev.entity == hero then
        other = ev.other
      elseif ev.other == hero then
        other = ev.entity
      end
      if other ~= nil and other:has("Coin") and not other:get("Coin").taken then
        other:get("Coin").taken = true
        h.coins += 1
        q:emit("coin.taken", { entity = other, coins = h.coins })
      end
    end
    -- A coin is drawn while it is there to take.
    for _, c, s in q:each("Coin", "Sprite") do
      if s.visible == c.taken then
        s.visible = not c.taken
      end
    end
  end,
}
```

An entity field in an event is an entity handle, so `ev.entity == hero` compares entities. `ev.name` is the event's own name, so a loop over every event (`q:events()`) can tell them apart; no field may be called `name`, and `q:emit` refuses one with an error that says so (call it `who`, `title`, or `what`). To see what fields an engine event carries, run the game and ask: `argus events name=overlap.begin limit=1`.

### A pause

`q:freeze()` stops the rest of the tick for every system but those marked `unfrozen`. The pause system itself is unfrozen, so it still sees the next press. From `templates/platformer/systems/pause.luau`:

```lua
return system "input.pause" {
  stage = "input",
  after = { "input.actions" },
  unfrozen = true,
  reads = { "Actions", "UiNode" },
  writes = { "UiNode" },
  doc = "Shows or hides the pause panel (@pause) on the pause action (pause.opened, pause.closed), and freezes the rest of every tick while it shows.",
  run = function(q)
    local panel = q:entity("@pause"):get("UiNode")
    if q:entity("@player"):get("Actions").pause_pressed then
      panel.visible = not panel.visible
      q:emit(panel.visible and "pause.opened" or "pause.closed", {})
    end
    if panel.visible then
      q:freeze()
    end
  end,
}
```

`q.time` keeps counting through frozen ticks. A game that needs paused time to stand still keeps its own clock in a component.

### Turns

On a discrete clock (`clock = "discrete"` in `scene.toml`), `run` is called once a turn: each press of a bound action, or each `argus run turns=1`. `on_load = true` also runs a system once before the first turn, which is how `examples/delve/systems/floor.luau` generates the first floor. A small one:

```lua
return system "sim.turns" {
  stage = "sim",
  writes = { "Tally" },
  on_load = true,
  rng = "turns",
  doc = "Counts turns into every Tally, once when the scene loads too.",
  run = function(q)
    for _, tally in q:each("Tally") do
      tally.sparks += 1
    end
    q:emit("turn.counted", { tick = q.tick, roll = q:rng():range(1, 6) })
  end,
}
```

A press that should not cost a turn (opening a menu) freezes the rest of that turn: see `examples/delve/systems/menu.luau`.

## Sharing code

A module is a Luau file in `lib/` that returns a value, usually a table of constants and functions. Any script file asks for it with `require` and its path in `lib/`, without `.luau`: `lib/clock.luau` is `require("clock")`, and `lib/rules/checks.luau` is `require("rules/checks")`. Outpost's HUD and inspector both show the colony's hour, so the hour is worked out in one module:

```lua
-- The colony's clock: the hour of its day on a 24-hour clock, as the
-- light shows it. The HUD and the inspector both show it.
local M = {}

-- The colony's hour at t, a share of its day, with night falling at dusk.
function M.hour(t, dusk)
  local h = if t < dusk then 5 + 14 * t / dusk else 19 + 10 * (t - dusk) / (1 - dusk)
  return math.floor(h) % 24
end

return M
```

That is `examples/outpost/lib/clock.luau`, shortened. The HUD (`systems/hud.luau`) and the inspector each start with `local clock = require("clock")` and call `clock.hour(t, rules.dusk)`. Outpost's `lib/ui.luau` is the other kind of module, helpers that build things: the menu and the inspector make their panels and buttons with it, each passing its own `q`.

How modules behave:

- **One value, frozen.** A module is evaluated once per load, and every file that requires it gets the same value. That value, and every table in it, is read-only: `clock.hour = nil` fails with "attempt to modify a readonly table", so one system cannot change what another sees.
- **The same sandbox.** A module has what a system file has and nothing more, and the time its functions take counts against the calling system's instruction budget. It reads the world only through what it is handed: `ui.button(q, parent, "menu.resume")` takes the system's `q`.
- **No state.** A module's file-level locals are like a system file's: not part of the world, so a seek, a replay, a save, or a test does not put them back. Keep only constants and functions there, and state in components.
- **Its first comment is its description.** The `--` lines before its code are what ARGUS.md and `argus script modules` say about it, with the files that use it.
- **Errors say where.** A missing module names the files it looked for and the modules there are. Modules that require each other in a circle are refused, naming the circle. An error inside a module is reported at the module's file and line, once, and each file that required it fails to load saying "lib/x.luau did not load".
- **Reload follows it.** `argus reload` with a module changed reloads every file that uses it, directly or through another module, even files that did not change; the answer's `modules` names the changed modules and `reloaded` every file evaluated again. A module that no longer compiles keeps its users on their last version, with its error in `argus script errors`.
- **Mods add, not replace.** A mod's files look in its own `mods/<id>/lib/` first, then in the game's `lib/`; the game's files look only in the game's. A mod can add modules of its own, but cannot change one the game's systems use.
- **Release builds carry it.** `argus build` compiles `lib/` into the packs with the other scripts.
- **Each file has its own 200 locals.** Luau allows about 200 local names at a file's top level, its `local function`s included (constants it can fold away do not count). A file past it does not load: `argus script errors` names it and the line ("Out of local registers"), and a system that runs after one of its systems is refused with that file's error. A module's top level has its own 200, so moving helpers into `lib/` makes room. A file that copies a module's functions into locals (`local base_at = room.base_at`) spends a local on each; calling through the module (`room.base_at(...)`) spends none.

Scenarios can share code too: their `require` looks in `tests/lib/` first, then in the game's `lib/`, so a test can use the game's constants instead of copying them ([Testing](testing.md#the-file)).

## The sandbox

Scripts run inside the daemon, sandboxed, so a mistake cannot take the game down and every run can be replayed. What that means for you:

- **Globals are read-only** once a file has loaded. `counter = 1` inside `run` fails with "attempt to modify a readonly table".
- **Each run has an instruction budget**: 5,000,000 of the VM's checks, which it makes at each function call and each turn of a loop. A loop that never ends is stopped ("instruction budget exceeded") and its system switched off. Work inside the string library counts too: a pattern that backtracks (the usual trim, `string.match(s, "^%s*(.-)%s*$")`, on a long run of spaces) can spend the budget on one call. `pcall` cannot catch running out: the call it wraps stops, and the next check outside it stops the run. `argus profile systems=true` gives each script system's checks, its last run's and its most (`checks`, `checks_max`, against `checks_budget`), beside its milliseconds. The Luau heap may hold 256 MB.
- **Strings are bytes.** `#s` and `string.sub` count bytes, so a cut at a byte count can fall inside a character such as "é". Text written from a script (a `str` field, an event's field, `q:send`) keeps such a cut character as U+FFFD. To cut whole characters, count with the `utf8` library: `string.sub(s, 1, (utf8.offset(s, 1001) or #s + 1) - 1)` keeps the first 1,000.
- **What a script has**: Luau's base functions (`assert`, `error`, `pcall`, `xpcall`, `select`, `type`, `typeof`, `tostring`, `tonumber`, `ipairs`, `pairs`, `next`, `unpack`, `setmetatable`, `getmetatable`, and the `raw` ones), and the libraries `math`, `string`, `table`, `vector`, `coroutine`, `bit32`, `utf8`, and `buffer`. `print` writes to the daemon's log, `.argus/daemon.log`; to see a value, emit it (`q:emit`) and read it with `argus events`. `tonumber` is Luau's: it takes `"nan"`, `"inf"`, `"1e999"`, and `"0x10"` as numbers, so a game that reads numbers from outside text (a model's answer) checks the range itself.
- **What it does not have**: `io`, `os`, `load`, `loadstring`, and `dofile`. `require` reads only the project's modules, in `lib/` ([Sharing code](#sharing-code)): a game's scripts read no other files, because a release build has only its packs. Values a designer tunes, which several systems share, go in a def table (`q:def`).
- **Keep all state in components.** A `local` at the top of a file is shared by every run of the system, but it is not part of the world: a seek, a replay, a save, or a test does not put it back. In a test of this, a counter kept in a file-level local read 30 after seeking back to frame 10 of a 20-tick run. Use file-level locals only for constants and helper functions.
- **Read time from `q`, and random numbers from `q:rng()`.** There is no clock and no date to read, and `math.random` fails with an error that says what to use: give the system a stream (`rng = "loot"`) and call `q:rng():next()`, `q:rng():range(lo, hi)`, or `q:rng():chance(p)`. A number that differed between a run and its replay would break every seek, replay, mark, and test. (The first builds let `math.random` and `os` through; the manual's writers found it.)

The reason behind all of these is [determinism](concepts.md#determinism-same-seed-same-run): a run is only reproducible if everything it depends on is in the world.

## How errors show up

An error never crashes the daemon. Every one comes back with a code, a sentence, a hint, and a file and line.

| when | what you see | what happens |
|---|---|---|
| a syntax error, on `argus reload` | a row in the reload's `errors` with phase `syntax` | the file's previous version keeps running |
| a component's fields changed, on `argus reload` | phase `declaration`: "already registered with a different schema" | refused; `argus scene load` starts the scene with the new fields |
| an undeclared read or write, while running | 4301 "system sim.gems reads Score without declaring it" | that system is switched off; the rest run on |
| a value out of range | 4102 "Spark.ttl must be in range [0, 60]" | that system is switched off |
| a stale handle | 4502 "handle used outside its system's run" | that system is switched off |
| a runaway loop | 4503 "instruction budget exceeded" | that system is switched off |

A system switched off by a fault is listed under `disabled` in `argus summary`, and the fault is a `system.fault` event. A file that did not load is in `argus summary`'s `script_errors`, with the first few errors, and every answer that runs the game (`argus run`, `sim.step`) counts them in its `script_errors`. A system that runs after one in a file that did not load is refused with that file's error, not its own. Find them all with:

```sh
argus script errors
```

```
file               frame  line  message                                                       phase  ref                   system
-----------------  -----  ----  ------------------------------------------------------------  -----  --------------------  --------
systems/gems.luau  0      10    [4301] system sim.gems reads Score without declaring it Hint: Add "Score" to the system's reads list. (systems/gems.luau:10)  run    systems/gems.luau:10  sim.gems
```

Fix the file and run `argus reload`: the system runs again from the current frame. A reload switches every faulted system on again, its file changed or not, since a fault can come from the run's data (an outside service's answer) rather than the code; the reload's `switched_on` names them. `argus events name=system.fault` shows when each fault happened.

## How reload picks changes up

Scripts are read when the scene loads, and again when you ask:

```sh
argus reload
```

It recompiles the changed Luau files and swaps their systems in while the world goes on from the current frame: entities, the tick, and the random streams are kept. A changed module in `lib/` reloads every file that requires it along with it. A deleted file's systems go. Assets are imported again at the same time, and Rust plugins rebuilt. An AI agent calls the same thing as the `reload` tool. A script saved on disk waits until something asks; the running game does not watch for it. (Assets are different: the importer looks for changed files twice a second by itself.)

Two kinds of change need the scene loaded again instead, with `argus scene load`:

- **A component whose fields changed.** A running world's rows cannot change shape. The scene starts over from its files with the new fields.
- **Entity and scene files** you edited by hand. `reload` leaves the live world's entities as they are; `scene load` reads them again. (Changes made with `argus edit` apply at once and need neither.)

`argus test` loads each scenario's scene fresh, scripts included, so it always tests what is on disk.

## Where to go next

- [Scenes](scenes.md): the entity files your systems work on.
- [Gameplay](gameplay.md): input, physics, navigation and jobs, events, randomness, timers and tweens, saves.
- [Testing](testing.md): pinning each system's behaviour with scenarios.
- [Tuning and performance](tuning-and-performance.md): `tune` fields, sliders, and `argus profile`; when a hot system should move to a Rust plugin.
- [Reference](reference.md): finding any function in `argus docs`.
