# Multiplayer

Two to four players, each at their own machine, can play one game together: on Windows, Linux, a Mac, or in a browser, in any mix. This chapter covers how a match works, what a game needs to be playable online, hosting and joining from the game's own menu, the relay the players connect through, and testing a match.

Argus runs the same game on every player's machine and sends only what each player does, never the world. That works because a run is the same, bit for bit, on every machine that is given the same input ([Testing](testing.md#determinism-why-the-same-seed-gives-the-same-run)).

## How a match works

Each player owns a **slot**, the same slots `input.toml` declares for players at one machine (`[slots.1]`, `[slots.2]`). Every tick, each machine sends its own player's actions to the others. Every machine runs the same ticks with every player's actions, so every machine computes the same world.

A player's actions take time to arrive. The game does not wait for them:

- **Prediction.** A tick runs at once with what is known. A player whose actions for it have not arrived is guessed to be doing what they were doing (their actions unchanged).
- **Rollback.** When their actions arrive and the guess was wrong, the game goes back to the tick that was wrong, from a snapshot it kept, and runs the ticks since again with what is now known, quietly, before the frame is drawn. It keeps the last 8 ticks, and waits rather than guess further ahead than that.
- **Input delay.** Each player's own actions take effect 2 ticks after they press (the host chooses the delay). On a link quicker than that, nothing is ever guessed.

A **turn-based game** (a `discrete` clock) is played in lockstep instead. A turn runs when every player's command for it is in. A player who has nothing to do answers at once with nothing, so one player's press moves the game for everyone, as it does alone. Letting go of a key takes no turn of its own.

**Out of step.** Every second, the players compare a digest of their worlds. If two ever differ, the match stops, and each player is told the tick and the fields that differ, with both values:

```
out of step at the end of tick 59 with slot 1
e:1 Fighter.speed: 9.0 (4022000000000000) here, 6.0 (4018000000000000) in slot 1's
```

A game whose scripts follow the rules below never gets there. When one does, it is a bug, in the game or in Argus, and the report says where to look.

A match is one run: every player's actions are in every player's input log. Either player's bug report replays the whole match on its own, and so does either player's input log.

## Making a game multiplayer

A game that plays well with two players at one machine is most of the way there.

**Slots.** Give each player's entity its slot, as for players at one machine (`[PlayerSlot] slot = 2`, [Gameplay](gameplay.md#slots-and-players)). Online, each player plays with their own machine's devices: everyone's keys are slot 1's keys, whichever slot the room gave them, so give slot 1 the keys and the pad a player at their own machine expects.

**The world is every player's.** Everything in the world is the same on every machine, the UI included. So nothing in it may depend on which machine it is:

- During a match, `q:net()` says only that a match is playing and how many players are in it. It never says which player this machine is. A game that wants "you are player 2" shows it outside the world (a page script, a window title), not in it.
- The player's settings (text size, contrast) are in the world, so a match plays with the game's own defaults.
- Keep every bit of state in components, read time from `q`, and draw random numbers from `q:rng()`, as always ([Scripting](scripting.md#the-sandbox)). A file-level local that changes is the most likely way to fall out of step: a rollback does not put it back.

**What a tick does outside the world.** In a match a tick may run more than once. Argus handles what reaches past the world:

- A **sound** starts at once, because a late sound feels wrong. After a rollback, a sound the corrected ticks no longer make is stopped, one they make again keeps playing, and one only they make plays then.
- **Particles** are part of the run, so they roll back with it.
- **Everything else** a tick asks of the host (`q:send`, a save to load or delete, an achievement) waits until the tick is final: every player's input for it is in, and it ran with it.

**Math.** Luau's `math`, `^`, and `vector` give the same numbers on every machine. A Rust plugin must use the SDK's `argus_plugin::math`, not the float's own functions ([Releasing](releasing.md#rust-plugins)).

**Two players on one keyboard.** A slot can have keys of its own, so two players share one keyboard at one machine (`[slots.2.keys]`, [Gameplay](gameplay.md#slots-and-players)). It is how a match is played in tests and by an agent, and a good way to try a game before playing it online.

## Hosting and joining

The game's own menu hosts and joins. Its scripts ask, and whatever runs the game (the desktop game, the web page, the daemon) does it:

| Call | What it asks |
|---|---|
| `q:net_host(players, scene)` | Host a room for `players` (2 to 4) on the relay. The match will start on `scene` (the current one when left out). |
| `q:net_join(code)` | Join the room with this code. |
| `q:net_leave()` | Leave the room or the match. |
| `q:net()` | Where it is: `{state, code, slot, players, present, why}`. |

`q:net().state` is one of:

- `alone`: no room.
- `asking`: waiting for the relay to answer.
- `room`: in a room. `code` is what the others join with, `slot` this player's slot, and `present` the slots whose players are there.
- `playing`: in a match. Only `players` is given (see above).
- `ended`: the match or the room is over, and `why` says why (a player left, the worlds fell out of step, the relay closed the connection).

A menu that shows the room's code, from a system that runs every tick:

```lua
local n = q:net()
local line = q:entity("@menu/status"):get("UiText")
if n.state == "room" then
  line.text = "Room " .. n.code .. ": waiting for the other player"
elseif n.state == "asking" then
  line.text = "Asking the relay"
elseif n.state == "ended" then
  line.text = n.why
end
```

A text box (`UiTextBox`, [Presentation](presentation.md#a-text-box)) takes the code to join with.

**The match starts on its own** once every slot is taken. Every player loads the host's scene afresh, from a seed the host chose, so the match begins on the same world everywhere. On that start `q:net().state` is already `playing`, so a menu checks it and stays hidden.

What `q:net()` read on a tick is recorded with the run's input, as `q:saves()` is, so a replay of a lobby or a match reads what the tick read.

**From outside the game.** A game without a menu for it can be hosted and joined at its start:

- The desktop game: `--relay ws://<address>:7879`, then `--host` (or `--host 3` for three players), or `--join <code>`. The room's code shows on the bottom line.
- The web page: `?relay=ws://<address>:7879`, then `&host=2` or `&room=<code>`. So a page's link can carry the room: `index.html?relay=ws://100.70.114.55:7879&room=KQ7XM`.
- The daemon: `argus net host relay=ws://... players=2` answers with the code, `argus net join relay=ws://... code=KQ7XM` joins, `argus net status` says how it goes, and `argus net leave` leaves. While it plays, the daemon's frames run the match, and `input.inject` is its side's input, so an agent can play one side against a person and look at the world, which is both players'.

## The relay

Every player connects out to a relay, so a match works behind home routers, and a web page and a desktop game can play together. A player hosts a room and gets a five-letter code; the others join with the code; the relay passes each player's messages to the rest of the room. It reads nothing past its own few bytes and keeps nothing.

```sh
argus relay              # this machine only, port 7879
argus relay tailnet      # this machine's tailnet address
argus relay :8000        # this machine, another port
argus relay tailnet lag=80 jitter=40   # a slow link, on purpose
```

`lag` holds each message passed on that many milliseconds, and `jitter` up to that many more: how a match is tried over a bad connection when the real one is good.

`argus help relay` has the rest: the limits (rooms, players a room, message size and rate) and what each address means. The relay keeps a dropped player's slot for a minute, for a client to come back to with its seat token (`argus_relay::Client::rejoin`).

A relay for anyone on the internet is `argus relay public`. [Releasing](releasing.md#running-a-relay) says what that exposes; read it first.

## Testing a match

A scenario plays a whole match itself. `t:net` at the start of a run makes the other players' worlds, as copies of this one, and sends every message through a network as bad as asked:

```lua
return scenario "two players over a bad link play whole rounds" {
  scene = "arena", seed = 1,
  run = function(t)
    t:net { players = 2, delay = 2, latency = 60, jitter = 30, loss = 0.05 }
    local two = t.input:slot(2)
    t.input:hold("move", { 1, 0 }, 40)
    two:hold("move", { -1, 0 }, 40)
    t:step(45)
    two:tap("attack")
    t:step(90)
    t.expect:field("@score", "Score.two"):eq(1)
  end,
}
```

- `latency` is the time a message takes one way, in milliseconds; `jitter` up to that much more for each message; `loss` the chance a message is lost; `delay` the input delay in ticks.
- `t.input` is slot 1's input and `t.input:slot(n)` slot n's, each through the match.
- Every `t:step(n)` runs every player's world n ticks and ends with every tick final and every world the same. When two differ, the run fails, naming the fields. Only steps by ticks work in a match.
- The expectations read slot 1's world, which is every player's.
- `t:net_status()` gives the counts: each player's rollbacks and ticks run again, the checks they agreed on, and the messages sent and lost.

The match ends with the run. The same works from the command line: `argus net sim players=2 latency=60`, then `argus sim step 600`.

## Limits today

- Text from the host (`input.message`, a page's `player.message`) and typed text in a text box do not reach a match yet; a menu that needs them is used before the match starts.
- A turn-based game's idle ticks between turns (2D staples item 5) do not run in a match.
- A save made during a match keeps the tick as it first ran, even if a rollback later changed it.
- The UI is in the shared world, so a menu one player opens, every player sees.
- A player whose connection drops ends the match for everyone: the relay would let them back into their slot, but a match does not take them back yet.

## Where to go next

- [Testing](testing.md): scenarios and goldens.
- [Releasing](releasing.md): building for the desktop and the web, and running a relay.
- The reference (`argus docs`): `net.host`, `net.join`, `net.sim`, `net.status` among the methods; `q:net` and `t:net` in the scripting and scenario API references.
