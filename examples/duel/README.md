# Duel

Two fighters in one arena, first to three points. Each can move, dash, and swing. A swing that touches the other fighter is a point: the one hit is knocked back and flashes, the round stands still for a second, and both go back to where they started. Nothing hits a fighter while it dashes, so a dash at the right moment goes straight through a swing. The swing and the dash each have a cooldown. It plays on one machine, two players sharing the keyboard or with two pads, or online, one player on each machine, through a relay.

It is the multiplayer example: a real-time game whose online matches show prediction and rollback at work.

## Playing

The title menu has three buttons:

- **Two players here**: a match on this machine.
- **Host online**: a room for two on the relay. The menu shows the room's code; the match starts when the other player joins.
- **Join online**: type the other player's code in the box, then Enter.

The game needs a relay to play online: `argus relay` runs one, and the desktop game takes `--relay ws://<address>:7879`, the web page `?relay=ws://<address>:7879`.

| | Player 1 | Player 2 |
|---|---|---|
| Keyboard | WASD to move, F to swing, Space (or G) to dash | the arrow keys, Period to swing, Slash to dash |
| Gamepad | the first pad: stick or d-pad, A (or RB) to swing, B (or LB) to dash | the second pad, the same |
| Touch | the stick at the bottom left, Attack and Dash at the bottom right | |

Online, each player plays on their own machine with player 1's keys, pad, and touch. On the win screen either player's swing starts a new match. Escape (or Start) goes back to the menu from a match on this machine.

## The files that matter

- `components/duel.luau`: the game's data. `Fighter` (speeds, the dash and the swing, their cooldowns, the knockback), `Swing` (a swing's hitbox), `Slash` (how a swing is drawn), and `Match` (the score, and where the match is: menu, fight, point, won). The fields marked `tune` are in the editor's tuning panel.
- `systems/`: the rules, in the order they run each tick:
  - `menu.luau` shows the title menu and the win screen, follows the room online (`q:net()`), and does what the menu's buttons ask.
  - `round.luau` moves the match on: a new round after a point, the win at three, a new match.
  - `move.luau` walks and dashes the fighters, slows a knockback, and keeps both inside the walls.
  - `attack.luau` swings, and scores a swing that lands.
  - `looks.luau` runs after them and only changes how things look and sound.
- `prefabs/swing.toml`: the hitbox `attack.luau` spawns. `prefabs/slash.toml`: its slash, which `looks.luau` spawns and which outlives it.
- `scenes/arena/`: one file per thing in the scene. `one.toml` and `two.toml` are the fighters; `arena.toml` the floor and the match; `menu.toml`, `win.toml`, and `hud.toml` the screens; `camera.toml`, `wall.toml`, and `braziers.toml` the room.
- `input.toml`: the actions and their keys, buttons, and touch controls; `[slots.2.keys]` gives player 2 its own keys on the shared keyboard.
- `audio.toml`: the sounds, each a described tone. `ui.toml`: the colours.
- `assets/`: the art, drawn by scripts. `sprites/actors.art.luau` draws both fighters (in the colours in `shared.lib.luau`), the slash, and the effects' specks; `sprites/room.art.luau` the floor and walls; `ui/panel.art.luau` the frame the screens sit in; `fx/` the effects.
- `tests/`: the scenarios. `rules.luau` (a point, a dash through a swing, a clash, the cooldowns, the walls, the shared keyboard), `match.luau` (first to three and the win screen), `menu.luau` (the buttons), `touch.luau`, `online.luau` (a whole match over a bad link, `t:net`, and the game's own input delay), `slash.luau` (a slash outlives its hitbox), and `looks.luau` (pictures of the menu, a fight, and the win screen). Run them with `argus test`.

## Online, and what the world may hold

In a match every player's machine runs the same world from the same input, so nothing in the world may depend on which machine it is on. `q:net()` says only that a match is playing, never which player this machine is. Keep every bit of state in components, read time from `q`, and draw random numbers from `q:rng()`.

Online, the other player's actions arrive late, and the game corrects its guesses by going back a few ticks and running them again. Two things make that kinder:

- **Input delay.** `argus.toml`'s `[net] delay = 3`: every player's press takes effect 3 ticks (50 ms) later, on every machine, so more of the other player's presses arrive in time. More hides a slower link, less answers sooner.
- **The slash outlives the hitbox.** A swing's hitbox is out for only 8 ticks, and on a slow link the other player's swing can be learned of when most of them are over. So the slash is drawn apart from the hitbox: it sweeps while the hitbox is out, then stays where it was, fading, for 16 ticks more. It is the same on every machine, as everything in the world must be; a slash drawn from when this machine learned of it would differ between the two, and the match would stop, out of step.

The menu waits ten ticks before it shows, and before it believes `q:net()`: a machine whose match has just begun may still read `alone` for its first frame, while the other reads `playing`, and the two must do the same.
