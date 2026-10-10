# Arena

A top-down arena shooter start. You walk the hero (the gunner in the yellow helmet) around a walled arena with WASD, the arrow keys, or the left stick. It fires on its own at the nearest enemy in range. Enemies come in waves from the arena's edges and walk straight at you. Each one takes two hits. A touch costs you one health, then you are safe for a moment, blinking. Each wave is bigger than the last. The HUD shows the wave, the enemies left, your score, and your health. Escape (or P, or Start) pauses. At zero health it is game over, and Enter (or R, or the south button) plays again.

## The files that matter

- `components/arena.luau`: the game's data. Player (the hero's speed, fire rate, range), Health, Enemy, Bullet, and Waves (how many enemies come and how fast). The fields marked `tune` show up in the editor's tuning panel.
- `systems/`: the rules, one file each, in the order they run each tick:
  - `pause.luau` pauses the game.
  - `restart.luau` starts a new run after game over.
  - `move.luau` walks the hero and keeps it inside the walls.
  - `waves.luau` sends each wave in from the edges.
  - `chase.luau` walks the enemies at the hero.
  - `fire.luau` shoots at the nearest enemy.
  - `bullets.luau` flies the bullets until they reach the wall.
  - `hits.luau` handles bullets hitting enemies and enemies touching the hero.
  - `looks.luau` runs after all of them and only changes how things are drawn (see The art).
- `prefabs/`: what the systems spawn. `enemy.toml` is an enemy and `bullet.toml` a bullet: their components and starting values. `waves.luau` and `fire.luau` spawn them with `q:spawn_prefab`, giving only what differs each time (where it starts, its speed or velocity). Change a prefab and every one spawned after it changes.
- `scenes/main/`: one file per thing in the scene.
  - `arena.toml` is the floor. Its size is the arena's size, and it holds the wave numbers.
  - `player.toml` is the hero.
  - `camera.toml` is the camera, the room's light, and the red flash and shake when you are hit.
  - `hud.toml`, `paused.toml` and `game_over.toml` are the text on screen.
  - `wall.toml` is the arena's edge, and `braziers.toml` the fires on its corners.
- `input.toml`: the actions (move, pause, restart) and their keys and buttons.
- `ui.toml`: the colours the HUD and the menus use.
- `tests/`: four scenarios that must keep passing:
  - enemies come from the edges and close in;
  - a hero who stands still clears wave 1;
  - a touch costs health, and game over and restart work;
  - the hero walks and stops at the wall, and pause freezes the game.

  Run them with `argus test`.

## The art

All the pictures are drawn by one script, `tools/art.py`, as pixel grids and by simple rules, and the PNGs it writes are kept in `assets/`. You only need Python (no extra packages) to change them: edit the script, then run `python tools/art.py` from the project's folder. It writes the same files every time.

- `assets/sprites/actors.png` holds the hero (three views, two steps each, a hit flash, fallen), the enemy (two steps, and cracked once hit), the bullet, the braziers, and the specks the effects are made of. `actors.toml` beside it lists the frames by number.
- `assets/sprites/room.png` holds the floor and the walls. `arena.toml` draws the floor, `wall.toml` the walls.
- `assets/ui/panel.png` is the frame the HUD and the menus sit in.
- `assets/fx/` holds the effects (`spark`, `puff`, `flame`) and two materials: `glow` (drawn added, for the bullets and fire) and `body` (the hero and enemies at full colour, however dark the room).

The enemies and bullets spawn as plain shapes in their prefabs' colours. `systems/looks.luau` swaps in the pictures, keeping those colours as the tint, so a different colour in `prefabs/enemy.toml` still changes the enemies. It also picks the frames each tick: the hero turns toward what it shoots at, steps while it walks, and flashes when hit. Each enemy walks, faces the hero, and has a shadow. It starts the sparks where a bullet hits and a puff where an enemy dies. It reads the game and writes only how things look, so the game plays the same without it.

What to change first:

- **Colours**: the named colours at the top of `tools/art.py` (floor, walls, the hero's palette), then run it. The UI's colours are in `ui.toml`.
- **The hero or the enemy**: their pictures in `tools/art.py`, one letter a pixel.
- **The light**: `Lighting` in `camera.toml` (how dark the room is), the hero's `Light` in `player.toml`, and the braziers in `braziers.toml`.
- **A bigger arena**: change the sizes in `arena.toml`, `wall.toml`, and the camera's height, then set `ARENA_W` and `ARENA_H` in `tools/art.py` to match and run it, so the floor is drawn at the new size.

The art is drawn at 12 pixels a world unit, and the camera shows 24 screen pixels a unit at 960 x 540. Each art pixel is two screen pixels, so it stays sharp. Keep the camera's height at 22.5, or another height that keeps that ratio a whole number.

## Ideas for what to add next

1. **Aim and fire yourself.** Add an `aim` action (the right stick, or the mouse) and a `fire` button to `input.toml`. Then change `systems/fire.luau` to shoot along the aim while fire is held.
2. **A second kind of enemy.** Give some enemies more health and a different colour, or a dash: a short wind-up, then a fast run at where the hero was. Spawn them from wave 3 on, in `systems/waves.luau`. They can use their own component, read in `systems/chase.luau`.
3. **Pickups.** Have a killed enemy sometimes drop a health or fire-rate pickup, in `systems/hits.luau`. Give that system its own random stream (`rng = "drops"`) and draw from it with `q:rng()`, so the same seed always drops the same pickups. The hero collects it by walking over it (`q:within`). Add a scenario that checks a pickup heals.
