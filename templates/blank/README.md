# blank

The smallest start for Argus: a player who walks over a floor, and one scenario that checks the walking. Walk with WASD, the arrow keys, or a gamepad's left stick.

## The files

- `scenes/main/` is the one scene. `player.toml` is the player: its sprite, its animator, and the camera it carries, with the dark that shows past the floor (its `Background`). `floor.toml` is the floor.
- `components/game.luau` declares `Player`, with its walking speed. `systems/move.luau` walks the player by the move action.
- `systems/look.luau` turns the player to face the way they walk. It only changes how the player is drawn.
- `tests/move.luau` is the scenario: hold right for a second, and the player must have walked six units. Run it with `argus test`.
- `input.toml` lists the actions and the keys and buttons for each.

## The art

Two pictures, both drawn by `tools/art.py`, a short standard-library Python script. Run `python tools/art.py` and the game picks up the new files, even while it runs. A release build leaves `tools/` out.

- `assets/player.png` is five 18 x 18 frames in a row, facing right. Its sidecar `assets/player.toml` cuts it into frames and names two clips: `idle` (frame 0) and `walk` (frames 1 to 4). `assets/anim/player.toml` is the animator that picks between them: `walk` while the move action is held, `idle` when it is let go.
- `assets/floor.png` is the floor's 30 x 30 units: flagstones inside a kerb, with moss in the joints.

The art is 9 pixels to a world unit, and the camera shows 15 units top to bottom, so on a 960 x 540 screen each pixel is drawn 4 x 4. To recolour it, change the palette at the top of `tools/art.py` and run it again. To use your own art, put your own `player.png` and `floor.png` in place of these (keep the frames in a row, or change `grid` and the clips in `player.toml`); nothing else needs to change.

## Ideas for what to add next

1. **Something to find.** A new entity file with a `Sprite` (a coin, a key), and a system that notices when the player is near it with `q:nearest`. Add a scenario that walks there.
2. **Keep to the floor.** In `systems/move.luau`, hold the player inside the floor's 30 x 30 units, and add a scenario that walks into the edge.
3. **Walk up and down.** Draw a row of frames walking up and one walking down under the first, give each a clip in `assets/player.toml` (`grid = [5, 3]`), and add animator states that play them by `Actions.move.y`.
