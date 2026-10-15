# Gameplay

This chapter covers the engine's gameplay pieces and how a game uses them: input, movement and collision, rigid bodies, navigation, jobs, events, randomness, timers and tweens, pausing, and saves. Each part names the components and script calls involved and points at a real example you can open.

Everything here is data on entities. A game turns a piece on by giving an entity its component, and reads or writes the component's fields from a Luau system (see [scripting](scripting.md)). The full field lists are in the reference: `argus docs`, or `argus describe <Component>` for one component (see [reference](reference.md)).

## Input

Gameplay never sees a key. The game names its **actions** in `input.toml` at the project's root, binds keys, buttons, and sticks to each, and scripts read the actions by name. Because of this a player can rebind keys, a test can press "jump" without knowing which key that is, and a replay survives a rebind.

### Actions and their types

Each action is a table under `[actions.<name>]`. Its type decides what a script reads:

| `type` | What the `Actions` component gets | Example |
|---|---|---|
| `button` (the default) | three bools: `jump` (held), `jump_pressed` (went down this tick), `jump_released` (went up this tick) | jump, pause, talk |
| `axis` | one number from -1 to 1 | zoom |
| `axis2` | a vector no longer than 1 | move, pan |
| `pointer` | a vector: the mouse's place on the UI canvas, in its pixels; `q:screen_to_world(p)` gives it in the world | pointer |

From `examples/hollow/input.toml` (its jump's `type` and `doc` left out; a table with no `type` is a button):

```toml
[actions.move]
type = "axis2"
doc = "Run left and right; down with jump drops through a plank"
[actions.move.bindings]
keyboard = ["wasd", "arrows"]
gamepad = ["left_stick", "dpad"]

[actions.jump]
[actions.jump.bindings]
keyboard = ["space", "z"]
gamepad = ["south"]

[slots.1]
devices = ["keyboard", "mouse", "gamepad:0"]
```

The `doc` string shows in the project's ARGUS.md and in a controls menu.

### Bindings

Each action's `bindings` table has a list per device:

- `keyboard`: key names such as `a` to `z`, `0` to `9`, `space`, `enter`, `escape`, `up`, `shiftleft`, `f1`. An `axis2` also takes `wasd`, `arrows`, `ijkl`, or four keys in the order up, left, down, right. An `axis` takes `"neg/pos"`, for example `"minus/equal"`.
- `mouse`: `left`, `right`, `middle`, `back`, `forward` for a button; `position` for a pointer. A button may carry modifiers: `mouse = ["ctrl+left"]` is pressed by a left click with Ctrl held (`ctrl`, `shift`, `alt`, `meta`, joined with `+`). While it is pressed, a plain `left` binding stands down, so Ctrl and a click is one action, not two, and the Ctrl key need not be bound (on a discrete clock it takes no turn).
- `gamepad`: `south`, `east`, `west`, `north` (A, B, X, Y on an Xbox pad), `lb`, `rb`, `lt`, `rt`, `start`, `select`, the d-pad buttons, and for axes `left_x`, `left_stick`, `dpad`, and so on.
- `platform`: `overlay`, held while the platform's overlay (Steam's Shift+Tab) is open. Hollow binds its pause to it.
- `touch`: a touch control's name, from the file's `[touch]` tables (see [Touch](#touch) below): a button control for a button, a stick for an `axis2`, a stick's `x` or `y` for an `axis` (`"stick.x"`).

Gamepad and mouse names are checked when the project opens. Keyboard names are not: a name no key sends is accepted and never fires, so check a new binding by pressing it. The full name lists are in the `input.toml` section of `argus docs`.

A button with `toggle = true` turns on with one press and off with the next, instead of being held.

### Touch

A game plays under a finger with nothing added: a finger that touches the screen is the mouse, its left button down where it lands and the pointer following it, so menus, the engine's click, and `q:screen_to_world` work on a phone or a tablet as they do with a mouse. That covers a game played by clicking. A game played with keys needs controls on the screen: buttons and sticks, declared in `input.toml` and bound to actions as keys are.

```toml
[actions.move.bindings]
keyboard = ["wasd", "arrows"]
touch = ["stick"]

[actions.jump.bindings]
keyboard = ["space"]
touch = ["jump"]

# A stick at the bottom left, 170 across, its centre 130 in from each edge.
[touch.stick]
kind = "stick"
anchor = "bottom-left"
at = [130, 130]
size = 170
floating = true

[touch.jump]
kind = "button"
anchor = "bottom-right"
at = [110, 110]
size = 120
```

- A control is placed on the UI canvas from its `anchor` (a corner, the middle of an edge, or `center`) by `at`, its centre measured inward from that anchor, and `size`, its diameter, in the canvas's pixels. So it stays in its corner on any screen. On a screen wider than the game's canvas, such as a phone on its side, the anchors are the screen's own edges, past the canvas's, so the controls sit where the thumbs are. A floating stick then takes its half of the whole screen.
- A finger that lands on a control holds it until it lifts. A stick gives its `axis2` the drag from its centre, as far as its edge being 1 and up positive, as a gamepad stick does. A `floating` stick takes a finger landing anywhere in its half of the screen and centres itself where the finger landed, which suits thumbs.
- A finger that lands on no control is the mouse, as above, and a second one there does nothing. Several fingers can hold several controls at once.
- The engine draws the controls over the game in the theme's colours while touch is the device the player last used: the `device` action is 5, and button prompts say what the touch buttons are called. A key, a click, or a pad hides them again. Give a control `draw = false` to draw it yourself.
- A slot takes touch when its `devices` list `touch` or `mouse`.
- What a finger presses is recorded as the actions it pressed, as a key is, so replays, seeks, and players' reports have it.

Hollow is played this way on a phone. Its `input.toml` has a floating stick at the bottom left for `move`, Jump and Talk buttons at the bottom right, and a small Menu button at the top right that opens the pause menu. Its `tests/touch.luau` walks and jumps the hero with two fingers.

To try it without a phone, send fingers with `argus input device events='[{touch: 1, phase: start, at: [130, 410]}]'` (the at is in the UI canvas's pixels), or from a scenario with `t.input:touch(1, "start", 130, 410)`. The semantic frame lists the controls on the screen under `touch`. A web build's page hands fingers to the game, and stops the browser scrolling or zooming under them; [Releasing](releasing.md) has how to play it on a phone.

### Slots and players

A **slot** is one player. `[slots.<n>] devices` says which devices feed slot n (1 to 8). Without any `[slots]` table there is one slot fed by the keyboard alone, so list `mouse` and a gamepad when you want them. `gamepad:0` is the first pad connected, `gamepad:1` the second; `gamepad` alone is every pad. `touch` is a touchscreen, which goes with the mouse: a slot that lists `mouse` takes the fingers too.

An entity receives a slot's actions when it has `[PlayerSlot] slot = 1` and an empty `[Actions]` table, as Hollow's hero does (`examples/hollow/scenes/hollow/rooms/persistent/player.toml`). The input stage writes the slot's actions onto it every tick.

For couch co-op, give a second slot its own pad and a second entity `slot = 2`:

```toml
[slots.1]
devices = ["keyboard", "gamepad:0"]

[slots.2]
devices = ["gamepad:1"]
```

### Reading actions in a script

A system that declares `reads = {"Actions"}` reads the fields by the action's name. From `templates/blank/systems/move.luau`:

```lua
return system "sim.move" {
  stage = "sim",
  reads = { "Actions", "Player" },
  writes = { "Transform" },
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

Use `jump_pressed` for "do it once when the button goes down" and `jump` for "while it is held". Note that on an `axis2`, y is up: `{0, -1}` is down, also in a menu.

`Actions` also has the engine's own `device` field: the kind of device the player last used (0 keyboard and mouse, 1 Xbox, 2 PlayStation, 3 Switch, 4 another pad). Button prompts follow it (see [presentation](presentation.md#ui)).

Some engine parts read actions by name unless you tell them otherwise:

| Who | Actions it reads |
|---|---|
| A `CharacterController` | `move` and `jump` (or its `move_action` and `jump_action`) |
| A `UiRoot` (menus) | `ui_move` (axis2), `ui_confirm`, `ui_cancel`, `ui_click` (buttons), `pointer` |
| A `Camera` | the actions its `pan_action` and `zoom_action` name |
| A conversation's box | the same `ui_*` actions as a menu |

So a game with menus or dialogue needs the `ui_*` actions in its `input.toml`. Hollow, Delve, Swarm, and Outpost all have them.

### Rebinding

Rebinding is a UI over the same table. A controls menu has buttons with an `InputBinding` component: pressed, it waits for the next key and binds the action to it. From outside the game, `argus input bind action=jump bindings=[x]` rebinds for the session, and `save=true` also writes `input.toml`. A shipped game keeps a player's rebinds in their `settings.toml`. See [content](content.md#accessibility) for the controls menu.

### How a key press becomes a turn

On a **continuous** clock (`continuous@60`, the default) the sim ticks 60 times a second whether or not anyone presses anything, and a script sees the actions as they are on each tick.

On a **discrete** clock (`clock = "discrete"`, as in Delve) the sim stands still until something asks for a turn. While the game is playing, each press of a bound key passes one turn, and that turn's systems see the press. A release passes no turn, and neither does a key that is bound to nothing. A mouse move passes no turn either: the `pointer` holds its place, and the next turn (a click's, or a key's) reads it there. The page and the window take the mouse's moves over the game whatever has the keyboard's focus. A scenario's `t:advance`, `argus run turns=1`, and `sim.advance` also pass turns. `argus input inject` on a discrete clock only queues the action for the next turn; it does not pass one by itself.

On a discrete clock `q.dt` is 1 and `q.time` counts turns.

**Between turns.** A turn-based game still moves while it waits. Animators, particles, tweens, the camera, timelines, and the post effects move *between* turns. A script's system joins them with `between_turns = true`. On a turn these systems react to what the turn changed, but no time passes for them (`q.dt` is 0). Between turns, while the game is playing in a window or a browser, idle ticks run at the clock's rate (60 a second): only these systems run, with `q.dt` 1/60, and no turn passes. So a slide that a turn starts plays out after the turn, and the hero idles while the player thinks.

Idle ticks are part of the run. The ones between two turns are recorded with the input, so a seek back, a replay, or a bug report runs the same ones before the turn, and the game's world comes out the same. A headless run (a test, an agent's `argus run turns=1`) has no time between turns unless it asks: `t:idle(seconds)` in a scenario, `sim.idle {seconds}` from outside. Keep gameplay off what moves between turns, since how long a player waits is theirs: a turn's outcome should not depend on whether a slide finished.

A press the game should not count as a turn (the one that opens a menu) is handled with `q:freeze()`. From `examples/delve/systems/menu.luau`:

```lua
local toggled = q:entity("@player"):get("Actions").pause_pressed
if toggled then
  menu.visible = not menu.visible
end
if menu.visible or toggled then
  q:freeze()
end
```

## Movement and collision

### Transform and the hierarchy

`Transform` is an entity's place: `pos` (a vector in world units, y up), `rot` (radians, counter-clockwise), and `scale`. Writing `pos` moves the entity, as `sim.move` in the input section does.

An entity with a `Parent` has a Transform local to its parent. The hierarchy stage works out its world place each tick into `GlobalTransform`, which you read and never write. In an entity file, a child is either a `[children.<name>.<Component>]` table or a separate file with `[Parent] entity = "@id"`. Hollow's lantern (`examples/hollow/scenes/hollow/rooms/persistent/lantern.toml`) is `[Parent] entity = "@player"` with `[Transform] pos = [0.3, -0.1]`, so it is carried at the hero's side wherever the hero goes.

A script that spawns a child adds `GlobalTransform` itself: `q:spawn({"Transform", "Parent", "GlobalTransform", "Sprite"})`, then set `Parent.entity`. When a parent is despawned, the link is cleared.

### Three tiers of physics

Physics comes in three tiers, chosen per entity. All three share one `Collider` shape, centred on the Transform plus `offset`:

- a box: `size`;
- a circle: `radius` above 0;
- a capsule standing up: a `radius` and a `height` above twice it (`height` is the whole of it, ends included);
- a convex polygon: `points`, its corners around the centre, at least three. A `radius` rounds its corners. Points that are not convex are taken as the shape around them.

```toml
[Collider]
points = [[-1.0, -0.5], [1.0, -0.5], [0.0, 1.0]]
```

A rigid body's shape turns with it. The overlap tier and the queries take every shape upright, whatever the Transform's rotation. A kinematic body always moves as a box: a capsule or a polygon on one moves as the box around it.

| Tier | Components | What it does | Example |
|---|---|---|---|
| Overlap | `Collider` alone | Reports touches as events, and nothing else. A trigger, a pickup. | Hollow's gem and exit, the platformer template's coins |
| Kinematic | `Collider` + `KinematicBody` | Moves by its velocity against tilemap collision and solid bodies. | Hollow's hero, the platformer template's ground |
| Dynamic | `Collider` + `RigidBody` | Real physics with gravity, contacts, and joints. Needs the `rigid` module. | `examples/toybox` |

Two colliders meet only when each one's `mask` has a bit of the other's `layer`. By default every collider meets every other.

### Kinematic bodies

A `KinematicBody` is an **actor** by default: it stops at walls, lands on floors, steps up small ledges, and rides moving platforms. With `solid = true` it is a **solid**: it goes where its velocity takes it and blocks, carries, or pushes actors. A solid with `one_way = true` blocks only from above. Every block of the platformer template's level is `[Collider]` plus `[KinematicBody] solid = true` (`templates/platformer/prefabs/platform.toml`). Give a solid a `velocity` and it is a moving platform.

Kinematic bodies also collide with every tilemap's collision layers. Tiles are solid, one-way, or sloped by their tileset frame's properties (`solid`, `one_way`, `slope`), and a collision layer with no tileset is solid wherever it has a tile (Delve's walls). See [scenes](scenes.md) for tilemaps.

`physics.move` writes what happened back into the body: `grounded`, `ground`, `wall` (-1, 0, or 1), `ceiling`, `slope`, and `on_one_way`. Animators and scripts read these.

### The character controller

A `CharacterController` turns `move` and `jump` into a platformer's run and jump on the entity's KinematicBody. It handles gravity, coyote time, a jump buffer, a shorter hop when jump is let go early, corner correction, step-up, and dropping through one-way platforms (down and jump). From Hollow's hero:

```toml
[Collider]
size = [0.75, 1.6]

[KinematicBody]
velocity = [0.0, 0.0]

[CharacterController]
run_speed = 7.0
jump_height = 3.2
jump_time = 0.36
```

The jump is set by its height and its time to the top, not by a gravity number: the controller works out gravity and take-off speed from them. The fields worth tuning first:

| Field | Default | What it is |
|---|---|---|
| `run_speed` | 7 | Top running speed, units a second |
| `ground_accel`, `ground_decel`, `air_accel`, `air_decel` | 70, 90, 45, 25 | How fast it speeds up and slows down |
| `jump_height`, `jump_time` | 3.2, 0.4 | A full jump's height and seconds to the top |
| `fall_gravity` | 1.7 | Gravity multiplier while falling |
| `jump_cut` | 0.45 | Upward speed kept when jump is let go early |
| `coyote_time`, `jump_buffer` | 0.1, 0.12 | Seconds of grace after a ledge and before landing |
| `corner_correction`, `step_height` | 0.25, 0.3 | How far it slides round corners and steps up |

All of these are tune fields, so you can change them while the game runs (see [tuning and performance](tuning-and-performance.md)). Without an `Actions` component the controller reads `move_x`, `move_y`, and `jump` as a script wrote them, which is how an AI-driven character could use it.

A system ordered after `physics.control` and before `physics.move` sees this tick's jump and velocity and may change them. Hollow's double jump (`examples/hollow/systems/feather.luau`) is such a system: `after = { "physics.control" }`, and it sets `KinematicBody.velocity` and `CharacterController.rising` itself.

### Triggers and contacts

When two colliders start touching, `physics.overlap` emits `overlap.begin {entity, other}`; when they stop, `overlap.end`. It runs at the end of the sim stage, so a system that reacts to touches says `after = { "physics.overlap" }`. Either entity may be first in the pair. From `templates/platformer/systems/pickups.luau`:

```lua
return system "sim.pickups" {
  stage = "sim",
  after = { "physics.overlap" },
  reads = { "Hero", "Coin" },
  writes = { "Hero", "Coin", "Sprite" },
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
  end,
}
```

When you do not need events, ask the spatial index directly. `q:within(pos, r, "Enemy")` loops over entities near a point, nearest first; `q:nearest(pos, r, "Enemy")` gives the closest one. Swarm's bullets use `q:nearest` (`examples/swarm/systems/hits.luau`). A sim system sees positions as they were at the end of the last tick.

### Asking what is there: rays and shapes

A system can ask what a line or a shape meets, now, without a body moving: line of sight, a hitscan shot, a ledge ahead, room to stand up.

```lua
local hit = q:raycast(eye, target, { ignore = me, mask = 1 })
if hit and hit.entity == target then
  -- nothing in the way
end
```

- `q:raycast(from, to, opts)` is the nearest thing the line from `from` to `to` meets, or nil. A hit has `entity` (a collider's) or `tile` (`{map, x, y}`, a Tilemap's cell), `point`, `normal` (the surface's, pointing out), `distance`, and `fraction` (0 at `from`, 1 at `to`). `q:raycast_all` is every hit, nearest first.
- `q:shape_cast(shape, from, to, opts)` is where a shape moving from `from` to `to` first touches something: the hit's `position` is where the shape stops, its `point` where they touch. A shape is `{ radius = 0.5 }` (a circle), `{ size = vector.create(1, 2, 0) }` (a box), `{ radius = 0.3, height = 1.6 }` (a capsule standing up), or `{ points = { ... } }` (a convex polygon around the shape's origin).
- `q:overlap(shape, at, opts)` is everything the shape overlaps when it is at `at`: a point is `{ radius = 0 }`.

They meet every active `Collider` and every Tilemap's collision cells: solid cells, slopes (solid under their line), and one-way platforms, which stop only what comes down onto their top (and nothing at rest, for `q:overlap`). Touching is not overlapping: a box resting on the floor is not stopped by it as it slides along. `opts` takes `mask` (layer bits, as a collider's `mask`), `ignore` (an entity or a list, usually the asker), `one_way = false` (one-way platforms are not there), and `tiles = false` or `colliders = false`. Points are vectors, tables, or entities (their positions). The system declares `reads = { "Collider", "Tilemap" }`, or leaves the one it does not read out with its option. The geometry is exact and uses nothing but the four operations and square roots, so a query answers the same on every machine. To see what a query found, draw it: `q:draw_line(eye, hit.point)` (see [debug drawing](presentation.md#debug-drawing)).

The kinematic tier also reports what happened to bodies: `body.jumped {entity, coyote, buffered}` and `body.dropped {entity}` from the controller, and `body.landed {entity, speed}`, `body.wall {entity, side}`, `body.ceiling {entity}`, `body.corner_corrected {entity, dx, dy}`, and `body.crushed {entity, by}` from the move.

## Rigid bodies

Real rigid-body physics (stacks, pendulums, rope bridges, balls that bounce) is an optional module built on the Rapier library. Turn it on in `argus.toml`:

```toml
modules = ["rigid"]
```

Then `RigidBody`, `Joint`, and `RigidWorld` exist. A `RigidBody` uses the entity's `Collider` as its shape. It is dynamic, unless `fixed = true` (it never moves) or `kinematic = true` (it moves by its velocity and pushes others). From `examples/toybox/scenes/toybox/`:

```toml
# ground.toml
[Collider]
size = [40.0, 2.0]

[RigidBody]
fixed = true

# hinge.toml: a pendulum's bob hinged to a point in the air
[Joint]
a = "@bob"
kind = "revolute"
anchor_a = [-3.0, 0.0]
anchor_b = [8.0, 12.0]
```

One entity's `RigidWorld` sets the gravity and solver for all of them (Toybox's `world.toml` has `gravity = [0.0, -15.0]`). A joint's `kind` is `revolute` (a hinge), `fixed`, `prismatic` (a slider), `rope`, or `spring`. With no `b`, the joint ties `a` to a point in the world. A joint with `break_force` above 0 breaks under that force.

A script throws a body by writing `RigidBody.velocity` and teleports it by writing its Transform. Contacts are `contact.begin {entity, other, impulse}` and `contact.end` events, and a broken joint is `joint.broken {joint, a, b, force}`. Rigid bodies also show up in `overlap.begin`.

Limits to know about: the kinematic tier does not see rigid bodies (a character pushes a crate but is not stopped by one), a one-way tile is solid to a rigid body, and a save keeps the bodies' components but not Rapier's contact state, so contacts are found again after a load.

## Navigation

Navigation runs on a **grid**. A `NavGrid` entity lays cells over the world. Each cell is blocked or open with a step cost. The grid can cover a tilemap (its solid and sloped tiles are blocked) or be a plain rectangle. `NavCost` entities on top block cells (`cost = 0`) or make them slower.

From `templates/colony/scenes/main/ground.toml` and `pond.toml`:

```toml
# ground.toml: a 32 by 18 meadow, a cell a unit
[NavGrid]
origin = [-0.5, -0.5]
width = 32
height = 18
diagonal = true

# pond.toml: water nobody walks through
[NavCost]
cost = 0
size = [5.0, 4.0]
```

Delve's grid covers its tilemap instead: `[NavGrid] tilemap = "@floor"`.

### Walking an entity

A `NavAgent` walks its entity along a path found with A*. It goes to `goal` (a place), or to `target` (an entity, followed as it moves), or down a flow field (`field`). It moves at `speed` units a second and writes `state` (`moving`, `arrived`, or `blocked`). It emits `nav.arrived {entity}` and `nav.blocked {entity, why}`.

The goal's default is (0, 0), and an agent walks there from its first tick. An agent that should stand still until it has somewhere to be gets its own position as its goal, as each colonist in `templates/colony/scenes/main/` does (Ada's file has `[Transform] pos = [14.0, 6.0]` beside `[NavAgent] goal = [14.0, 6.0]`). A script sends it somewhere by writing `goal`; `templates/colony/systems/idle.luau` strolls idle colonists that way, checking `q:walkable(to)` first.

### Paths from scripts

A system that declares `reads = {"NavGrid"}` can ask for paths itself:

- `q:path(from, to)`: the waypoints and the cost, or nil when there is no way.
- `q:walkable(pos)`: whether a point is on an open cell.

### Flow fields and crowds

Many entities chasing one target should share a **flow field** instead of each finding its own path. Put a `NavField` on the target. It keeps every cell's distance to the target's cell, found again when the target moves to another cell. Delve's hero has one (`examples/delve/scenes/delve/player.toml`):

```toml
[NavField]
grid = "@floor"
```

Then `q:distance(field, pos)` gives the steps from a place to the target, and `q:downhill(field, pos)` the next cell's centre toward it. Delve's monsters compare the distance of each neighbouring cell and step to the smallest (`examples/delve/systems/monsters.luau`). A system using these declares `reads = {"NavField"}`. A `NavAgent` with `field` set walks down the field by itself.

To look at a grid from outside the game: `argus nav grid` draws it as characters, `argus nav path from=@player to=[10,4]` finds a path, `argus nav field of=@player` draws a field, and `argus nav bake` writes a grid to a file. See [testing](testing.md) for `t.expect:path`.

## Jobs and workers

A colony game posts work and lets colonists take it. The engine's job system does the assigning, walking, and counting.

- A **Job** is an entity: a `kind`, a `priority`, the skill it `requires`, a `site` where it is worked, seconds of `work`, and optionally an entity to `carry` somewhere (`to`) after.
- A **Worker** has `skills` (words such as `"farm cook"`; `*` is every skill) and a work `rate`. It needs a `NavAgent` to walk.
- A **JobKind** gives every job of one kind its starting priority, skill, and work. Its priority is a tune field, so a player or an agent can reweigh the colony while it runs.

Each tick `jobs.update` gives every free worker the best open job it can do: highest priority first (the job's own plus its kind's), then the nearest, and never one it cannot reach over the grid. It walks the worker there, counts the work, carries what the job carries, then emits `job.done` and despawns the job. A job with `only` set is for that one worker (a meal, a night's sleep), and if it outranks what the worker is doing, it takes the worker off it.

A game posts a job by spawning it. From `examples/outpost/systems/fields.luau`:

```lua
local j = q:spawn({ "Job" })
local job = j:get("Job")
job.kind = "harvest"
job.site = e
f.job = j
```

And it makes the result happen on `job.done`, which carries `job`, `kind`, `worker`, `site`, `to`, and `carry`. From `examples/outpost/systems/work.luau`, run with `after = { "jobs.update" }`:

```lua
for _, ev in ipairs(q:events("job.done")) do
  if ev.kind == "harvest" and ev.site ~= nil and ev.site:alive() then
    local f = ev.site:get("Field")
    f.growth = 0
    if f.job == ev.job then f.job = nil end
  end
end
```

Handle `job.done` right after `jobs.update` and before the systems that post jobs, so a finished job is forgotten before anything looks for a new one. Outpost found that bug the hard way (`examples/outpost/systems/work.luau` explains it).

Other job events: `job.claimed`, `job.started`, `job.carrying`, `job.released`, `job.cancelled`, and `job.idle {worker, why}`. A worker with no job says why in `Worker.idle`. To ask why a colonist does what it does, run `argus jobs why worker=@colonist.ada`. The queue itself is a query: `argus query group_by=Job.kind select=[count()]`.

## Events

An event is a named record of something that happened on a tick. The engine emits many (named through this chapter). A game emits its own with `q:emit`, and the name is the game's to choose. From `examples/delve/systems/player_turn.luau`:

```lua
q:emit("potion.drunk", { healed = after - before, hp = after, max = hp.max, left = pl.potions, turn = turn.n })
```

Fields are numbers, strings, booleans, entities, and vectors. Every event goes into the event log, which `argus events name=potion.drunk` reads and a scenario checks with `t.expect:events`.

A system reacts with `q:events(name)`: the events with that name emitted **so far this tick**, oldest first. From `examples/delve/systems/messages.luau`, which runs `after = { "sim.monsters", "sim.potions" }`:

```lua
for _, ev in ipairs(q:events("potion.picked")) do
  table.insert(first, q:text("message.potion_picked", { count = ev.carried }))
end
```

Three rules follow from "this tick, so far":

1. Order the reader after the writer with `after`. An event emitted later in the tick is never seen by a system that already ran.
2. An event from an earlier tick is gone. Keep anything that must last in a component.
3. Mind the stage. `overlap.begin` comes at the end of the sim stage. Animator frame events and a tween's `on_complete` come from the late stage (`anim.update`, `tween.update`), so a sim system never sees them; a late system with `after = { "tween.update" }` does.

`argus describe systems=true` shows every system in run order, the engine's included.

A few events are requests the engine listens for:

| Emit this | And the engine |
|---|---|
| `camera.shake {amount, camera}` | raises the camera's `CameraShake.trauma` (the drawing camera's when no camera is named) |
| `particles.burst {emitter, count, pos}` | bursts that emitter, at `pos` if given |
| `sound.stop {entity, fade_ms}` | stops the spatial sounds playing from that entity, fading over `fade_ms` |
| any name a `PostFlash`, `PostShake`, or `PostHitStop` lists in `on` | starts that effect (`hit` when `on` is empty) |

## Talking to the host

Whatever runs the game can trade text with it: the page of a web build, the daemon's clients, a scenario. A game whose turns are decided outside it uses this. Bring Your Own Game Master sends the scene to an AI model the page calls, and the model's answer comes back in.

Text comes in as a **message**: a channel, a short name the game chooses, and a text of up to 64 KiB. The next tick's scripts get it as a `host.message` event with `channel`, `text`, and `slot`:

```lua
for _, ev in ipairs(q:events("host.message")) do
  if ev.channel == "gm.ruling" then
    apply_ruling(q, ev.text)
  end
end
```

A message is input. It is recorded with the run's other input, so a seek, a replay, a mark, a player's report, or a scenario brings it in again at the same tick, and nobody has to send it twice. A run that depends on a model's answers still reproduces exactly. When the message's tick runs depends on who sent it. A web build's `player.message` on a discrete clock is a turn, run inside the call, as a key press is; a game that shouldn't spend one freezes it (see [Pausing](#pausing)). The daemon's `input.message` and a scenario's `t.input:message` only queue the message for the next tick, as an inject is queued: on a discrete clock that is the next turn (a key in the window, `argus run turns=1`, `t:advance`). Messages sent before that tick all arrive in it, oldest first, an event each.

A message that arrives on a frozen tick is not lost. The `unfrozen` systems see it on that tick; each system the freeze kept from running sees it the next tick it runs, before that tick's own messages, oldest first. Every system sees a message once. So a model's answer that comes in while the pause is up waits for the game's turn system until the pause ends. On a discrete clock that is the first turn after the pause, since the press that closes a pause menu is usually frozen too. Other events are moments of the sim: a frozen tick drops them for the systems it kept from running.

A script sends text out with `q:send(channel, text)`. It is an event, `host.send`, with `channel` and `text`, so it is in the event log, `argus events name=host.send`, and `t.expect:events` like any other.

| Who sends a message | How |
|---|---|
| the page of a web build | `player.message(channel, text)`, and `player.take_sent()` for what the game sent (see [Releasing](releasing.md)) |
| the daemon | `argus input message channel=gm.ruling text='...'` (the `input.message` method) |
| a scenario | `t.input:message(channel, text)` |

## Randomness

Randomness comes from **named streams**. A system that wants random numbers names its stream with the `rng` key and draws from `q:rng()`. Each stream is seeded from the run's seed and its own name only, so adding or reordering systems never changes another system's numbers, and the same seed always gives the same run. From `examples/delve/systems/potions.luau`:

```lua
return system "sim.potions" {
  stage = "sim",
  rng = "loot",
  -- after, reads, and writes left out here
  run = function(q)
    local rng = q:rng()
    for _, ev in ipairs(q:events("death")) do
      if rng:range(1, 100) <= ev.drop then
        -- spawn a potion where the monster fell
      end
    end
  end,
}
```

| Call | Gives |
|---|---|
| `rng:next()` | a number in [0, 1) |
| `rng:below(n)` | an integer from 0 to n - 1 |
| `rng:range(lo, hi)` | an integer from lo to hi, both included |
| `rng:chance(p)` | true with probability p |

A system's stream goes on from tick to tick, so what it draws depends on everything it drew before. A game that generates from a seed of its own (a run seed the player sees and shares, a floor, a room) wants each thing drawn from its own numbers instead: `q:rng_from(a, b, ...)` is a generator seeded from those whole numbers alone, with the same four calls. `q:rng_from(run.seed, floor, room)` draws the same room in whatever order the rooms are visited, on every machine. It needs no `rng` key. Make one where it is used rather than keeping it in a file-level local, which a seek does not put back.

The run's seed is the scene's `seed` in `scene.toml`: a number replays exactly, and `"random"` lets the runner pick one and record it. Scenarios run many seeds (see [testing](testing.md)).

Luau's own `math.random` is switched off (calling it fails, and the error says to use the system's stream), and scripts have no `os`, so no clock or date. A number that changed between a run and its replay would make a seek, a replay, or a test from the same seed come out differently, and marks and tests would stop reproducing.

## Timers and tweens

### Time

A system reads time from `q`: `q.dt` is the seconds this tick covers (1/60 on `continuous@60`, 1 on a discrete clock, and for a system that moves between turns 0 on a turn and 1/60 between turns), `q.time` the sim seconds since the run began, and `q.tick` the tick number. A game with days sets `[time] day` in `argus.toml` and reads `q.day`, `q.day_time` (0 to 1), and `q.day_length`. Gameplay never reads the wall clock.

### Timers

There is no timer call. A timer is a field your system counts down. This one fires at most twice a second while the `fire` action is held:

```lua
-- Lab.cooldown is an f32 field of the game's own component.
if lab.cooldown > 0 then
  lab.cooldown = math.max(0, lab.cooldown - q.dt)
elseif a.fire then
  lab.fired += 1
  lab.cooldown = 0.5
end
```

Counting ticks works too, as Swarm's `Director.timer` and Toybox's `Dropper.timer` do. Keep the timer in a component, not in a Luau local, so a save, a seek, and a replay all have it.

### Tweens

A tween animates one field of an entity from one value to another over time. Start one from a script with `q:tween`, which spawns a `Tween` entity (so the system declares `writes = {"Tween"}`):

```lua
q:tween(q:entity("@ghost"), "Dissolve.amount", { to = 1, duration = 0.5, easing = "linear", on_complete = "ghost.gone" })
```

The options are `from` (else where the field is now), `to`, `duration`, `delay`, `easing`, `repeats` (-1 forever), `yoyo`, `on_complete` (an event emitted when it ends), and `keep`. The path is `Component.field` for a number or a vector, or `Component.vec.x` for one axis. Easings are `linear`, `step`, and `in_`, `out_`, or `in_out_` with `quad`, `cubic`, `quart`, `sine`, `expo`, `back`, `elastic`, or `bounce` (`out_back`). A curve that overshoots is held inside the field's range.

Tweens run in the late stage, so read the `on_complete` event from a late system ordered `after = { "tween.update" }`. A sim system never sees it.

Squash and stretch, UI slides, camera nudges, and fades are all tweens. For several tweens, camera cuts, and lines in a sequence, use a timeline (see [content](content.md#timelines-and-cutscenes)).

## Pausing

`q:freeze()` makes the rest of this tick stand still, except for systems declared `unfrozen = true`. The engine's UI systems are unfrozen, so menus still work. A pause menu is an unfrozen input system that freezes every tick while the menu is open. From `examples/hollow/systems/pause.luau`:

```lua
-- stage = "input", after = { "input.actions" }, unfrozen = true
run = function(q)
  local menu = q:entity("@pause"):get("UiNode")
  if q:entity("@player"):get("Actions").pause_pressed then
    menu.visible = not menu.visible
  end
  if menu.visible then
    q:entity("@hud"):get("Run").started += q.dt
    q:freeze()
  end
end,
```

A host's message that arrives while the game is frozen waits for the systems the freeze stopped (see [Talking to the host](#talking-to-the-host)). `q.time` keeps running through a freeze (a pause stops the game's systems, not the clock). A game that shows its own time keeps it in a component its systems advance, as Hollow's `Run.started` and Outpost's `Colony.clock` do. Systems that must answer during a pause, such as Hollow's lighting under the settings page, are `unfrozen` too.

A menu's buttons are pressed on a frozen tick: the menu's system froze it in the input stage, before the UI knew a button would be pressed. So a button that starts or changes the game ("New run", "Begin") does its work in an `unfrozen` system that reads the button's event. A system that is not `unfrozen` does not run on that tick. Delve's `sim.menu_buttons` resets the hero and the floor count itself on `menu.new_run`, and `sim.floor`, also `unfrozen`, draws the new floor on the same turn.

Other ways time changes:

- `q:speed(3)` asks for three ticks a frame from the next frame; `q:speed()` reads the speed. Outpost's 1x, 3x, and 10x keys use it (`examples/outpost/systems/controls.luau`). The speed is never 0: pause with `q:freeze()`.
- A `PostHitStop` freezes the game for a few ticks on an event, for a heavy hit (see [presentation](presentation.md#post-effects)).
- From outside, `argus run pause=true` and `argus run speed=10` control the daemon's clock.

## Saves and loading

A save is the world at one tick: every entity, the clocks, the random streams, particles, the rooms loaded, and a runtime tilemap's cells. A script asks for one with `q:save(name)`; it is written at the end of the tick that asked. `q:load(name)` loads one after the step that asked. Both names default to `"auto"`. Outpost saves itself each time a day turns (`examples/outpost/systems/day.luau`):

```lua
q:emit("day.turned", { day = day, food = food, growth = rules.growth })
q:save("auto")
```

Where saves go: the daemon keeps them in `.argus/saves/<name>.save`, a shipped game in the player's save folder, and the web build in the browser's storage. A save emits `game.saved {save, path, bytes, tick}` and a load `game.loaded {save, tick, migrated}`, `save` being the save's name. From outside, `argus run save=week1` and `argus run load=week1` do the same.

What a save does not hold: anything a script keeps in Luau variables (keep game state in components), and the input state (a loaded game hears the player's hands as they are now).

### Save slots

A menu can show the saves there are. `q:saves()` lists them, newest first. Each has its `name`, `saved_at` (seconds since 1970), `scene`, `tick`, `time`, and `summary`: fields the game gave when it saved, for a menu to show.

```lua
q:save("slot2", { level = 4, place = "The Hall", minutes = 37 })

local latest = q:saves()[1]          -- for a Continue button
if latest then
  label.text = latest.summary.place .. ", level " .. latest.summary.level
end
```

A summary holds words, numbers, and true or false. `q:delete_save(name)` deletes a save after the step that asked, from the disk and the platform's cloud, and emits `game.save_deleted {save}`. A three-slot menu is three names, `q:save` and `q:load` on a button, and `q:saves()` to show which slots hold a game.

What a tick reads with `q:saves()` is part of the run, as a key press is. The first time a tick runs, the list comes from wherever the saves are kept and is recorded with the run's input. A replay, a seek back, or a bug report reads the recorded list, so the game does what it did whatever saves are on the disk now. This needs `input.toml`. Without it the list is read as it is, and is not recorded. While `argus test` runs scenarios, the list starts empty, so a test never depends on your own saves.

### Saves that survive code changes

Each component has a schema `version`, 1 by default. When you add, rename, or remove fields of a component that saves hold, raise its version and say how an old row becomes a new one with `migrate`: a table of steps by version (`rename`, `default`, `drop`), a function per version, or one `function(row, from)`. This one is built from the scripting reference's example:

```lua
component "Health" {
  version = 3,
  cur = field.i32 { default = 10 },
  max = field.i32 { default = 10 },
  migrate = {
    [2] = { rename = { hp = "cur" }, default = { max = 10 } },
    [3] = function(row) row.cur = math.min(row.cur, row.max) return row end,
  },
}
```

Without a `migrate`, old rows come over field by field, by name. Each migration is reported as a `component.migrated` event. A save from a newer version of the game is refused. Outpost keeps a real old save in `examples/outpost/tests/saves/week1.save`, and `tests/week1.luau` loads it on every test run, so a change that breaks old saves fails a test.

## Where to go next

- [Presentation](presentation.md): sprites, animation, effects, lighting, the camera, UI, and sound.
- [Content](content.md): data tables, dialogue, timelines, strings, and accessibility.
- [Scripting](scripting.md): systems, `q`, components, and the sandbox.
- [Scenes](scenes.md): entity files, prefabs, rooms, and tilemaps.
- [Testing](testing.md): scenarios that press actions and check what happened.
