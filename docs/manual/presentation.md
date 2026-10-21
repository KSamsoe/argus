# Presentation

This chapter covers how a game looks and sounds: sprites, animation, particles, materials, post effects, 2D lighting, the camera, the UI, and audio. Like everything in Argus, each of these is components on entities and text files under `assets/`, so an agent can read and change them and a person can tune them while the game runs.

Assets live under `assets/`. An asset's **name** is its path there without the extension: `assets/sprites/hero.png` is `sprites/hero`, and that is how a scene names it. A `.toml` beside a file with the same name is its **sidecar**: how to cut, play, or mix it. `argus asset list` shows every asset and its warnings, and `argus asset get <name>` shows one as imported.

## Sprites

The `Sprite` component draws an entity. Its main fields:

| Field | What it is |
|---|---|
| `atlas` | The sprite asset to draw (empty for a plain shape) |
| `frame` | Which frame of the atlas, 0 first |
| `kind` | With no atlas: `rect` or `circle` |
| `color` | `#RRGGBB` or `#RRGGBBAA`: a shape's colour, or a tint over an image |
| `size` | Size in world units (whatever the image's pixels) |
| `z` | Draw order: higher is in front |
| `flip_x`, `visible` | Mirror it, or hide it |
| `material` | Draw it with a material (below) |
| `repeat` | How many times its frame repeats across and down: `[14, 7]` is a floor of 14 by 7 tiles in one sprite (default `[1, 1]`, once) |

A see-through `color` (`#0B163096`) blends in linear light, as lighting does, so a dark sheet over the scene takes away less than its alpha suggests: black at half alpha leaves white at about three quarters of its brightness as you see it. For a night or a dimming that reads as its number, use a `PostFade` toward a dark colour (below), whose amount is a share of what you see.

### Shapes before art

A game runs before its art exists. A sprite with no atlas is a described shape. From `templates/platformer/prefabs/coin.toml`:

```toml
[Sprite]
kind = "circle"
size = [0.6, 0.6]
color = "#FFD84A"
z = 2.0
```

A sidecar on its own (no image) is a **placeholder** asset: `kind`, `size` in pixels, and `color`. Scenes name it like real art, and when the PNG of the same name arrives it replaces the shape with nothing else changed. From `examples/hollow/assets/hermit.toml`:

```toml
# A placeholder: replace it with hermit.png (same name) and nothing else changes.
kind = "rect"
size = [14, 24]
color = "#8C7A5B"
```

### Images, frames, and clips

A `.png` with no sidecar is one frame with its pivot in the middle. A sidecar cuts it into frames and names clips. From `examples/hollow/assets/hero.toml`, eight frames in a row:

```toml
grid = [8, 1]

[clips.idle]
frames = [0]

[clips.run]
from = 1
to = 4
fps = 12

[clips.land]
frames = [7, 0]
fps = 12
loop = false
```

Other keys: `[[frames]] rect = [x, y, w, h]` for frames of different sizes, `pivot = [x, y]` (fractions of the frame, y up from the bottom: `[0.5, 0]` is the feet), `duration_ms` per frame, and a clip's `direction` (`forward`, `reverse`, `pingpong`). A tileset's frames carry `properties` such as `solid` and `one_way` (see [scenes](scenes.md)).

An image whose sides are not powers of two (16, 32, 64, ...) is padded to the next ones for the GPU, and `asset list` says so ("texture 90x18 is not power-of-two, padding to 128x32"). It is a note, not a fault: the picture is the same, and the padding costs only texture memory. Drawing to power-of-two sizes, or packing several images into one sheet, avoids it.

The entity file names the asset and sets its world size. Hollow's hero:

```toml
[Sprite]
atlas = "hero"
size = [1.0, 1.625]
z = 5.0
```

### Art scripts

A sprite can also be drawn by a script: `assets/sprites/slime.art.luau` is the sprite `sprites/slime`, in place of a PNG and its sidecar. It is Luau, drawing on canvases with a pixel-art kit, `art`, and it ends by returning the sprite's frames and clips. The importer runs it, packs the frames into a sheet, and from then on it is a sprite like any other: scenes, animators, and releases use it the same way, and a release carries the picture, not the script. Change the script and the game shows the change while it runs, as it does for a game script. Nothing else is needed: no image editor, no Python.

```lua
-- assets/sprites/slime.art.luau: a slime of two frames, bouncing.
local pal = { body = "#4CAF50", shine = "#A5D6A7", eye = "#1B1B1B", line = "#1E3A20" }

local function slime(squash)
  local c = art.canvas(12, 12)
  c:ellipse(5.5, 7 + squash, 5 - squash, 4 - squash, pal.body)
  c:pixel(4, 6, pal.eye)
  c:pixel(7, 6, pal.eye)
  c:shade{ light = pal.shine }   -- light on the edges facing up and left
  c:outline(pal.line)            -- a line round all of it
  return c
end

return art.sprite{
  pixels_per_unit = 12,
  pivot = {0.5, 0},               -- it stands on its bottom edge
  clips = { bounce = { slime(0), slime(1), fps = 4 } },
}
```

Coordinates are pixels, x right and y down from the top left, as in an image file; colours are `"#RRGGBB"`, `"#RRGGBBAA"`, or `"none"`. The kit draws pixels, lines, rectangles, circles, rings, ellipses, and polygons; fills, outlines, shades from a light, dithers, and textures with noise and speckles; mirrors, flips, rotates, scales, crops, and draws one canvas over another; and mixes, lightens, darkens, and makes colours. `art.rng(seed)` gives seeded random numbers and `art.grid` cuts a sheet into frames. The whole kit, and every key `art.sprite` takes, is in the reference: `argus docs files section=art.luau`.

Scripts share code and colours through modules. `local pal = require("colours")` runs `assets/colours.lib.luau`, a Luau file that returns something (a table of colours, a helper function), and a change to it imports again every script that requires it, so one palette can colour several sprites. A module is not an asset, and a release leaves it out.

A script is sandboxed: it cannot read files, the clock, or the network, and `math.random` is refused in favour of `art.rng(seed)`. Its maths is the same on every machine, so the same script draws the same pixels everywhere. A mistake is an import error with the script's line, and `print` comes back as a warning on the asset, in `argus asset list`. To look at what it drew, put it in a scene and use `observe`; the packed sheet is in `.argus/assets/<name>.png`. Frames are packed edge to edge, so a frame drawn much larger than its pixels (a zoomed camera) can pick up a line of its neighbour's colour at an edge, more so in the software rasteriser that picture goldens use: leave a one-pixel clear border round a frame that is shown that way.

The templates' art is drawn this way: their `assets/*.art.luau` are worked examples of characters, tiles, and panels.

You need not start from an empty canvas. The `generate` tool (or `argus generate`) writes an art script for you to make your own, free and offline:

```sh
argus generate name=sprites/slime sprite=creature seed=4   # assets/sprites/slime.art.luau
```

The kinds are `creature` and `character` (rows of text, with idle and walk frames), `item` (a potion, a gem, a coin, a key, a heart, an apple, a sword, or a shield), `tile` (one that repeats with no seam, in four variants), `plant` (a bush or a tree that sways), and `rock`. Another seed is another take. The script's head says the command that made it, and it is an edit like any other, undone with `edit undo=true`. The answer has a picture of the frames, and `argus asset preview name=<sprite>` makes one of any sprite: its frames in rows of 12, each numbered under it, scaled up on a checkerboard so you can see what is clear. `frames=37..51` shows only those, and `columns=` sets how many to a row.

## Animation

### The animator

An `Animator` runs a state machine on its entity's sprite: each state plays a clip, and leaves by the first transition whose condition holds. The machine is an asset with `type = "animator"`. From `examples/hollow/assets/anim/hero.toml`:

```toml
type = "animator"
initial = "idle"

[states.idle]
clip = "idle"

[[states.idle.transitions]]
to = "run"
when = "KinematicBody.grounded and abs(KinematicBody.velocity.x) > 0.5"

[states.run]
clip = "run"
events = [{ frame = 0, name = "footstep", sound = "sfx/step" }, { frame = 2, name = "footstep", sound = "sfx/step" }]

[[states.run.transitions]]
to = "idle"
when = "KinematicBody.grounded and abs(KinematicBody.velocity.x) <= 0.5"
blend = 0.08

[[any]]
to = "jump"
when = "not KinematicBody.grounded and KinematicBody.velocity.y > 0"
```

The entity says which machine: `[Animator] machine = "anim/hero"`.

- A **condition** is terms joined by `and` and `or` (`and` binds tighter): a field of the entity (`KinematicBody.grounded`), `abs(...)`, or `end` (a one-shot clip finished), `time` (seconds in the state), `frame`, `loops`, compared with `<`, `<=`, `>`, `>=`, `==`, `!=`. `"Body.grounded and Body.vx > 0.5 or Body.dash > 0"` holds when walking on the ground or dashing.
- `face = "KinematicBody.velocity.x"`, at the top of the machine, mirrors the sprite while that field is below zero and unmirrors it above zero; at zero it keeps the way it faced, so a hero who stops faces the way they walked. Draw the art facing right.
- `[[any]]` transitions are checked first, from every state.
- `blend` cross-fades into the next clip; `window = [first, last]` limits a transition to some frames.
- **Events by frame** fire an event with the entity, frame, and state, and can play a sound. The run's feet above play `sfx/step`. These events come from `anim.update` in the late stage, so only a late system ordered after it sees them with `q:events`.
- A script enters a state by setting `Animator.play = "attack"`. `Animator.state` says where it is.
- For feel, a state's `fps`, `anticipation`, and `hold` can name a field of the entity instead of a number (`fps = "Hero.run_fps"`, a tune field of your own), and `windup` and `hold_frame` shape a wind-up and a held pose.

### Aseprite files

An `.aseprite` or `.ase` file under `assets/` is a sprite with no export step: its frames and their durations, its tags as clips, and its slices as the pivot and frame properties. A sidecar beside it can pick `layers` and change a tag's clip (`[clips.<tag>] fps = 12`) without opening Aseprite; `argus asset get <name>` lists the file's layers, tags, and slices. Limits: `grid` and frame rects are ignored (the frames are the file's), and scripts cannot read a frame's properties yet (a slice used as a hitbox is in `asset.get`, not in Luau).

### Spine skeletons

A `.json` exported from Spine 4 is a skeleton, its images in the export's `images` folder. A `Skeleton` component poses it, each slot drawn by a part entity under it. With an `Animator`, the machine's clips are the skeleton's animations; without one, `animation` plays at `speed`. From `examples/puppet/scenes/stage/`:

```toml
# hero.toml: an animator plays it (assets/puppet/moves.toml)
[Skeleton]
skeleton = "puppet/puppet"
scale = 0.01
z = 2.0

[Animator]
machine = "puppet/moves"

# knight.toml: another skin, mirrored, walking on its own
[Skeleton]
skeleton = "puppet/puppet"
skin = "armored"
animation = "walk"
speed = 0.8
scale = 0.01
flip_x = true
```

`scale` is world units per Spine unit. Spine's own events fire as engine events with the same names (Puppet counts its `step` and `waved` events). Bones, slots, region attachments, skins, events, and IK work. Not supported, each a warning in `asset.list`: meshes, paths, clipping, transform and path constraints, physics, and deforms. A part's shear is dropped, and an event's audio path is not played.

## Particles

A particle effect is an asset with `type = "emitter"`; an `Emitter` component plays it at its entity. Particles live in a pool, not as entities, so tens of thousands are cheap. From `examples/swarm/assets/fx/pop.toml`:

```toml
type = "emitter"
particle = "circle"
max = 20000
lifetime = [0.2, 0.45]
speed = [3, 7]
spread = 360
drag = 3
size = [[0, 0.35], [1, 0]]
color = [[0, "#FFF2A0FF"], [0.5, "#FF9040C0"], [1, "#FF402000"]]
z = 4
material = "fx/glow"
```

A range is a number or `[min, max]`; a curve is a number or `[[t, value], ...]` over a particle's life. `rate` emits continuously, `burst` all at once when it starts. `shape` is `point`, `circle`, or `box`.

Swarm keeps one idle emitter (`examples/swarm/scenes/arena/sparks.toml`: `[Emitter] effect = "fx/pop"`, `playing = false`) and bursts it wherever an enemy dies:

```lua
q:emit("particles.burst", { emitter = q:entity("@sparks"), count = 12, pos = target:get("Transform").pos })
```

You can also set `Emitter.burst = 12` for a burst on the next tick, or `Emitter.started = false` to play it again from the start. `auto_despawn = true` removes a one-shot emitter's entity when its last particle is gone. The effect's numbers are tune fields (`argus tune fields asset=fx/pop`).

## Materials and shaders

A sprite, an emitter, or a skeleton can draw with a **material**: `[Sprite] material = "fx/glow"`. The engine has a small library: `unlit`, `lit`, `palette`, `dissolve`, `outline`, and `sdf_text`. A sidecar alone over a built-in changes its blend or its defaults. From `examples/swarm/assets/fx/glow.toml`:

```toml
shader = "unlit"
blend = "add"
```

Three built-ins read a component on the sprite's entity: `dissolve` reads `Dissolve.amount` (0 whole, 1 gone), `outline` reads `Outline.color`, and `palette` reads `Palette.row`. Tweening `Dissolve.amount` to 1 dissolves an enemy away.

A material of your own is a `.wgsl` file with a sidecar of typed parameters. The shader defines one function; the engine supplies the rest:

```wgsl
// assets/materials/glow.wgsl
fn material(in: MaterialInput, p: Params) -> vec4<f32> {
    let c = sample_atlas(in, in.uv);
    return vec4<f32>(p.tint.rgb * p.power, c.a);
}
```

```toml
# assets/materials/glow.toml
type = "material"
blend = "add"

[params.tint]
type = "color"
default = "#FF8000"

[params.power]
type = "f32"
default = 1
range = [0, 4]
```

`MaterialInput` has `uv` (where in the atlas), `local` (where on the sprite, 0 to 1, y up), `world`, `color` (the sprite's tint), `frame`, `texel`, and `time` (sim seconds). `sample_atlas(in, uv)` reads the sprite's image. A parameter with `from = "Component.field"` is fed from each sprite's own entity (or `from = "@id:Component.field"` from a named one), so a tween or a script animates it. A shader that does not compile is a `script.errors` row with its line, and the last good version keeps drawing.

## Post effects

Post effects are passes over the whole picture. Each is a component on the **drawing camera**, and its strengths and times are tune fields:

| Component | What it does |
|---|---|
| `PostShake` | The picture shakes on an event |
| `PostFlash` | The screen flashes a colour on an event |
| `PostHitStop` | The game freezes for a few ticks on an event, while effects play on |
| `PostFade` | The screen fades toward a colour by `amount` |
| `PostVignette`, `PostAberration`, `PostPalette`, `PostCrt` | Dark edges, colour fringes, a limited palette, an old screen |

The first three listen for an event named in `on` (`hit` when empty), and an `amount` field on the event scales flash and shake. From `examples/hollow/scenes/hollow/rooms/persistent/camera.toml`, the pit's impact:

```toml
[PostFlash]
on = "player.fell"
color = "#FF3030"
peak = 0.68
decay = 2.0

[PostShake]
on = "player.fell"
trauma = 0.7

[PostHitStop]
on = "player.fell"
frames = 6

[PostFade]
```

Flash, fade, and vignette strengths are shares of what you see: a flash at 0.1 takes the screen a tenth of the way to its colour, on a dark cave as on a bright sky, and a vignette at 0.25 takes a quarter of the brightness from the edges. (Before 0.0.5 they were shares of linear light, so a small flash showed strongly on a dark screen; the examples' values were changed to look as they did.)

At each room change Hollow tweens `PostFade.amount` a little toward black and back (a timeline; see [content](content.md#timelines-and-cutscenes)). On a discrete clock, effects step with turns, so a flash fades only as turns pass.

## Lighting

2D lighting is off until the drawing camera has a `Lighting` component. Then everything is lit by the ambient light times `level`, plus every `Light` in reach. Hollow's camera:

```toml
[Lighting]
ambient = "#8A94B8"
level = 0.6
```

A `Light` goes on any entity: a point light, or a cone when `angle` (half its width in degrees) is under 180, with `direction`. It fades to nothing at `radius`. From `examples/hollow/scenes/hollow/rooms/persistent/lantern.toml`, the hero's lantern:

```toml
[Sprite]
kind = "rect"
size = [0.22, 0.3]
color = "#FFD27A"
z = 5.5

[Emissive]
amount = 1.0

[Light]
color = "#FFB870"
intensity = 1.3
radius = 7.5
falloff = 1.6
flicker = 0.12
```

- **Shadows.** Solid and sloped tiles of every collision layer cast shadows by themselves (`Lighting.tiles`). An `Occluder` makes an entity cast one, a box the size of its sprite unless `size` says otherwise. A light's `shadows = false` turns its own off.
- **Glows.** `Emissive` draws a sprite at its own colour in the dark: a gem, a lit window, the lantern itself. A material can say `emissive = true` for everything drawn with it.
- **Normal maps.** A `<name>.normal.png` beside `<name>.png`, the same size, is the sprite's normal map; nothing to declare. A light's `height` decides how it rakes across the bumps.
- **Changing the light.** `Lighting.level` and `ambient` are fields like any other. Hollow glides the level from room to room (`examples/hollow/systems/light.luau`) and Outpost follows its day from dawn to midnight (`examples/outpost/systems/light.luau`). The UI is drawn after the light, so a HUD stays readable in the dark.

Limits: shadows are hard, a slope occludes as a whole tile, a sprite's occluder is a box, and a light inside a solid tile is shadowed on every side (keep a carried light inside its carrier's collider, as Hollow's lantern is).

## The camera

A `Camera` frames the scene from its entity's position; `height` is how many world units show top to bottom. The active camera with the highest `priority` draws. The height always fills the window, so a window wider than the camera's `aspect` (16:9 by default), such as a phone on its side, shows more of the world at the sides, and a narrower one shows less. Only the picture changes: the camera, its room bounds, and its `view_min` and `view_max` stay the game's own shape, so a run plays the same on any screen.

**Following.** With a `target`, it follows: the target moves inside the `dead_zone` without moving the camera, `lookahead` looks ahead the way it moves, and `smoothing` is how long the camera takes to catch up. From Hollow's camera:

```toml
[Camera]
height = 12.0
target = "@player"
offset = [0.0, 1.5]
touch_offset = [0.0, -4.0]
dead_zone = [1.5, 1.0]
blend = 0.8
```

**Bounds.** With `bounded = true` (the default) the view stays inside the current room's bounds, or the scene's `bounds = [x0, y0, x1, y1]` in `scene.toml`. `aspect` (16:9 by default) is the view's shape for these bounds.

**Played by touch.** On a phone, the player's thumbs rest on the [touch controls](gameplay.md#touch), usually low on the screen, and can hide a hero drawn there. `touch_offset` is added to `offset` while the player plays by touch, and the view may then pass the room's edge by the same amount in that direction. Hollow's `[0.0, -4.0]` aims 4 units lower. A hero standing on a room's floor is then drawn near the middle of the screen, with up to 4 units of the dark below the floor under the thumbs, instead of at the bottom edge. The camera glides to the new framing when the player picks up the phone, and back when they press a key. It is zero by default, which frames the same on every device.

**Free pan and zoom.** With no target, `pan_action` and `zoom_action` name actions a player moves it with; `zoom_min` and `zoom_max` limit the zoom. Outpost's camera, which `examples/outpost/systems/found.luau` builds in Luau, as an entity file would write it:

```toml
[Camera]
height = 26.0
pan_action = "pan"
pan_speed = 0.8
zoom_action = "zoom"
zoom_min = 8.0
zoom_max = 40.0
blend = 0.8
```

With neither a target nor actions, a camera stays where its entity is.

**The background.** Where nothing is drawn, the frame shows the engine's dark grey. A `Background` on the drawing camera's entity gives it a colour instead (`#RRGGBB`), so the space past a map's edge or around a floor matches the game:

```toml
[Background]
color = "#14111c"
```

**Cuts and blends.** A second camera with `active = false` and a higher priority takes over when it becomes active, and its `blend` glides the view over that many seconds. Hollow's outro cuts to `@wide` from a timeline (see [content](content.md#timelines-and-cutscenes)).

**Shake.** A `CameraShake` beside the camera moves the view in the world. Raise its trauma with an event from any system:

```lua
q:emit("camera.shake", { amount = 0.8 })
```

`decay`, `curve`, `max_offset`, `max_angle`, and `frequency` shape it. `PostShake` (above) instead moves the finished picture of the world. Neither moves the UI, which is drawn after the post passes.

**Parallax.** A background can move slower than the world as the camera moves, so it looks far away. Give a sprite a `Parallax`:

```toml
[Parallax]
factor = [0.5, 0.5]
origin = [0.0, 0.0]
```

`factor` is how fast it crosses the screen against the world: 0.5 is a far hill, 0 the sky (it never moves), and above 1 a foreground that passes faster. `origin` is where the camera's centre is when the sprite is drawn exactly where its Transform says; set it to where the camera starts. A tile layer takes `parallax = [0.5, 0.5]` (or `parallax = 0.5` for both) in its `*.tilemap.toml`, and the map takes `parallax_origin`, in world units from its top-left corner. A Tiled map's layer parallax factors and its parallax origin are read as they are.

Parallax is drawing only. A parallax sprite still collides and overlaps where its Transform is, and a parallax layer's tiles collide where their cells are, so keep collision on layers without it. The semantic frame shows a parallax sprite where it is drawn.

## UI

The UI is entities too, laid out on a canvas of UI pixels (960 x 540 by default, y down) that is scaled to fit the window. In a window of another shape the canvas sits in the middle at its own shape, with the world drawn around it, and the touch controls go to the window's own edges ([Gameplay](gameplay.md#touch)). `ui.toml` at the project's root sets the canvas, the font, the text size, and the colour tokens. From `examples/delve/ui.toml`:

```toml
font = "fonts/LiberationSans-Regular"
text_size = 18

[colors]
text = "#EADCB8"
panel = "#16131BF0"
border = "#A07C48"
focus = "#FFB04A"
button = "#3A3442"
button_focus = "#6A5A52"

[high_contrast_colors]
text = "#FFFFFF"
panel = "#000000"
button = "#000000"
```

The engine's tokens are `text`, `muted`, `panel`, `border`, `focus`, `accent`, and `disabled`; a game adds its own (`button` and `button_focus` above, which Delve's `systems/look.luau` swaps on the focused button). A `color` field may name a token or give a `#RRGGBB(AA)`.

### The parts

| Component | What it does |
|---|---|
| `UiRoot` | The top of a tree: its `layer`, which player slot drives it, whether it is `interactive`, and `follow` to hang it from a world entity |
| `UiNode` | Layout, flexbox style: `direction`, `justify`, `align`, `gap`, `pad`, `size`, `fill`, `absolute` with `anchor` and `pivot`, `order`, `visible` |
| `UiText` | Words: `text`, or a string table's `key`; `size`, `color`, `font`, `align`, `wrap`. A newline (`\n` in a script's string, a line break in a TOML multi-line string) always starts a new line; `wrap` also breaks at spaces to fit the node's width |
| `UiPanel` | What a node draws behind it: a `color`, an `image` (nine-sliced with `slice`; `frame` picks one frame of a sheet), a `border` |
| `UiButton` | Takes focus and is pressed; emits `ui.pressed` and its own `event` |
| `UiTextBox` | A box the player types into: pressed, it types; Enter sends its `event` with the text (see [A text box](#a-text-box)) |
| `UiScroll` | A node whose children scroll inside it, cut off inside its padding; a log keeps its newest line in view, and `to` scrolls to a line (see [A log that scrolls](#a-log-that-scrolls)) |
| `UiRect` | Where the node landed (written by the layout) |

Nodes are children of other nodes through `Parent` (or `[children.<name>]` tables), as anything is.

A node is as big as what it holds, or its `size` (a `fill` share takes part of its parent instead). `min` and `max` bound that, and `grow` and `shrink` say how readily it takes up room its parent has to spare or gives up room its parent is short of. A node with no `min` never shrinks smaller than what it holds. A node with a `min` can: its `min` takes the place of its content as the smallest it may be, as in CSS. So in a column short of room, a panel with `min = [0.0, 380.0]` shrinks down to 380 even when its words need more, and they run past its bottom. Give such a node `shrink = 0.0` to keep it at least as big as what it holds.

A node that does not fit is laid out past its parent or the canvas anyway: a row of words too wide for the screen comes out centred, both its ends off it, its first button among them. The engine says so: `argus summary` lists such nodes under `ui_overflow`, and the semantic frame (`argus observe`) lists them under `problems`, each with how far past which edge it is, buttons and text boxes first. A node inside one that clips (a `UiScroll`, or `clip = true`) is cut on purpose and not listed, nor is a tree hung from a world entity, which may stand at the screen's edge.

A script can make UI while the game runs: spawn an entity with a `UiNode` (and a `UiText`, `UiPanel`, or `UiButton`) and give it a `Parent` in a tree. It needs no `UiRect`; the layout adds one. A spawned button takes focus, hover, and presses as an authored one does, and a script may move the focus to it by setting its tree's `UiRoot.focus`.

### A HUD

A HUD is a tree that takes no input. From `examples/delve/scenes/delve/turn.toml`:

```toml
[UiRoot]
interactive = false

[UiNode]
absolute = true
offset = [10.0, 5.0]
pad = [12.0, 4.0]

[UiPanel]
image = "ui/panel"
slice = 4.0
scale = 2.0

[UiText]
key = "hud.line"
color = "accent"
```

The panel is an image, `assets/ui/panel.png`, nine-sliced: its corners stay as drawn (`slice` pixels in from each edge, shown at `scale` times their size) and its middle stretches to fit the words.

The string `hud.line` holds holes the layout fills each frame from the world: it begins `"Floor {@player:Player.floor}, turn {Turn.n}."` (see [content](content.md#strings-and-languages)). A script can also write `UiText.text` itself, as Hollow's `sim.run` does.

A tree with `follow` hangs from a world entity: a sign's words, a prompt over a character. Hollow's hermit has `[UiRoot] follow = "@hermit"`, `follow_offset = [0.0, 1.1]`, `interactive = false`, and a `UiNode` with `pivot = [0.5, 1.0]` and `visible = false`, which `systems/hermit.luau` shows while the hero is near.

### Menus and buttons

A menu is a tree of buttons. Hollow's pause menu is a few entity files in `scenes/hollow/rooms/persistent/`: the root, a panel under it, and buttons under the panel:

```toml
# pause.toml: the root, hidden until Escape
[UiRoot]
layer = 10

[UiNode]
justify = "center"
align = "center"
visible = false

# pause.resume.toml: one button
[Parent]
entity = "@pause.panel"

[UiNode]
pad = [18.0, 8.0]
order = 1

[UiPanel]
color = "button"

[UiButton]
event = "pause.resume"

[UiText]
key = "pause.resume"
align = "center"
```

Focus moves between buttons with `ui_move`, to the nearest in that direction (or where a button's `up`, `down`, `left`, `right` say). `ui_confirm` presses the focused button, and `ui_click` the one under the pointer. A menu that appears with nothing focused gets its first button. A press emits `ui.pressed {entity, event, by}` and the button's own event (`pause.resume` above), which a system reacts to with `q:events`. `ui_cancel` emits `ui.cancel {root}`. The UI systems run `unfrozen`, so a menu works while the game is frozen. Pages are subtrees whose `visible` a system switches (`examples/hollow/systems/pause_menu.luau`).

### Rich text

A `UiText`'s words take markup, so one line can carry a colour, a weight, a size, the player's own key, and a picture:

```toml
[UiText]
text = "Press [glyph=drink] to [color=accent][b]drink a potion[/b][/color], [i]if you dare[/i]. [img=sprites/potion]"
```

| Markup | What it draws |
|---|---|
| `[color=accent]words[/color]` | In a theme colour (`ui.toml`'s names) or `#RRGGBB` (`AA`). |
| `[b]words[/b]` | Bold: a font asset's strokes thicker; the pixel font's glyphs drawn twice, a pixel apart. |
| `[i]words[/i]` | Italic: leaning right. |
| `[size=24]words[/size]` | At that size, in canvas pixels. A line is as tall as its largest words, and the rest stand on its foot. |
| `[glyph=jump]` | What the player presses for the action, on the device they use now, outlined like a key in the theme's `border` colour: Space on the keyboard, A on an Xbox pad, the touch button's words under a finger. It changes when they pick up another device. `[glyph=jump:2]` is slot 2's. |
| `[img=icons:3]` | A sprite's frame (frame 0 without the number), as tall as the words and as wide. |
| `[[` | A bracket. |

Anything else in brackets is words as written, so `[E] Talk` still reads `[E] Talk`. Markup that names a tag but does not read (a size that is not a number, a `[/b]` with no `[b]`, a `[glyph]` for an action with nothing bound) is shown as written and said under the semantic frame's `problems`, with the node, as a hole that names nothing is. Holes and string tables work as before, and markup may stand in a table's text and around a hole: `"[b]{Gold.count}[/b] gold"`.

The semantic frame, the screen reader, and the web page's reader get the plain words: no tags, a key's words (`Press Q to drink a potion`), nothing for a picture. `UiText.shown` keeps the markup, with each `[glyph]` already made the key it shows.

Delve uses it in its string table (`examples/delve/strings/en.toml`). The log shows harm and healing in two colours that `ui.toml` adds (`harm` and `heal`, each with a brighter one for high contrast), and the menu's hint shows the player's own keys:

```toml
[menu]
hint = "[glyph=ui_move] to choose, [glyph=ui_confirm] to press, [glyph=pause] to go back"

[message]
kill = "You [b]kill[/b] the {monster}."
hurt = "The {monster} hits you for [color=harm]{amount}[/color]."
potion_drunk = "You drink a potion and heal [color=heal]{amount}[/color], to {hp}/{max}."
```

On the keyboard the hint reads "Arrow keys to choose, Enter to press, Esc to go back", with each key outlined. Keys that sit side by side, like "Arrow keys", share one outline. Delve's `tests/looks.luau` checks those words and compares the log and the menu with saved pictures.

### A text box

A `UiTextBox` is a button the player types into. It goes on a node with a `UiButton` (to take the focus and be pressed) and a `UiText` (which shows it):

```toml
[UiNode]
size = [360.0, 28.0]
pad = [8.0, 4.0]

[UiPanel]
color = "button"

[UiButton]

[UiText]

[UiTextBox]
hint = "What do you do?"
event = "player.said"
max = 200
```

Pressed (confirm, or a click), it types: the keyboard's characters go into `UiTextBox.text`, and the keyboard moves no action meanwhile, so `w` is a letter and not a step. Backspace takes the last character off. Enter sends it: `ui.submitted {entity, text}` and the box's own `event` with the same fields, which a system reads with `q:events("player.said")`; the box is emptied (`clear = false` keeps the text) and stops typing (`keep = true` keeps it typing, for an input line that is always open). Escape, the cancel action, or a click elsewhere stops it without sending. While it types, its `UiText` shows the text and a caret; empty and not typing, the `hint`. A script starts it with `UiTextBox.typing = true`, or empties it by writing `text`. Started by a script, it types from two ticks later: what the keyboard typed before then is dropped.

What is typed is input like a key: the input journal records it, so a replay, a mark, and a player's report type it again, and a seek back and forward does. In a turn-based game a character typed takes no turn (the systems that run `unfrozen`, the UI's among them, see it, as they see a menu's moves); Enter, which sends the line, does.

A game can take typed text without a box: while its slot's `Actions.typing` is true, the keyboard types instead of moving actions, and what it typed this tick is `Actions.typed` (Backspace, Enter, and Escape as `"\u{8}"`, `"\r"`, and `"\u{1b}"`). A console or a chat line sets `typing` and reads `typed` itself.

### A log that scrolls

A `UiScroll` on a node with a size lets its children be as tall as they are: what is outside the node's padding is cut off, and `UiScroll.y` is how far down it is scrolled. The padding stays where it is, so a panel's border is never drawn over. With `end` (the default), a node scrolled to the end stays at the end as lines are added, so a log keeps its newest line in view. The mouse wheel over it scrolls it back, by `step` pixels a notch; a log scrolled back stays where it was put while lines come, and follows again once scrolled to the end. The layout writes `content` (how tall the lines are), `view`, and `at_end`.

- **Its size.** A fixed size, a `fill`, or `grow` gives it its height. A node sized by its content with a `max` (`size = [0, 0]`, `max = [0, 204]`) grows with its lines up to the max, and scrolls past it.
- **`y` stays in range.** The layout keeps `y` between 0 and `content - view`: a script that sets 5000 gets the end.
- **Scrolling to a line.** `UiScroll.to = <child>` brings that child into view at the next layout: all of it if it fits, with the least scroll; its top at the top if it is taller than the view. The layout sets `to` back to none. A line a script adds this tick is laid out this tick, so it can set `to` as it adds the line: a game that shows each turn from its first line sets `to` to the turn's first line.
- **The wheel** scrolls what is under the pointer. A wheel event does not move the pointer, so a test moves the mouse over the node first (`t.input:pointer`).
- **In the semantic frame**, a line scrolled wholly out of view is not listed; one partly in view is listed with all its words.

Delve's message log is one (`examples/delve/scenes/delve/log.toml`, `systems/messages.luau`): a node with a size and a `UiScroll`, its newest line `@messages` last by `order`, and each turn's words before it moved up into a line of their own, the oldest let go after 40.

```toml
[UiRoot]
layer = 1

[UiNode]
absolute = true
offset = [10.0, 38.0]
size = [560.0, 66.0]
pad = [10.0, 3.0]

[UiScroll]
```

The wheel is an `axis` action bound `mouse = ["wheel"]`, the tree's `scroll` action (`ui_scroll` unless `UiRoot.scroll` names another):

```toml
[actions.ui_scroll]
type = "axis"
doc = "Scroll the message log: the mouse wheel over it"
[actions.ui_scroll.bindings]
mouse = ["wheel"]
```

The tree has to be `interactive` (the default) to be scrolled. In a turn-based game a turn of the wheel takes no turn.

Button prompts come from `InputGlyph`: `action = "restart"` fills `InputGlyph.label` with what the player presses on the device they last used ("R" on a keyboard, the pad's name for the button on a pad), for a `{InputGlyph.label}` hole or a script.

### Fonts

Without a font the UI draws in the built-in pixel font (ASCII and a set of accented letters). A `.ttf` or `.otf` under `assets/` is baked into a distance field, sharp at any size, and named in `ui.toml`'s `font` or a `UiText`'s `font`. It bakes printable ASCII and Latin-1; a sidecar's `chars` adds a language's other letters. Delve and Outpost use Liberation Sans, licence beside it.

### Not drawn: Describe

The `Describe` component is not drawn. It is what the semantic frame, and so an agent and a screen reader, says about an entity. Swarm's wave line was only a `Describe` with `role = "hud"` (on `@director`), so an agent read it but a player saw nothing; its HUD is now a UI tree as well (`prefabs/hud.toml`). Text a player must see is UI. See [content](content.md#accessibility).

## Audio

`audio.toml` at the project's root defines the mixer's buses, ducking rules, sounds, and music. Without it there are buses `master`, `music`, `sfx`, `ui`, and `ambience`, and any `assets/<name>.wav` or `.ogg` plays as a sound named by its path. From `examples/hollow/audio.toml`:

```toml
[buses]
master   = { gain_db = 0 }
sfx      = { parent = "master", gain_db = -4 }
music    = { parent = "master", gain_db = -8 }
dialogue = { parent = "master", gain_db = -2 }

# A line said ducks the music.
[[duck]]
when = "dialogue"
duck = "music"
db = -6
attack_ms = 200
release_ms = 400

[sounds."sfx/step"]
gain = 0.3
pitch_jitter = 0.12
variations = [ { wave = "noise", ms = 35, release_ms = 25 }, { wave = "noise", ms = 30, release_ms = 30 } ]
```

### Sounds

A sound plays one of its `variations` each time, picked from the run's seed: a file path (WAV or Ogg Vorbis, under `assets/`) or a synthesised `tone` (`sine`, `square`, `saw`, `triangle`, or `noise`, with `freq`, `ms`, `slide`, or `notes`). Tones let a game have sound before any audio files exist; Hollow, Delve, Swarm, and Outpost use nothing else. `gain_jitter` and `pitch_jitter` vary each play, `max_voices` caps how many play at once, and `loop` loops. A sound can also be a sidecar under `assets/` with `type = "sound"` and the same keys.

A tone is also a sound-effect synthesiser. Beside its wave, pitch, length, and envelope it takes `punch` (the start louder than the end), `slide_accel` (a slide that eases off), `jump` and `jump_ms` (a second note partway, as a coin has), `vibrato` and `vibrato_hz`, `duty` and `duty_sweep` (a thinner square), `repeat_ms` (a power-up's steps), `lowpass`, `highpass`, and their sweeps, `resonance`, `phaser_ms`, and `noise_pitch` (noise with a pitch, for a rumble). Every one is off unless you give it; `argus docs files section=audio.toml` says what each does. A tone is the same samples on every machine and in the browser.

You need not start from nothing: the `generate` tool (or `argus generate`) makes a sound effect from a kind and a seed, free and offline, and writes it as a sound asset you then change:

```sh
argus generate name=sfx/zap sound=laser seed=3     # assets/sfx/zap.toml
argus generate name=sfx/zap2 from=sfx/zap vary=0.15 # another take on it
```

The kinds are `pickup`, `laser`, `explosion`, `powerup`, `hit`, `jump`, `blip`, `step`, and `click`; another seed is another take on the same kind. The file says the command that made it, and it is an edit like any other: in the change feed with its author and note, undone with `edit undo=true`. It will not write over a file that is there unless you say `replace=true`. The answer says what the sound is like (below), so an agent can check a jump rises before anyone listens.

A script plays a sound by name:

```lua
q:play("sfx/hit", { entity = e })
```

The options are `entity` (where it plays from), `gain`, `pitch`, `bus`, `pan`, and `loop`. A sound is **spatial** by default: it is quieter and panned by its distance from the entity with an `AudioListener`, or from the drawing camera when there is none (`[spatial] min_distance` and `max_distance` in `audio.toml`). Give it `spatial = false` for UI and music-like sounds. Animator events, dialogue lines, and timeline cues play sounds by name too.

`q:play` is a `sound.play` event, `{sound, entity, gain, pitch, ...}` as it was called, which a scenario counts like any event (`t.expect:events("sound.play", { sound = "sfx/hit" })`); the mixer answers it with `sound.played`. `q:play` returns nothing, so a script cannot name the voice it started. To stop sounds from a script, emit `sound.stop`. With `sound`, it stops every voice of that sound: `q:emit("sound.stop", { sound = "sfx/hum", fade_ms = 300 })` ends a hum's loop, spatial or not. With `entity`, it stops the spatial sounds playing from that entity.

### Music

A track is a set of looping layers. The `Music` component plays a `track`, and its `intensity` (0 to 1) fades layers in: a layer is silent below its `from` and full at its `to`. From Hollow:

```toml
[music.cave]
bpm = 80
fade_ms = 400
layers = [ { sound = "music/cave.low" }, { sound = "music/cave.high", from = 0.3, to = 0.8 } ]
```

With a `bpm`, a change of track waits for the next bar of the track playing and cross-fades over `fade_ms`; `change_on = "beat"` or `"now"` changes when. Adaptive music is a system writing the two fields. Hollow picks the track by the hero's room and the intensity by height (`examples/hollow/systems/music.luau`); Swarm's intensity is how many enemies are near:

```lua
local m = q:entity("@director"):get("Music")
m.intensity = math.min(1, n / 30)
```

### Checking sound without ears

`argus audio describe sound=sfx/jump` says what a sound is like (each variation's own samples, before the sound's `gain`), measured from the samples the mixer plays, a file's or a tone's: its length, its peak and loudness, when it is loudest and when it has faded, how its pitch moves (where it has one) and how bright it is, in numbers and in a sentence: "0.17 s long, peak -6.0 dB; loudest at 0.00 s, faded to a tenth by 0.16 s; a note starting near 560 Hz at 0.00 s, ending near 826 Hz at 0.14 s (higher)".

Every play is an event (`sound.play` asked, `sound.played` with the variation chosen and the gain and pitch it played at, its jitter included, or `sound.missing`), and music changes are `music.started`, `music.layer`, and `music.queued`. `argus audio status` shows the buses with their levels and ducking, the voices, and the music. `argus audio clip` writes the last seconds of the mix as a WAV for a person to hear. Headless, the mixer runs on sim time, so the clip is the same every run.

## Debug drawing

A system can draw shapes in the world to show what it is thinking: where a ray went, what a guard can see, the path it chose, a number over an enemy.

```lua
local hit = q:raycast(eye, target, { ignore = me })
q:draw_line(eye, hit and hit.point or target, { color = hit and "#ff4040" or "#40ff40" })
q:draw_text(me, "hp " .. hp.now)
```

There are `q:draw_line(from, to)`, `q:draw_arrow(from, to)`, `q:draw_box(at, size)`, `q:draw_circle(at, r)`, and `q:draw_text(at, text)`. A point can be an entity, which means where it is. The last argument is optional and takes `color` (`"#rrggbb"` or `"#rrggbbaa"`; amber when not given), `width` (in UI pixels, so a line is as thick at any zoom), `fill = true` for a box or circle, and `size` for text.

Shapes are drawn over the world and under the UI, and the light and the post effects leave them alone. A system's shapes stay up until that system runs again. In a real-time game that means every tick. In a turn-based game they stay up between turns. When a system stops drawing something, the shape goes the next time the system runs.

They are for development. The daemon's window, the editor's viewport, and `obs.frame` pictures show them. The semantic frame lists them under `debug`, in the picture's pixels with the system that drew each, so an agent can read them without looking. A release build draws nothing, and a draw there costs almost nothing. Shapes are not part of the world: no digest, keyframe, replay, or save holds them, so leaving a draw in changes nothing about how the game plays.

## Where to go next

- [Gameplay](gameplay.md): input, physics, navigation, events, and timers.
- [Content](content.md): dialogue, timelines, strings, and accessibility.
- [Tuning and performance](tuning-and-performance.md): changing these numbers while the game runs.
- [Testing](testing.md): picture goldens and checks on what is on screen.
