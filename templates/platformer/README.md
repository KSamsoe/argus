# platformer

A side-on platformer start for Argus. The hero runs and jumps from left to right: over a pit, over a row of spikes, up onto a ledge, to a flag. On the way there are three coins to pick up. Falling into the pit or touching the spikes puts the hero back at the start. Reaching the flag says how the run went, and R plays it again. Escape pauses.

Everything is drawn with plain rectangles and circles, so there are no images to replace before you start.

## Controls

| action | keyboard | gamepad |
|---|---|---|
| run | arrow keys or A / D | left stick or d-pad |
| jump (hold it to jump higher) | Space or Z | A (south) |
| start again | R | Select |
| pause | Escape or P | Start |

## The files that matter

- `scenes/level/` is the level, one file per thing in it. `scene.toml` has a map of where everything is. `player.toml` is the hero, and its `CharacterController` is where the jump and the run speed are set. `camera.toml` is the camera that follows the hero.
- `prefabs/` holds the pieces the level is built from: `platform.toml` (a solid block), `coin.toml`, and `spikes.toml`. A block in the level is a small file that names its prefab and gives its place and size.
- `components/level.luau` holds the game's own data: `Hero`, `Hazard`, `Coin`, and `Goal`.
- `systems/` holds the rules, one file each: `hazards.luau` (the pit and the spikes), `pickups.luau` (coins), `goal.luau` (the flag, and starting again), `hud.luau` (the line at the top), and `pause.luau`.
- `tests/` holds the scenarios that must keep passing: the whole level played start to finish, the ledge jump, the hazards, and the pause. Run them with `argus test`.
- `input.toml` lists the actions and the keys and buttons for each.

The engine does the running, jumping, and colliding. Your scripts only say what touching things means.

## Ideas for what to add next

1. **More level.** Copy `ground.end.toml` or `ledge.toml` and change its place and size. Then widen `bounds` in `scene.toml` and move the flag and the right wall. Add a platform with `one_way = true` on its `KinematicBody`, so the hero can jump up through it from below.
2. **A moving platform or an enemy.** Give a platform a `velocity` and write a small system that turns it around at each end. The engine already carries the hero along on a solid that moves. An enemy that walks back and forth is the same idea, with a `Hazard` on it.
3. **A double jump or a dash.** Write a system that runs after `physics.control` (`after = { "physics.control" }`) and changes the hero's `KinematicBody.velocity` when jump is pressed in the air. ARGUS.md's section on `CharacterController` explains where such a system fits. Add a scenario for it in `tests/`.
