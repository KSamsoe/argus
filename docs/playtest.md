# The playtest server

A web build of a game, hosted on klab so someone else can play it, and the reports they send with F8 collected for the agent (phase 2 item 7, and the phase 2 gate).

## For the player

Open the link, click the game, and play. When something feels wrong, press F8: the game pauses and asks what happened. Type a line and press Enter (or click Send report). The last half minute of play goes to the developer with your note. Escape goes back to the game.

The link is `https://klab.tailde2255.ts.net:8443/`, reachable by people on Kevin's tailnet. It rests on HTTPS certificates being on for the tailnet (Tailscale admin console, DNS, Enable HTTPS) and on this, run once on klab (done 2026-09-24):

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

## Reports from a desktop build

A desktop build of a project whose `argus.toml` names the server sends its players' reports here too:

```toml
[reports]
url = "http://100.70.114.55:8765/"
```

F8 and a note, or a crash, saves the report in the player's save location and posts it to the server at once. A report that does not go (the player is off the tailnet, or klab is down) waits there, and goes the next time the game starts. Hollow names klab's server this way.

## Reports into marks

From a machine on the tailnet:

```bash
argus mark pull url=http://100.70.114.55:8765/
```

From a machine that reaches klab's SSH but not its port 8765, open a tunnel first and pull through it:

```bash
ssh -N -L 18765:127.0.0.1:8765 kevin@100.70.114.55
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

The page is swapped in with one rename; the server needs no restart, and reports already sent stay. If `tools/web/serve.mjs` itself changed, the script copies it to `~/argus-play/serve.mjs` and restarts the service.

The server sends the page brotli-compressed to a browser that takes it: about 2.7 MB instead of 14 MB, so the page loads in under 5 s on a 20 Mbit link (section 17; `tools/web/check.mjs` measures it).

## The games in a browser

The example games, Hollow, Delve, and Outpost, are also on a public site anyone can open, with no Tailscale: `https://argusengine.org/`, on Cloudflare (static files only; `tools/web/site/wrangler.toml` and `deploy.sh`, Kevin's account, the domain bought there 2026-10-27), and the same site from klab at `https://klab.tailde2255.ts.net/`. Its front page is Argus's own (`tools/web/site/index.html`): what Argus is, with Hollow running live in the page and what an agent sees of it drawn over the game, and the public release announced as coming soon. The page of the games (`tools/web/games/index.html`) is at `/games/`, and each game is in its own folder (`/hollow/`, `/delve/`, `/outpost/`). Swarm is not there: its chase is a Rust plugin, which a page cannot load, so it plays from a release's example games.

The front page's live game is the site's own `/hollow/` build, which `tools/web/site/live.js` loads beside it, so the page is only right on a site that has that build; `tools/web/site/assemble.sh <site dir>` puts the front page, the page of the games, and the pictures on one. To look at the front page here, assemble it around a Hollow web build and serve it:

```bash
mkdir -p /tmp/site && cp -r target-web/site /tmp/site/hollow
tools/web/site/assemble.sh /tmp/site
node tools/web/serve.mjs /tmp/site 8796 127.0.0.1 --read-only
```

It is a second server beside the playtest one, apart from it:

- `~/argus-games/site/`: the front page, the page of the games, each game's page as `argus build --target web --release` wrote it (without its `build.json`, which names this machine's folders), and the README's clips and stills.
- `argus-games.service`: a user service (`tools/web/argus-games.service`) running `serve.mjs --read-only` on `127.0.0.1:8766` only. Read-only, it serves the files and refuses every report (`POST .../marks` is 403) and every other method: the internet can read the site and write nothing to klab. A player's F8 there says the report did not go.
- Tailscale Funnel puts `127.0.0.1:8766` on the internet at port 443, set once on klab (it needs root, so Kevin runs it):

```bash
sudo tailscale funnel --bg --https=443 http://127.0.0.1:8766
```

The first time, Tailscale may ask for Funnel to be allowed for the tailnet, with a link to the admin console. `tailscale funnel status` shows it; `sudo tailscale funnel --https=443 off` takes it down. The playtest server on 8443 stays on the tailnet only.

To put up the games again (after a release, say), with an installed argus:

```bash
tools/web/publish-games.sh ~/AppData/Local/Argus/app/bin/argus.exe
```

It builds the three games' pages from copies of `examples/`, each checked by its build, adds the front page with `assemble.sh`, swaps the site in on klab with one rename, installs or restarts the service when its file or `serve.mjs` changed, and last deploys the same site to argusengine.org (wrangler must be signed in to Kevin's Cloudflare account: `npx wrangler login`, once). `node tools/web/games-check.mjs https://argusengine.org/ <out-dir>` opens each game in headless Chrome and says whether it runs, with a screenshot of each.

## Linux builds

klab builds the Linux release: the engine's source is in `~/argus` (updated from a git bundle, since klab has no access to GitHub), Rust is installed for `kevin`, and the build needs `libasound2-dev` and `libudev-dev`. In `~/argus`:

```bash
cargo run -q -p argus-cli -- --project examples/hollow build --release
```

The release lands in `examples/hollow/dist/linux/`. It needs glibc 2.39 or newer to run (Ubuntu 24.04 and later), because klab has that; a build for older systems would be made on an older one.
