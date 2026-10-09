# The browser editor spike (phase 5 item 10)

The daemon now serves a small web client. A phone or a tablet on Kevin's tailnet opens a link, sees the review requests waiting with their clips, and answers one with a tap. It can also watch the game, follow the edit feed, and, with an edit token, move tuning sliders. The page talks the same protocol as every other client, over a WebSocket instead of the local socket, and a call takes as long either way.

The web build's sound also moved off the page's main thread: it plays in an AudioWorklet from a queue, so a slow frame no longer starves it.

This was a spike. It answers what the editor needs that only a native window has, and what a browser uses instead. It does not replace the desktop editor.

## Try it

Start the daemon with the web client on this PC's tailnet address, then make a link for the phone:

```
argus daemon start --web tailnet
argus token issue scope=review hours=4 label=phone
```

The second command prints a `url` like `http://100.93.53.97:7878/#t=argus-...`. Open it on any device signed in to the tailnet. The page takes the token from the link and clears it from the address bar. `--web` on its own (or `--web local`) listens on this machine only, and `--web :PORT` picks another port.

`argus token list` shows the tokens and when they expire; `argus token revoke id=...` ends one at once. A token lasts 12 hours unless `hours=` says otherwise, at most 30 days.

The check, in headless Chrome as a 390 x 844 touch phone:

```
node tools/web/editor-check.mjs
```

## The answers

| Question | Answer |
|---|---|
| §19: is a small web client worth having before the browser editor? | Yes, and it is built. Answering a review from the couch needs four things: the list, the clip, the choices, and a way to say who answered. The page has all four, and the daemon records the answer as `web:<label>`. It took no new protocol methods beyond tokens. |
| What does the editor need that only a native window has? | Four things: the local socket, the shared-memory frames, files read straight from disk, and starting the daemon itself. The table below gives each one and what the browser uses instead. |
| Is the browser slower? | Not for calls. 200 calls of `session.status` took 1.88 ms each over the WebSocket and 1.77 ms over the local socket (p95 2.10 and 1.95 ms). Frames are the difference: the page asks for a PNG each time, which runs at 88 frames a second at 480 x 270 on this machine, against the shared-memory stream's full rate at full size. |
| Who may connect? | Only someone holding a token, and only from this machine or the tailnet. See "Trust" below. |
| Can the desktop editor run in a browser later? | Most of it. The editor is egui on eframe, and eframe's web runner compiles for wasm with the editor's own features (checked offline, no new crates). What stops the editor compiling today is its client: `interprocess` has no wasm target, and memory-mapped frames have no browser equivalent. |

## What only a native window has, and the browser's way

| Native editor | Browser client |
|---|---|
| The local socket (a named pipe on Windows), which only this user can open. | A WebSocket at `/ws` on the daemon's web listener, carrying the same JSON-RPC lines. The token goes in the subprotocol (`t.<token>`), since a browser cannot set headers on a WebSocket. |
| `render.stream`: frames written into a memory-mapped file the editor reads, with a notification per frame. | `obs.frame` writes a PNG under `.argus/frames/`, and the page fetches it from `/file/` with the token. One request per frame, measured at 88 a second at 480 x 270. A faster stream would send frames as binary WebSocket messages, or as video; neither was needed for review. |
| Files the editor reads itself: a review's clip, a tileset's image. | `/file/` serves files under `.argus/reviews`, `.argus/marks` and `.argus/frames` only (PNG, JSON, WAV, text), with the token. A path with `..` or outside those folders is refused. |
| The editor starts the daemon (`argus daemon start`). | The daemon serves the page, so it is already running. A browser cannot start it. |
| `render.window` and `input.device`: a native window and real devices. | Refused to web tokens. The page has no game input yet; it would send `input.inject` the way the web player's page does. |

## Trust

§5 says remote access goes through `argus token`, with a scope and an expiry, over TLS. The spike keeps the tokens and puts the tailnet where TLS would be (decided with Kevin, 2026-10-08).

- **Where it listens.** Only on this machine (127.0.0.1 or ::1) or its Tailscale address (100.64.0.0/10, fd7a:115c:a1e0::/48). Tailscale encrypts the link between devices, so the daemon needs no certificate. Any other address, a home network's 192.168 one included, is refused when the daemon starts, with the reason.
- **Tokens.** `argus token issue` makes a random secret (`argus-` and 40 hex characters from the operating system). The project keeps only its SHA-1 in `.argus/tokens.json`, with the scope, the label and the expiry. A token is checked on every file request, and on an open WebSocket once a second, so a revoked or expired token is cut off mid-session within a second: the page is told why, then the connection closes.
- **Scopes.** *observe* reads: status, docs, frames, the feed, reviews. *review* also answers reviews and drops and notes marks. *edit* also plays, pauses, tunes and edits. No token can open or close a session, shut the daemon down, open a window, stream shared memory, or issue tokens.
- **The browser's own attacks.** The page is served with a content security policy, no caching and `nosniff`. A `Host` header that is not the listener's own address is refused (against DNS rebinding), and so is a WebSocket from another origin.
- **TLS** for anywhere beyond the tailnet is not built. It would need a certificate the phone trusts, which Tailscale already gives.

## The sound in an AudioWorklet

The web player used cpal's Web Audio output, whose buffers the page's main thread schedules about 85 ms ahead. Now the page opens an AudioWorklet: the browser's audio thread plays from a queue, and the page keeps the queue 100 ms deep by pulling blocks from the mixer every 10 ms (`?audio_ms=` sets another depth). The mixer runs at the AudioContext's rate, rebuilt as it is for a device that wants another rate. The worklet counts every 128-frame block it could not fill once it has started.

Measured in headless Chrome at 48 kHz, during the web check:

| Case | Blocks short |
|---|---|
| The page running for 2 s | 0 |
| Three 60 ms stalls of the page's thread | 0 |
| Three 150 ms stalls | 75 (about 67 ms of silence each, the stall less the queue) |

A stall shorter than the queue is now heard as nothing; a longer one is a gap the length of the difference. CI's web check fails if the sound opens any other way, or runs short while the page runs.

An AudioWorklet needs a secure context, which a page from `localhost` or over HTTPS is, but a page over plain http from another machine is not. There the page falls back to cpal's output, and `?cpal` asks for it.

## What was found

- **A daemon that could not start kept the CLI waiting 30 seconds.** `argus daemon start --web 192.168.1.50` waited for a daemon that had already refused the address. The address is now checked before the daemon starts, and `daemon start` notices a daemon that exits early and quotes its log's last line. Two projects' daemons both asking for port 7878 say so in one line.
- **The page's header showed the tick from when it connected.** It now follows each watched frame and the status every three seconds.
- **A revoked token's open connection kept working.** Revoking stopped new connections, but a page already connected kept calling and kept receiving notifications. Each connection now re-checks its token once a second.

## Recommendation

- **Keep the desktop editor as the editor.** It has everything (the hierarchy, inspector, tuning, agent panel, scrubber, profiler, tile painting, animator and timeline editors) at full frame rate, and nothing here argues for rewriting it in HTML.
- **Keep the web client small: review, watch, tune, feed.** It covers the couch playtest and a quick look from another room.
- **If a full browser editor is wanted later, compile the egui editor to wasm rather than rewrite it.** The work is in `argus-client`: a transport behind a trait (the local socket natively, a WebSocket in the browser), frames as binary WebSocket messages instead of the mapped file, and the editor's two direct file reads through `/file/`. Every panel is already a protocol client, so the panels would not change.

## Left for later

- Game input from the page (`input.inject`), for playing from a tablet.
- Frames faster than one PNG per request, if a remote viewport is wanted at full size.
- TLS beyond the tailnet.
- A hand check: Kevin opening the link on his phone over the tailnet, which no headless check can do.
