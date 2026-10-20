# Content

This chapter covers the data a game is made of beyond its scripts: data tables, dialogue, timelines and cutscenes, the strings a player reads in each language, and the accessibility features and settings a player can change. All of it is text files an agent can read and edit and a translator or designer can work on without touching code.

## Def tables

A **def table** is a table of rows: items, recipes, job kinds, monster stats. Data that is the same every run belongs here rather than in a scene or a script, so balancing a game means editing a table, not code.

### Declaring a table

A table is declared in Luau, in `defs/` (or `components/` or `systems/`), with its fields as `field.<type>` like a component's. From `examples/outpost/defs/schemas.luau`:

```lua
def "items" {
  doc = "What there is: goods a stockpile holds",
  prefix = "item",
  key = "id",
  id = field.str {},
  name = field.str { doc = "What a player calls it" },
  food = field.f32 { default = 0, range = {0, 10}, tune = true, doc = "Food it is worth eaten, in meals (a meal is 1)" },
  color = field.str { default = "#FFFFFF", doc = "Its placeholder's colour" },
}

def "recipes" {
  doc = "What a station makes from what",
  prefix = "recipe",
  key = "id",
  id = field.str {},
  output = field.ref { doc = "The item made" },
  count = field.f32 { default = 1, range = {0, 100}, doc = "How many are made" },
  input = field.ref { doc = "The item used up" },
  input_count = field.f32 { default = 1, range = {0, 100}, tune = true, doc = "How many are used up" },
  work = field.f32 { default = 1, range = {0, 600}, tune = true, doc = "Seconds of a cook's work" },
}
```

`key` is the field that names a row (`id` by default) and `prefix` is how other data refers to a row (the table's name by default). A row is referred to as `<prefix>:<key>`: `item:meal`.

### Writing rows

The rows are in `defs/<table>.toml`, the file named after the table (not its prefix). Each top-level table is one row, its header the row's key. From `examples/outpost/defs/recipes.toml`:

```toml
# Two crops, cooked, feed a colonist for a day; eaten raw they are 0.8.
["meal"]
output = "item:meal"
count = 1
input = "item:crop"
input_count = 2
work = 3.0
```

A field left out takes its default. A `ref` field must name a row that exists (`item:crop`), or the project reports it at its file and line and no scene spawns until it is fixed. So must every element of a list of refs: a pool written `pool = ["monster:rat", "monster:rta"]` is refused when the project loads, naming the element (`rooms.cellar.pool, element 2`), not when some seed draws it. A list field (`field.list { of = "ref" }`, see [Scripting](scripting.md#list-fields)) is a TOML array. Quote a header with dots in it (`["hermit.start"]`): an unquoted `[hermit.start]` is the row `hermit` with a table inside. A row cannot hold an entity.

### Reading rows from a script

`q:def("item:meal")` gives one row and `q:defs("recipes")` every row of a table in file order. Rows are read-only. Besides its fields, a row has `key` (`"meal"`) and `ref` (`"item:meal"`). A `ref` field reads as the reference it holds, a string (`r.output` is `"item:meal"`), so `q:def(r.output)` is the row it names. From `examples/outpost/systems/kitchen.luau`:

```lua
local r = q:def(st.recipe)
local food = q:def(r.input).food
```

A common pattern is to make live entities from rows when the game starts. Outpost spawns a `JobKind` for each row of its `jobs` table, so a table edit changes a new colony and a tuning slider the running one (`examples/outpost/systems/found.luau`):

```lua
for _, row in ipairs(q:defs("jobs")) do
  if not have[row.key] then
    local kind = q:spawn({ "JobKind" }):get("JobKind")
    kind.name = row.key
    kind.priority = row.priority
    kind.skill = row.skill
    kind.work = row.work
  end
end
```

### Looking at and changing rows

- `argus world defs` lists the tables; `argus world defs table=items` shows one as rows.
- An edit changes one field of a row, kept in the file with a note: `argus edit note='meals feed more' ops='[{op: def, def: item:meal, path: food, value: 1.2}]'`. `def_row` ops add, replace, or remove a whole row. `argus edit undo=true` takes it back.
- Fields with `tune = true` are in the tuning panel, and `argus test sweep=...` can run the scenarios once per candidate value of a row's field (see [tuning and performance](tuning-and-performance.md)).
- A mod changes a game's row field by field by writing a row with the same key (see [releasing](releasing.md)). A list it gives replaces the game's; to change one without writing it all out, a mod's row adds to it with `"+tags" = ["cursed"]` (quoted: `+` is not allowed in a bare TOML key) and takes from it with `-tags = ["damp"]`, every element equal to one given taken out, then the added ones put at the end. In the game's own rows the two change the field's default.

The engine declares two tables of its own, `dialogue` and `timeline`, below.

## Dialogue

Conversations are rows of the engine's `dialogue` table, in `defs/dialogue.toml`. Lines and speakers' names are **string keys**, so the same graph plays in every language and a translator never opens it.

### Nodes, choices, and branches

There are three kinds of row:

| Row | Has | What it is |
|---|---|---|
| A node | no `from` | A `line` said by a `speaker`; then the choices from it, or with none, its `next` |
| A choice | `from` and a `line` | Something the player can pick at the node `from` |
| A branch | `from` and no `line` | A way on from a node with no line, taken when its `when` holds |

An empty `next` ends the conversation. From `examples/hollow/defs/dialogue.toml`:

```toml
["hermit.start"]
next = "dialogue:hermit.hello"

# Met before: another greeting.
["hermit.start.again"]
from = "dialogue:hermit.start"
when = "speaker:Hermit.met"
next = "dialogue:hermit.again"

["hermit.hello"]
speaker = "hermit.name"
line = "hermit.hello"
effects = "speaker:Hermit.met = true"
sound = "sfx/voice"
next = "dialogue:hermit.ask"

["hermit.ask"]
speaker = "hermit.name"
line = "hermit.ask"

# Offered once the hero has found the gem, until it is given.
["hermit.ask.give"]
from = "dialogue:hermit.ask"
line = "hermit.give"
when = "@hud:Run.gems >= 1 and not speaker:Hermit.has_gem"
effects = "speaker:Hermit.has_gem = true; emit hermit.given"
next = "dialogue:hermit.thanks"

["hermit.ask.bye"]
from = "dialogue:hermit.ask"
line = "hermit.bye"
```

`hermit.start` has no line, so it goes to its first branch whose `when` holds (`hermit.start.again` once he has met the hero), else to its own `next`.

### Conditions and effects

A **condition** (`when`) is tests joined by `and` and `or` (`and` binds tighter): `[not] [who:]Component.field [op value]`. The op is `==`, `!=`, `<`, `<=`, `>`, or `>=`. `who` is `listener` (the default), `speaker`, or an entity's `@id`. A bare path is true when it is a true bool, a non-empty string, an entity, or a number that is not zero. A node whose `when` fails is passed over to its `next`, and a choice whose `when` fails is not offered.

**Effects** happen when a node is shown, a choice taken, or a branch followed. They are separated by `;`, each `[who:]Component.field = value`, `+= n`, `-= n`, or `emit <event>`.

The facts a conversation reads and writes are ordinary component fields. Hollow's are a `Hermit` component on the hermit (`met`, `has_gem`), declared in `examples/hollow/components/hermit.luau`.

### Starting a conversation

A conversation is a `Talk` entity. A script spawns one with the node to start at, who speaks, and who listens. From `examples/hollow/systems/hermit.luau`:

```lua
if near and not paused and hero:get("Actions").talk_pressed then
  local t = q:spawn({ "Talk" }):get("Talk")
  t.start = "dialogue:hermit.start"
  t.speaker = hermit
  t.listener = hero
  t.slot = 1
  q:freeze()
end
```

The engine's `dialogue.update` then shows each line in a box of UI nodes at the top of the screen: the speaker's name, the line, and a button for each choice (or a Continue button). The player moves and presses with the menu actions (`ui_move`, `ui_confirm`, the mouse), so the game's `input.toml` needs them. The box uses the theme's `panel`, `button`, and `accent` colours, and its Continue button is the string key `dialogue.continue`. There is no setting to turn the engine's box off and draw your own.

A conversation does not pause the game by itself. Hollow freezes every tick while a `Talk` exists, in an unfrozen input system, as its pause menu does (see [gameplay](gameplay.md#pausing)).

A script, a test, or an agent can also drive it through the `Talk` fields: set `pick` to take a choice (counted from 0), `advance` to go on, or `stop` to end it. `Talk.node`, `line`, `who`, and `choices` say what is showing, in the player's language.

Its events: `dialogue.started {talk, start, speaker, listener}`, `dialogue.line {talk, node, line, speaker, choices}`, `dialogue.chose {talk, node, choice}`, and `dialogue.ended`. A node's `sound` plays as its line shows; Hollow plays its hermit's voice on a `dialogue` bus that ducks the music (see [presentation](presentation.md#audio)).

When a `when` or `effects` does not parse, it is a `script.errors` row (phase dialogue) with its file and line, and the scene still runs. A problem while playing, such as a node that does not exist, is a `dialogue.problem` event.

## Timelines and cutscenes

A **timeline** is cues against time: tweens, animator states, camera cuts, conversations, and events. Cutscenes, boss intros, room transitions, and Outpost's event vignettes are timelines. They are rows of the engine's `timeline` table in `defs/timeline.toml`: a row without `timeline` is a timeline, and a row with `timeline = "timeline:<name>"` is a cue on it, fired `at` seconds in.

### Kinds of cue

| `kind` | What it does | Its fields |
|---|---|---|
| `tween` | Drives `who`'s field from `from` (or where it is) to `to` | `path`, `from`, `to`, `duration`, `easing` |
| `anim` | Puts `who`'s Animator in a state | `state` |
| `camera` | Makes `who`'s Camera draw | `duration` (0: to the end) |
| `line` | Starts a conversation, `who` speaking to `other` | `line` (a dialogue node) |
| `event` | Makes its effects (the dialogue's language) | `effects`, `when` |

Every cue may also play a `sound`, be passed over unless its `when` holds, and `wait`: the timeline holds until the cue is done (its conversation ended, its tween finished, its state played through). `who` is `actor` (the default), `other`, `camera` (the one drawing), or an `@id`. `track` only says which lane an editor draws the cue in.

From `examples/hollow/defs/timeline.toml`, the outro when the hero finds the way out (its second tween, `outro.pull`, which moves the camera along the hall, left out):

```toml
["outro"]
length = 5.0

["outro.wide"]
timeline = "timeline:outro"
kind = "camera"
who = "@wide"

["outro.zoom"]
timeline = "timeline:outro"
track = "wide"
kind = "tween"
who = "@wide"
path = "Camera.height"
from = [12.0, 0.0]
to = [14.0, 0.0]
duration = 2.5
easing = "in_out_sine"

["outro.call"]
timeline = "timeline:outro"
track = "hermit"
at = 3.0
kind = "line"
who = "@hermit"
line = "dialogue:outro.start"
wait = true
```

A tween's `from` and `to` are always two numbers; for a single number such as `Camera.height`, only the first is used. `@wide` is a second camera, `active = false` with a higher priority, so the cut glides to it over its `blend` and back when the timeline ends.

### Playing one

A script plays a timeline by spawning a `Timeline` with its `actor` and `other`. From `examples/hollow/systems/run.luau`:

```lua
local outro = q:spawn({ "Timeline" }):get("Timeline")
outro.name = "timeline:outro"
outro.actor = hero
```

Setting `skip = true` jumps to its end: tweens finish, states are entered, events and edits are made, lines are not said. Hollow skips the outro when the player restarts. A Timeline also has `speed`, `paused`, and `looping`. A room scene can name a timeline to play at every room change (`[rooms] transition = "timeline:rooms.fade"`, see [scenes](scenes.md)).

Its events are `timeline.started`, `timeline.cue`, `timeline.held`, `timeline.resumed`, and `timeline.ended {skipped}`, each with `timeline` (the entity playing it) and `row` (which timeline: its row of the table, such as `outro`). A cue that cannot work (a tween without a path, a `who` that is no one) is a `script.errors` row (phase timeline), and that cue is left out.

From outside the game:

```sh
argus query timelines=outro                                  # its cues by track, and what is playing
argus run 'timeline={name: outro, actor: "@player"}' step=60  # play it now and watch
argus seek timeline=outro time=3                             # scrub to 3 seconds into its latest play
```

## Strings and languages

Every word a player reads should come from a **string table**, one file per language under `strings/`, named by its code: `strings/en.toml`, `strings/fr.toml`. The base language (`[strings] base` in `argus.toml`, `en` by default) is the one every other falls back to, key by key.

Keys are any TOML keys; tables and dotted keys flatten with dots. From `examples/hollow/strings/en.toml`:

```toml
language.name = "English"

[settings]
title = "Settings"
text = "Text size: {percent}%"
language = "Language: {name}"

[controls]
jump = "Jump: {InputBinding.keys}"

[hud]
no_falls = "no falls"
one_fall = "1 fall"
falls = "{n} falls"
```

And `strings/fr.toml` has the same keys in French (`[settings] title = "Réglages"`). By custom each file names its own language in `language.name`, which a settings menu shows.

### Using strings

- A `UiText` with `key = "settings.title"` shows that string in the current language.
- A script asks with `q:text(key, args)`: `q:text("hud.falls", { n = run.falls })`.
- `{name}` holes are filled from a script's `args`. `{Component.field}` holes are filled from the UiText's own entity and `{@id:Component.field}` from a named entity, every layout. Delve's HUD line is `"Floor {@player:Player.floor}, turn {Turn.n}. HP {@player:Health.cur}/{@player:Health.max}. Potions {@player:Player.potions}."`, on an entity that has the `Turn`.
- A key no table has shows as `[key]`, so a missing string is easy to see. A hole naming nothing shows as `{?Component.field}`.
- There are no plural forms. Choose the key in the script, as Hollow does with `hud.no_falls`, `hud.one_fall`, and `hud.falls`.

The engine's own keys, which a table may override: `dialogue.continue`, `dialogue.end`, and `input.press_a_key` (what a controls button shows while it waits). Hollow's French table overrides `input.press_a_key`.

### Switching language

The language is `UiSettings.language` on the world's first `UiSettings` entity: a table's code, or empty for the base. Changing it changes every UI text at once, a conversation in progress included. `q:languages()` lists the languages there are tables for, the base first. Hollow's settings page cycles them (`examples/hollow/systems/pause_menu.luau`):

```lua
if #q:events("settings.language") > 0 then
  local langs = q:languages()
  local at = 1
  for i, l in ipairs(langs) do
    if l == s.language or (s.language == "" and i == 1) then at = i end
  end
  s.language = langs[at % #langs + 1]
end
```

### For translators

- `argus query strings=fr` shows French beside the base, key by key, and every language's count of strings and missing ones. `argus strings list lang=fr missing=true` is the to-do list.
- `argus edit ops='[{op: text, lang: fr, key: hud.no_falls, text: "aucune chute"}]' note='...'` changes one string in a transaction.
- A release build ships each language but the base in a pack of its own, so a language can be added to a shipped game by dropping in its pack (see [releasing](releasing.md)).
- Letters: the built-in pixel font has ASCII and 35 accented letters. A font asset bakes ASCII and Latin-1; its sidecar's `chars` adds more (see [presentation](presentation.md#fonts)).
- `Describe` names and texts may be string keys too: a key the tables have is said in the player's language (see below).

## Accessibility

Most of Argus's accessibility comes from decisions made elsewhere: actions instead of keys, UI theme tokens, and the semantic frame.

### The screen reader feed

The **semantic frame** is the engine's description of what is on screen: the UI's text and buttons in reading order, and the entities in view that have a `Describe`. An agent reads it with `argus observe`. The same frame is the player's screen reader feed: a desktop build hands it to the operating system's screen reader (Narrator, NVDA, JAWS) while one is running, and the web page gives it to the browser's. A press from the screen reader is a click on the button, so it is recorded input and replays like any other.

UI text and buttons are read with nothing more to do. For things in the world, add a `Describe`. From `examples/delve/scenes/delve/player.toml`:

```toml
[Describe]
name = "hero"
text = "HP {Health.cur}/{Health.max}, {Player.kills} kills"
```

| Field | What it is |
|---|---|
| `name` | What it is in a word or two; empty for its id |
| `text` | What it says, with `{Component.field}` and `{@id:Component.field}` holes filled each frame |
| `role` | `hud` (on the screen, listed first), `dialogue`, `menu`, or empty for a thing in the world |
| `order` | Reading order: lower first, then top to bottom, left to right |
| `hidden` | Leave it out (a darkening sheet over the scene, as in `templates/colony/scenes/main/night.toml`) |

A `Describe` is never drawn. Its `name` and `text` may be string keys (`text = "describe.hero"`): a key the tables have is said in the player's language, as the UI is, and other words are said as written. So a player playing in French hears the descriptions in French too, where the game's `strings/fr.toml` has them.

### Text size and high contrast

`UiSettings.text_scale` (0.5 to 4) multiplies every text size, and `UiSettings.high_contrast` swaps in the theme's high-contrast colours from `ui.toml`'s `[high_contrast_colors]`. Hollow and Delve put a `UiSettings` on their settings page entity and cycle the size through 100, 125, and 150 percent. A game whose look depends on darkness should check high contrast in its own systems too: Hollow raises its light level when it is on (`examples/hollow/systems/light.luau`).

### Colour-blind checks

The engine can draw a frame as a colour-blind eye sees it, the UI included: `protanopia`, `deuteranopia`, `tritanopia`, or `achromatopsia`. It is a way of looking, not something a game ships:

```sh
argus obs frame format=png see_as=deuteranopia
```

The editor's viewport has the same choice (See as), and `render.stream` takes `see_as` too.

### Rebinding

A controls page is buttons with an `InputBinding`: pressed, it waits for the next key (Escape cancels) and binds its action to it. `InputBinding.keys` says what the action is bound to now, for a `{InputBinding.keys}` hole. From `examples/hollow/scenes/hollow/rooms/persistent/controls.jump.toml`:

```toml
[Parent]
entity = "@controls"

[UiNode]
pad = [18.0, 8.0]
order = 1

[UiPanel]
color = "button"

[UiButton]

[InputBinding]
action = "jump"

[UiText]
key = "controls.jump"
```

with `controls.jump = "Jump: {InputBinding.keys}"` in the string table. `device` picks `keyboard` (the default), `mouse`, or `gamepad`. Hollow, Delve, Swarm, and Outpost each have a controls page.

### Hold or toggle

An action in `input.toml` can be a toggle (`toggle = true`). A settings button with an `InputToggle {action}` lets the player switch a button action between holding and toggling. No example game has one yet.

### Button prompts

`InputGlyph` says what to press for an action on the device the player last used (see [presentation](presentation.md#menus-and-buttons)), so a prompt reads "Press R" on a keyboard and names the pad's button on a pad.

## The player's settings

A player's choices live outside the game's world, so a replay is the same whoever plays it:

- **In the world**: the first `UiSettings` entity holds `text_scale`, `high_contrast`, `fullscreen`, and `language`. The game's settings page writes them; the UI and the window read them. Fullscreen is applied by the release window and the web page each frame.
- **On the player's machine**: a release build writes `settings.toml` in the player's save folder (`%APPDATA%\<name>` on Windows, `~/Library/Application Support/<name>` on macOS, `~/.local/share/<name>` elsewhere), and the web page keeps the same text in the browser. It is read before the first tick. Only what differs from the game's own files is written.

From a file a game wrote:

```toml
[ui]
text_scale = 1.25
high_contrast = true
fullscreen = false
language = ""

[bindings.jump]
keyboard = ["x"]

[toggles]
crouch = true
```

The daemon never reads `settings.toml`, so a project's tests always run with the game's own settings. A file that does not parse, or a binding that does not apply, is passed over; it never stops the game.

## Where to go next

- [Gameplay](gameplay.md): input, events, and saves.
- [Presentation](presentation.md): the UI, fonts, and sound these build on.
- [Scenes](scenes.md): rooms and their transition timeline.
- [Testing](testing.md): checking a conversation or a timeline in a scenario.
- [Releasing](releasing.md): language packs and mods.
