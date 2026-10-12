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

Three art scripts draw every picture: Luau files the engine runs to make each one, so changing the art needs nothing but Argus. Change one and save it, and the game shows the change while it runs. A release build carries the pictures, not the scripts. The art is 15 pixels to a world unit, so on a 960 x 540 screen each pixel of it is 3.

- `assets/sprites.art.luau` draws everything in the level, one sprite of 36 frames: the hero's frames, the coin turning, the flag waving, its pole, the spikes, the ground's tiles, and a sparkle. Its first lines list the frames by number (the game picks them by number, so keep them in that order), and it names the clips the animators play.
- `assets/backdrop.art.luau` draws the sky's colours and three strips behind the level: clouds, far hills, and near hills with trees.
- `assets/ui/panel.art.luau` draws the frame round the HUD and the pause panel, as rows of text.
- `assets/shared.lib.luau` is a module, not a picture: what the sprites and backdrop scripts share, which each takes with `require("shared")`. It holds the art's scale (15 pixels a unit) and the helpers that round numbers and mix colours. Change it and both scripts draw again; a release leaves it out.
- `assets/anim/` holds the animators that pick frames: `hero.toml` (idle, run, jump, fall, land, from how the hero's body moves), `coin.toml`, and `flag.toml`. `assets/fx/` holds the particle effects: dust where the hero jumps and lands, sparkles where a coin is taken and at the flag.
- `systems/looks.luau` puts it together. It lays grass and earth tiles over every solid block (planks over a one-way one), so a block you add, of any size, is dressed with no art of its own. It slides the backdrop layers (`scenes/level/clouds.toml`, `hills.far.toml`, `hills.near.toml`, each with a `Parallax` from `components/looks.luau`) slower than the level, so they look far away. It turns the hero to face the way it runs, and bursts the dust and the sparkles.

What to change first: the colours. They are named at the top of each script (`CAP`, `TUNIC`, `GRASS`, `EARTH`, `SKY`, ...); change one and save it. Then the shapes: the hero is drawn as rows of letters, one a pixel (`HEAD`, `BODIES`, `LEGS` in `assets/sprites.art.luau`), so you can redraw it in a text editor. The grass and earth tiles and the trees are laid at random from seeds, so they come out the same every time; change a seed and they come out another way. To use art from a paint program instead, put `sprites.png` (with a `sprites.toml` cutting it into the same frames, in the same order, and naming the same clips) in place of `sprites.art.luau`, and the same for the others, or give `[Sprite]` an `atlas` of your own. `argus docs presentation section=Art scripts` says what a script can draw, and `argus docs files section=art.luau` how scripts share a module.

## Ideas for what to add next

1. **More level.** Copy `ground.end.toml` or `ledge.toml` and change its place and size: `systems/looks.luau` dresses it the tick it appears. Then widen `bounds` in `scene.toml` and move the flag and the right wall. Add a platform with `one_way = true` on its `KinematicBody`, so the hero can jump up through it from below; it is drawn as a plank.
2. **A moving platform or an enemy.** A platform is a `KinematicBody` with `solid = true`: give it a `velocity` and write a small system that turns it around at each end. The engine already carries the hero along on a solid that moves, and the platform's tiles move with it, being its children. An enemy that walks back and forth is an actor instead (a `KinematicBody` that is not solid, so it and the hero pass through each other), with a `gravity` of its own (only a `CharacterController` sets one; the hero's comes to about 50), a `Collider`, and a `Hazard`: touching it is an `overlap.begin`, which `systems/hazards.luau` already answers. Put it off the hero's way, or teach `tests/run.luau`'s playthrough to jump it (another `jump_at`), or that scenario's hero will be hurt on the way to the flag.
3. **A double jump or a dash.** Write a system that runs after `physics.control` (`after = { "physics.control" }`) and changes the hero's `KinematicBody.velocity` when jump is pressed in the air. ARGUS.md's section on `CharacterController` explains where such a system fits. Add a scenario for it in `tests/`.
