# The host channel: checklist

Kevin, 2026-10-12: the first real game made with Argus, Bring Your Own Game Master (`Documents/argus-games/byogm`), needs text to cross between the game and whatever hosts it. Its AI game master is a model the page calls: the game sends out a summary of the scene, and the model's ruling (a few KB of JSON) comes back in. The ruling has to enter the sim as recorded input, so a seek, a replay, a mark, a report, or a scenario reproduces the turn without calling the model again.

Argus has no way to do that today. Input is buttons, axes, and pointers; nothing carries text into the sim, and nothing lets the page see what the game emits. The game's design doc planned a game-specific workaround (bytes packed into pointer actions, the save-load request abused as an outbox, `main.js` patched after every build); Kevin chose a small general feature instead.

Three pieces, each general: any game can use them, and a game that doesn't costs nothing.

## 0. Messages in

- A host sends a **message**: a channel name and a text. It is an input command like an inject: journaled, so seeks replay it and a live one branches the run, carried by keyframes, replay files, marks, and reports, and due at the next tick.
- A script reads the tick's messages as `host.message` events (`channel`, `text`, `slot`), from `q:events("host.message")`, in any system after `input.actions`.
- On a discrete clock a message is a turn, as a key press is; a game that shouldn't spend one freezes it, as a menu does.
- Every host can send one: the daemon (`input.message {channel, text, slot}`, `argus input message`), a scenario (`t.input:message(channel, text)`), and a release build's page (`player.message(channel, text)`).
- Limits, so a mistake fails loudly: a channel is a short name (letters, digits, `.`, `_`, `-`, up to 64), a text up to 64 KiB.

**Built (2026-10-12).** `Command::Message` in argus-input's journal; a message is scheduled for the tick it was sent before and taken into that tick's inbox when the tick begins, so seeks re-issue it from the journal and a live one branches the run, as an inject does. A keyframe carries the messages not yet due (a message sent just before a keyframe would otherwise be lost on a seek back to it), so replay files go to version 7; version 6 and older still read. `input.actions` hands the inbox to the scripts as `host.message` events. `input.message` on the daemon (in the change feed), `Player::message` (a turn on a discrete clock), and `t.input:message` in scenarios.

## 1. Messages out

- A script sends text to the host with `q:send(channel, text)`. It is an event, `host.send` (`channel`, `text`), so everything that sees events sees it: `events`, `obs.subscribe`, a mark's events, a scenario's `t.expect:events`.
- A release build keeps what was sent since the host last asked: `player.take_sent()` on the page, as JSON `[{channel, text, tick}]`.

**Built (2026-10-12).** `q:send` in argus-luau emits `host.send`; the release player keeps an outbox of them from its event pump, which `WebPlayer.take_sent()` hands the page as JSON.

## 2. A game's own page script

- A project's `web/` folder is copied into a web build beside the page, and the page loads `web/game.js` when there is one, after the player is made, with `window.argus` set. That is where a game's bridge to the outside lives (here, the call to the model), so a web build no longer needs patching.

**Built (2026-10-12).** `argus build --target web` copies `web/` beside the page and names `web/game.js` in `data/packs.json`; `main.js` imports it after it sets `window.argus`.

## 3. Docs

- The manual: messages in and out in the gameplay chapter, next to input and events; the page script in releasing; what a mark carries in playtesting.
- The reference: `input.message`, `q:send`, `t.input:message`, `player.message` and `player.take_sent` in their tables, generated as the rest is.

**Done (2026-10-12).** The manual's gameplay chapter has "Talking to the host"; releasing has the page script with an example; scripting, testing, and playtesting say where messages show up. The reference's entries come from the method, Luau, and scenario tables, so `argus docs` lists `input.message`, `q:send`, and `t.input:message`.

## The gate

- A scenario sends a message, a script answers with `q:send`, and the answer's event is checked.
- A run with messages in it, seeked back and stepped on, reaches the same world digest; a replay file written with messages in it reads back and reproduces its end frame.
- A web build of a project with `web/game.js` runs that script, and a message sent from the page reaches a script, in headless Chrome.
- Every earlier gate's tests still pass; a full CI passes.

**Where it stands (2026-10-12).** The first three are met, each by a test: `argus-test/tests/runner.rs` (a scenario sends a message and checks the script's answer), `argus-input/tests/input.rs` (five tests: a message at its tick, a seek bringing it in again with one pending at the keyframe, a keyframe and journal on disk, refusals, the event), `argus-player/tests/host.rs` (a report with twelve messages replays in a fresh player to the same digest), `argus-daemon/tests/marks.rs` (a seek back before a message and on again, and a mark opened in a daemon that never saw the messages, with the same digest), and `tools/web/host-check.mjs` (a web build of the roguelike with a `web/game.js`: the page's message reaches a script and the script's answer comes back, in headless Chrome; a CI step now).

**Gate met (2026-10-12).** A full CI on the branch passed every step in 59 minutes: fmt, clippy, test, games, perf, and web, the new host check among them.
