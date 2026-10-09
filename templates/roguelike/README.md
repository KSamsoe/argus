# roguelike

A turn-based dungeon crawl to build on. The hero (the pale circle) starts in
the left room of a small hand-drawn dungeon: two rooms and a corridor. Every
key press is one turn. The hero steps one cell, and rock blocks it. Walking
into a monster attacks it. Then each monster that can see the hero bites if
it is next to it, or steps one cell toward it, finding its way round the
walls. The seed decides where the rats and goblins start. When the hero's
hit points reach zero the run is over, and R starts a new one.

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
- `components/roguelike.luau`: Health, Player, Monster, Dungeon, Turn.
- `scenes/dungeon/`: the one scene, one entity per file. The hero is in
  `player.toml` and the map in `dungeon.toml`. The camera, the HUD line,
  the message line and the pause screen each have their own file.
- `maps/dungeon.tilemap.toml` and `assets/tiles/`: the grid. The floor
  layer is what you see; the walls layer is what blocks.
- `tests/`: four scenarios that must keep passing. Rock blocks the hero and
  bumping it costs no turn. A monster steps toward the hero after the
  hero's turn, then bites. Walking into a monster hurts it until it dies.
  The hero's death ends the run, and restart begins a new one. They use
  cells of the drawn map's left room (the hero starts at (3, 6)), so if you
  redraw that room, move those cells in the tests to match.

A turn runs in this order: `sim.dungeon` (only when a run starts), then
`sim.hero_turn`, then `sim.monsters`, then `sim.messages`. Run `argus test`
after every change.

## Ideas for what to add next

1. A dungeon made from the seed: roll rooms and corridors with the
   dungeon's random stream instead of drawing them. Add stairs down to a
   new floor. The example game Delve (`systems/floor.luau`) does both.
2. Items: a potion a monster sometimes drops, picked up by walking onto it
   and drunk with a new action. Delve's `systems/potions.luau` shows one way.
3. Other monster minds: a bat that flutters at random, a goblin that runs
   when it is hurt, or an archer that keeps its distance. Each is a new
   branch in `systems/monsters.luau` and a new line in `KINDS`.
