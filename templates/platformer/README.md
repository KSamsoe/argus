# platformer

A side-on platformer start for Argus. The hero runs and jumps from left to right: over a pit, over a row of spikes, up onto a ledge, to a flag. On the way there are three coins to pick up. Falling into the pit or touching the spikes puts the hero back at the start. Reaching the flag says how the run went, and R plays it again. Escape pauses.

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
- `systems/` holds the rules, one file each: `hazards.luau` (the pit and the spikes), `pickups.luau` (coins), `goal.luau` (the flag, and starting again), `hud.luau` (the line at the top), and `pause.luau`. `looks.luau` is how the level is drawn (below); it changes nothing about how the game plays.
- `tests/` holds the scenarios that must keep passing: the whole level played start to finish, the ledge jump, the hazards, and the pause. Run them with `argus test`.
- `input.toml` lists the actions and the keys and buttons for each.

The engine does the running, jumping, and colliding. Your scripts only say what touching things means.

## The art

Every picture is pixel art drawn by one file, `tools/art.py`, which writes the PNGs into `assets/` with a sidecar `.toml` beside each saying how it is cut into frames. The art is 15 pixels to a world unit, so on a 960 x 540 screen each pixel of it is 3.

- `assets/sprites.png` is everything in the level, one atlas: the hero's frames, the coin turning, the flag waving, its pole, the spikes, the ground's tiles, and a sparkle. Its sidecar lists the frames by number and names the clips.
- `assets/backdrop.png` is the sky's colours and three strips behind the level: clouds, far hills, and near hills with trees.
- `assets/ui/panel.png` is the frame round the HUD and the pause panel.
- `assets/anim/` holds the animators that pick frames: `hero.toml` (idle, run, jump, fall, land, from how the hero's body moves), `coin.toml`, and `flag.toml`. `assets/fx/` holds the particle effects: dust where the hero jumps and lands, sparkles where a coin is taken and at the flag.
- `systems/looks.luau` puts it together. It lays grass and earth tiles over every solid block (planks over a one-way one), so a block you add, of any size, is dressed with no art of its own. It slides the backdrop layers (`scenes/level/clouds.toml`, `hills.far.toml`, `hills.near.toml`, each with a `Parallax` from `components/looks.luau`) slower than the level, so they look far away. It turns the hero to face the way it runs, and bursts the dust and the sparkles.

To remake the art, run `python tools/art.py` from this folder (Python 3, nothing to install). It writes the same files every time, so you can change one thing and see only that change.

What to change first: the colours. They are named at the top of `tools/art.py` (`CAP`, `TUNIC`, `GRASS`, `EARTH`, `SKY`, ...); change one and run it. Then the shapes: the hero is drawn as rows of letters, one a pixel (`HEAD`, `BODIES`, `LEGS`), so you can redraw it in a text editor. To use art of your own instead, replace a PNG and keep its frames where the sidecar says, or give `[Sprite]` an `atlas` of your own. A release leaves `tools/` out.

## Ideas for what to add next

1. **More level.** Copy `ground.end.toml` or `ledge.toml` and change its place and size: `systems/looks.luau` dresses it the tick it appears. Then widen `bounds` in `scene.toml` and move the flag and the right wall. Add a platform with `one_way = true` on its `KinematicBody`, so the hero can jump up through it from below; it is drawn as a plank.
2. **A moving platform or an enemy.** Give a platform a `velocity` and write a small system that turns it around at each end. The engine already carries the hero along on a solid that moves, and the platform's tiles move with it, being its children. An enemy that walks back and forth is the same idea, with a `Hazard` on it.
3. **A double jump or a dash.** Write a system that runs after `physics.control` (`after = { "physics.control" }`) and changes the hero's `KinematicBody.velocity` when jump is pressed in the air. ARGUS.md's section on `CharacterController` explains where such a system fits. Add a scenario for it in `tests/`.
