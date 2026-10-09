# Arena

A top-down arena shooter start. You walk the hero (the yellow circle) around a walled arena with WASD, the arrow keys, or the left stick. It fires on its own at the nearest enemy in range. Enemies come in waves from the arena's edges and walk straight at you. Each one takes two hits. A touch costs you one health, then you are safe for a moment, blinking. Each wave is bigger than the last. The HUD shows the wave, the enemies left, your score, and your health. Escape (or P, or Start) pauses. At zero health it is game over, and Enter (or R, or the south button) plays again.

Everything is drawn as plain rectangles and circles, so there are no assets to replace before you start.

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
- `scenes/main/`: one file per thing in the scene.
  - `arena.toml` is the floor. Its size is the arena's size, and it holds the wave numbers.
  - `player.toml` is the hero.
  - `camera.toml` is the camera.
  - `hud.toml`, `paused.toml` and `game_over.toml` are the text on screen.
  - `wall.toml` is the arena's edge.
- `input.toml`: the actions (move, pause, restart) and their keys and buttons.
- `tests/`: four scenarios that must keep passing:
  - enemies come from the edges and close in;
  - a hero who stands still clears wave 1;
  - a touch costs health, and game over and restart work;
  - the hero walks and stops at the wall, and pause freezes the game.

  Run them with `argus test`.

## Ideas for what to add next

1. **Aim and fire yourself.** Add an `aim` action (the right stick, or the mouse) and a `fire` button to `input.toml`. Then change `systems/fire.luau` to shoot along the aim while fire is held.
2. **A second kind of enemy.** Give some enemies more health and a different colour, or a dash: a short wind-up, then a fast run at where the hero was. Spawn them from wave 3 on, in `systems/waves.luau`. They can use their own component, read in `systems/chase.luau`.
3. **Pickups.** Have a killed enemy sometimes drop a health or fire-rate pickup, in `systems/hits.luau`. Give that system its own random stream (`rng = "drops"`) and draw from it with `q:rng()`, so the same seed always drops the same pickups. The hero collects it by walking over it (`q:within`). Add a scenario that checks a pickup heals.
