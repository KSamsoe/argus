# The playtest server

A web build of a game, hosted on klab so someone else can play it, and the reports they send with F8 collected for the agent (phase 2 item 7, and the phase 2 gate).

## For the player

Open the link, click the game, and play. When something feels wrong, press F8: the game pauses and asks what happened. Type a line and press Enter (or click Send report). The last half minute of play goes to the developer with your note. Escape goes back to the game.

The link is `https://klab.tailde2255.ts.net:8443/`, reachable by people on Kevin's tailnet. It needs HTTPS certificates turned on for the tailnet (Tailscale admin console, DNS, Enable HTTPS) and this, run once on klab:

```bash
sudo tailscale serve --bg --https=8443 http://127.0.0.1:8765
```

Over HTTPS the page draws with WebGPU; a browser without it falls back to WebGL2 and plays the same game.

## What runs on klab

- `~/argus-play/site/`: the page, as `argus build --target web --release` wrote it.
- `~/argus-play/marks/`: the reports players sent, one `.argr` file each.
- `~/argus-play/serve.mjs`: the server (`tools/web/serve.mjs`). It serves the page and takes reports at `POST /marks`, at most 8 MB each.
- `argus-play.service`: a user service that runs the server with Node. It listens on `127.0.0.1:8765`, where Tailscale's HTTPS proxy reaches it, and on klab's tailnet address `100.70.114.55:8765`, where `argus mark pull` reaches it. Nothing listens on the home network.

Service commands, as `kevin` on klab:

```bash
systemctl --user status argus-play
```

```bash
journalctl --user -u argus-play -n 50
```

## Reports into marks

From a machine on the tailnet:

```bash
argus mark pull url=http://100.70.114.55:8765/
```

From a machine that reaches klab only by SSH (this PC), open a tunnel first and pull through it:

```bash
ssh -N -L 18765:127.0.0.1:8765 kevin@192.168.1.176
```

```bash
argus mark pull url=http://127.0.0.1:18765/
```

Each report becomes a mark in the project's `.argus/marks/`, once: the project remembers which it has pulled from that server. `argus mark open id=<n>` reproduces it. A report replays in a build of the same code; the mark says which code made it.

## Putting up a new build

```bash
argus build --target web --release --out target-web/site
```

```bash
tools/web/deploy.sh target-web/site
```

The page is swapped in with one rename; the server needs no restart, and reports already sent stay. If `tools/web/serve.mjs` itself changed, copy it to `~/argus-play/serve.mjs` and restart the service.

## Linux builds

klab builds the Linux release: the engine's source is in `~/argus` (updated from a git bundle, since klab has no access to GitHub), Rust is installed for `kevin`, and the build needs `libasound2-dev` and `libudev-dev`. In `~/argus`:

```bash
cargo run -q -p argus-cli -- --project examples/hollow build --release
```

The release lands in `examples/hollow/dist/linux/`. It needs glibc 2.39 or newer to run (Ubuntu 24.04 and later), because klab has that; a build for older systems would be made on an older one.
