# roguelike

A turn-based dungeon crawl to build on. The hero (in blue, with a sword and
a shield) starts in the left room of a small hand-drawn dungeon: two rooms
and a corridor. Every key press is one turn. The hero steps one cell, and
rock blocks it. Walking into a monster attacks it. Then each monster that
can see the hero bites if it is next to it, or steps one cell toward it,
finding its way round the walls. The seed decides where the rats and
goblins start. When the hero's hit points reach zero the run is over, and R
starts a new one.

| Action  | Keyboard        | Gamepad |
|---------|-----------------|---------|
| move    | arrows or WASD  | d-pad   |
| wait    | period or space | B       |
| restart | R               | A       |
| pause   | Escape          | Start   |

## The files that matter

- `systems/dungeon.luau`: the map, drawn as a picture of `#`, `.` and `@`,
  plus the monster kinds and how many there are. Start here.
- `systems/hero_turn.luau`: the hero's turn (step, attack, bump into rock,
  or wait).
- `systems/monsters.luau`: the monsters' turn. Each one reads the hero's
  flow field: every open cell's count of steps to the hero.
- `systems/messages.luau`: the message line ("You hit the rat."), made from
  the events the turn emitted.
- `systems/pause.luau`: the pause. While it shows, no turn passes.
- `systems/look.luau`: how it all looks (see "The art" below). It decides
  nothing about the game.
- `components/roguelike.luau`: Health, Player, Monster, Dungeon, Turn.
- `scenes/dungeon/`: the one scene, one entity per file. The hero is in
  `player.toml` and the map in `dungeon.toml`. The camera, the dark behind
  the map (`backdrop.toml`), the HUD line, and the message line each have
  their own file; the pause screen is `pause.toml` and its plate and words
  (`pause.panel`, `pause.title`, `pause.text`).
- `maps/dungeon.tilemap.toml`: the grid. Its floor and walls layers are the
  game's: what is open and what blocks. Its stone and shade layers are the
  look's, and nothing collides with them.
- `tests/`: four scenarios that must keep passing. Rock blocks the hero and
  bumping it costs no turn. A monster steps toward the hero after the
  hero's turn, then bites. Walking into a monster hurts it until it dies.
  The hero's death ends the run, and restart begins a new one. They use
  cells of the drawn map's left room (the hero starts at (3, 6)), so if you
  redraw that room, move those cells in the tests to match.

A turn runs in this order: `sim.dungeon` (only when a run starts), then
`sim.hero_turn`, then `sim.monsters`, then `sim.messages`; after them
`late.look` dresses what they did. Run `argus test` after every change.

## The art

Everything you see is pixel art drawn by `tools/art.py`, a Python script
(standard library only) that writes the PNGs in `assets/` and the sidecar
`.toml` beside each, which cuts it into frames. The PNGs are kept in the
project, so you need Python only to change them. To remake them after an
edit, run `python tools/art.py` from the project's folder; it writes the
same files every time.

- `assets/tiles/dungeon.png`: the dungeon's tiles, 16 x 16 pixels a cell:
  flagstones, brick wall faces (some with a torch), the rock beside a room,
  the shadow a wall casts, and what a fight leaves. Its sidecar lists every
  frame.
- `assets/sprites/creatures.png`: a row each for the hero, the rat, the
  goblin, and a bat (drawn, not in the game yet): two idle frames, a hit
  flash, and a bite (the hero's fourth is fallen).
- `assets/ui/panel.png` and `plate.png`: the plates the HUD, the pause
  screen, and the message line sit on. `ui.toml` holds the text colours.

`systems/look.luau` puts the art on the game. When a run begins it paints
the map's stone and shade layers from its walls, so a dungeon you redraw
(or generate) is dressed by itself; each turn it picks the hero's and the
monsters' frames: the idle frames swap, a creature flashes when hit, and
monsters face the hero. A monster kind it has no art for stays the plain
coloured circle `systems/dungeon.luau` gives it.

What to change first: the colours at the top of `tools/art.py`, then the
creatures' pictures below them, where one character is one pixel. A new
monster is a new row of pictures in `art.py` and a line in `look.luau`'s
`ROWS`.

## Ideas for what to add next

1. A dungeon made from the seed: roll rooms and corridors with the
   dungeon's random stream instead of drawing them. Add stairs down to a
   new floor. The example game Delve (`systems/floor.luau`) does both.
2. Items: a potion a monster sometimes drops, picked up by walking onto it
   and drunk with a new action. Delve's `systems/potions.luau` shows one way.
3. Other monster minds: a bat that flutters at random, a goblin that runs
   when it is hurt, or an archer that keeps its distance. Each is a new
   branch in `systems/monsters.luau` and a new line in `KINDS`. The bat is
   already drawn: a `bat` line in `KINDS` shows it.
