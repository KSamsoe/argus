# Multiplayer: checklist

Kevin, 2026-10-21: after 0.1.2, the next work is online multiplayer, chosen over a polish batch, agents in the browser, and another staples comparison. Players reach each other through a relay server, and a new small versus game shows it off, on the web and the desktop.

The design kept this open on purpose (§15): "Determinism plus an action log is most of a lockstep model; snapshots plus resimulation is most of rollback." Inputs already arrive only as per-slot actions in the input log, and a run already replays from its journal. What is missing is listed in [docs/determinism-spike.md](determinism-spike.md) and below.

**The version.** Item 0 changes some of the engine's arithmetic on Windows, so a replay or a golden recorded on 0.1.x would not end the same. That breaks the 0.1.x promise, so this work ships as 0.2.0, with the change listed under "Breaking changes" in its notes and every example's goldens recorded again.

In order:

## 0. The same run on every machine

Two players' machines must compute the same world, bit for bit, every tick, or the match drifts apart.

- **One math library for the sim.** Every `sin`, `cos`, `tan`, `atan2`, `exp`, `ln`, `powf`, and the like that can reach the world (the transform hierarchy's rotations, the camera's follow and shake, tweens' easing, physics, navigation) goes through `libm`, as the skeletons, art scripts, and sound generation already do, and so does the plugin SDK. A test reads the sim crates' sources and fails on a platform math call outside an allowed list of presentation-only code (drawing, light, particles, post).
- **The same for Luau.** A script's numbers come from the same library: Luau's `math` functions and its `^` operator use `libm`'s, built into Luau's own C++, since replacing them from the host is ignored by Luau's fast path (measured in the spike). Luau's C++ is built with `-ffp-contract=off`, so clang never fuses a multiply and an add on an ARM Mac.
- **A cheap desync check.** A digest of the world's columns as bytes, fast enough to compare every second during a match. The existing digest and its rows stay, for finding what differs.
- **Checked in CI.** The determinism spike's tools run on this PC, klab, and headless Chrome for every example, and a difference fails the step.
- Tests: the source scan; Luau's `math.sin` and `^` against `libm`'s bits; the fast digest changing when any field does and costing well under a millisecond for Swarm; every example's run ending on the same digest on all three platforms.

**Done (2026-10-21, ae0ebd7a and d9010978).** The sim, the plugin SDK (`argus_plugin::math`), and Luau (vendored `luau0-src` and `mlua-sys`, vendor/README.md) all compute with `libm`, and `crates/argus-sim/tests/platform_math.rs` fails on a platform call in sim code. `World::column_digest` takes 28 µs for a Swarm-sized world, against 1.7 ms for the text digest. CI's det step (`tools/determinism/ci.sh`) gave the same run on this PC, klab, and Chrome for every example's long runs, puppet's included, and the same world on klab for all 348 scenario runs.

## 1. A match: prediction and rollback

- **A match** is two to four players, each owning a slot (`[slots.<n>]` as today). Every tick, a player's own actions for that tick go to the others.
- **Prediction.** A player who has not heard another's actions for a tick plays on with that player's last ones. When the real ones arrive and differ, the game goes back to the first tick that was wrong, from a snapshot it kept, and runs the ticks again with what is now known, quietly, before the frame is drawn. A window of up to 8 ticks back is kept.
- **Input delay.** A match may delay every player's input by a tick or a few, so fewer corrections are needed on a slow link. The game chooses it, and it can be tuned.
- **What rolls back.** The sim stage only. Sound and particles are presentation: a sound a corrected tick no longer makes stops, and one it still makes is not played twice.
- **Turn-based games** need no prediction: a turn runs when every player's command for it has arrived (lockstep).
- **The run is one run.** Every player's actions are in the input journal, so a match replays alone from any player's recording, and a report from either player reproduces it.
- **Out of step.** Players compare the fast digest every second. When it differs, the match stops and says so, and each player's report names the first tick and the fields that differ.
- Tests: two players in one process over a simulated link (delay, jitter, loss) end every tick on the same world; a forced misprediction rolls back and agrees; a desync is caught within a second, and its report names the field; the cost of a rollback of 8 ticks is measured against a frame.

**Done (2026-10-21).** The new crate `argus-net` is the match: its wire messages, a `Link` (and `SimNet`, a bad network inside one process), and `Match`, which runs any `Peer`. `Player` is one (`crates/argus-player/src/netplay.rs`): `join_match`, then `net_devices` for the local player's input. A rollback restores a keyframe and runs the ticks again quietly, the input journal and log rewritten from there, so either player's input log, replayed alone, and either player's report end on the same world. Sounds start at once; after a rollback a sound no longer made is stopped and one made again is left playing. Everything else a tick asks of the host waits until the tick is final. Turn-based games run in lockstep, and a release rides along with the next command rather than taking a turn. `OutOfStep::describe` gives the tick and each differing field's two values, for a report's note. Measured: a rollback of 8 ticks of Hollow is 2.1 ms, 13% of a 60 Hz frame (`crates/argus-player/tests/netplay.rs`).

Left for item 3, as found: during a match every input must come through it (a host message or typed text does not yet, and a rollback would drop one); idle ticks between turns are not run in lockstep; a `q:save` during a match keeps the tick as first run. And for item 4: the UI is in the shared world, so a menu one player opens is in both players' worlds.

## 2. The relay

- **`argus relay`**, in the install: a small server players connect to over WebSocket. A player hosts a room and gets its code; others join with it. The relay passes the players' messages on and keeps nothing.
- It works from a web page and a desktop build alike, behind home routers, since every player only connects out to the relay.
- It is safe to leave running: rooms and players have limits, messages have a size and a rate cap, and a player who sends what does not read is dropped.
- For testing it runs on klab over the tailnet. Letting the public reach one is Kevin's decision, taken with what it exposes written down.
- Tests: rooms, codes, joining a full room, a player dropping and coming back, the limits; a match through a real relay on this machine.

**Done (2026-10-21, a94ab2d8 and 20546481).** `argus relay [local | tailnet | public | ip:port]` (crate `argus-relay`, port 7879) serves rooms; `Client` is a desktop player's end, a `Link`. A dropped player comes back to its slot with its seat token; a room no one is in goes after a minute. It runs on klab as `argus-relay.service`, on the tailnet only (`tools/relay/deploy.sh`): `ws://100.70.114.55:7879`, a 6.6 ms round trip from this PC (`tools/relay/check.mjs`, Node's own WebSocket, as a page's).

**What letting the public reach a relay exposes** (Kevin's decision; `public` must be said):

- Anyone who reaches the port can open rooms, up to the limit (500), and send through them: a message goes to up to three others, so a relay can be made to send three times what it is sent, up to its limits (240 messages a second of up to 64 KB, 2,000 connections). Nothing caps the bandwidth of the machine as a whole.
- A room code is 5 of 31 letters, about 29 million codes. Nothing limits how often one address may try a code, so a stranger could find a room by trying, and take a free slot in a match not yet full.
- The traffic is not encrypted (`ws://`). A web page served over `https` may only reach `wss://`, so a public relay needs a TLS proxy in front (Tailscale Funnel, as the games site uses, or Caddy).
- The relay reads only its own few bytes of each message and keeps nothing; what the players send each other (their actions, and their worlds' digests and rows when they fall out of step) passes through unread.

Before a public relay: connections a minute per address, a cap on its total bandwidth, and longer codes or an approval by the host would close the first two.

## 3. Hosts and scripts

- **Scripts** see the match: `q:net()` gives the players, the local player's slot, each one's ping, and the match's state. Events say when a player joins, leaves, or falls out of step. A game's own menu hosts and joins, so it can show a room code and who is in.
- **The web page** joins a room from its address (`?room=CODE`), or from the game's menu. **The desktop game** joins from its menu, or from the command line.
- **The daemon** can run a match too, so an agent can play one side while a person plays the other, and look at either.
- **Scenarios** run a match in one test: `t:net { players = 2, delay = 2, jitter = 30, loss = 0.05 }`, every player's input scripted, and the test fails if the players ever disagree.
- Tests: a scenario match under a bad link; a page and a desktop game in one match through a relay; the daemon as one player.

**Done (2026-10-21, e402b88e, 9c6d6ddb, 650972c6).** Scripts: `q:net()` (recorded with the input, as `q:saves()` is; in a match only what every player shares), `q:net_host`, `q:net_join`, `q:net_leave`. `argus_net::Lobby` makes a room on the relay; once every slot is taken the host's `Start` has every player load the scene afresh from its seed, and the first world's digest is the match's fingerprint. The desktop game takes `--relay`, `--host [players]`, `--join <code>` (and plays a match headless, for tests); the page `?relay`, `?room`, `?host`; the daemon `net.host`, `net.join`, `net.leave`, `net.status`, playing one side so an agent plays against a person. Scenarios: `t:net {players, delay, latency, jitter, loss}` and `t.input:slot(n)` (`net.sim`: the other players are forks over a simulated network; every step ends with every world the same, or the run fails naming the fields). Tests: argus-daemon `tests/netplay.rs` (a scenario match over a 70 ms link with 5% loss; a drifted world failing its run by `Fighter.speed`; the daemon against a Player through a relay), argus-player `tests/netplay.rs` (a room on a relay starting one match), and `tools/net/check.mjs` (Duel's web build against its desktop build, and two pages, through a relay made slow on purpose: 9 checks agreed, rollbacks on both).

## 4. A versus game

- **A new example**, small and quick to read: two players in one arena, each with a move, a dash, and an attack, first to three wins. Real time, so it shows prediction and rollback at work.
- On the keyboard, a gamepad, and touch, on the web and the desktop, built with its own icon like the rest.
- Its art and sound are made by its own scripts, as the other examples' are.
- Two players on one machine, sharing the keyboard, also works, which is how it is played in tests and by an agent.
- Tests: scenarios of whole rounds over a bad link; its pictures; the web build in headless Chrome as two pages through a relay.

**Done (2026-10-21).** `examples/duel`: two fighters in the arena template's room, move, a dash that cannot be hit, a swing, first to three; a title menu (two players here, host online, join online with a code), the room's code from `q:net()`, a win screen; art from its own scripts, sounds as tones; the keyboard for both players (`[slots.2.keys]`), pads, and touch. 13 scenarios, among them five rounds over `t:net { latency = 60, jitter = 30, loss = 0.05 }` (13 rollbacks a side, 81 of 1,724 messages lost) and three picture goldens; its web and desktop builds play together (item 3). It is in CI's games step, the det step, and the release's example games. Not on the public games site yet: that is Kevin's to decide, with a relay the public can reach.

## Docs

- A **Multiplayer** chapter: making a game multiplayer (slots and who owns them, what may and may not be in the sim stage, presentation that copes with rollback), hosting and joining, the relay, and testing a match.
- The reference: `argus relay`, the net methods, `q:net()` and its events, and scenarios' `t:net`.
- Releasing: running a relay, and what letting the public reach one exposes.

## Gate

- The versus game played by two people on two machines, one in a browser and one on the desktop, through the relay on klab, over a link made slow on purpose, without falling out of step.
- An agent with only the docs adds a feature to the versus game and checks it with a two-player scenario.
- Every example still gives the same run on this PC, klab, and Chrome.
