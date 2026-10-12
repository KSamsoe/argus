# blank

The smallest start for Argus: a player who walks over a floor, and one scenario that checks the walking. Walk with WASD, the arrow keys, or a gamepad's left stick.

## The files

- `scenes/main/` is the one scene. `player.toml` is the player: its sprite, its animator, and the camera it carries, with the dark that shows past the floor (its `Background`). `floor.toml` is the floor.
- `components/game.luau` declares `Player`, with its walking speed. `systems/move.luau` walks the player by the move action.
- `tests/move.luau` is the scenario: hold right for a second, and the player must have walked six units. Run it with `argus test`.
- `input.toml` lists the actions and the keys and buttons for each.

## The art

Two art scripts draw it: Luau files the engine runs to make each picture, so changing the art needs nothing but Argus. Change one and save it, and the game shows the change while it runs. A release build carries the pictures, not the scripts.

- `assets/player.art.luau` draws the player as rows of text, one character a pixel: five 18 x 18 frames facing right, and two clips, `idle` (frame 0) and `walk` (frames 1 to 4). `assets/anim/player.toml` is the animator that picks between them (`walk` while the move action is held, `idle` when it is let go) and mirrors the player while they walk left.
- `assets/floor.art.luau` draws the floor's 30 x 30 units: flagstones inside a kerb, with moss in the joints. Its `SEED` picks another floor.

The art is 9 pixels to a world unit, and the camera shows 15 units top to bottom, so on a 960 x 540 screen each pixel is drawn 4 x 4. To recolour it, change a colour at the top of a script. To use art from a paint program, put `player.png` (with a `player.toml` cutting it into frames and naming the same clips) or `floor.png` in place of a script; nothing else needs to change. `argus docs presentation section=Art scripts` says what a script can draw.

## Ideas for what to add next

1. **Something to find.** A new entity file with a `Sprite` (a coin, a key), and a system that notices when the player is near it with `q:nearest`. Add a scenario that walks there.
2. **Keep to the floor.** In `systems/move.luau`, hold the player inside the floor's 30 x 30 units, and add a scenario that walks into the edge.
3. **Walk up and down.** Draw frames walking up and walking down in `assets/player.art.luau`, give each set a clip, and add animator states that play them by `Actions.move.y`.
