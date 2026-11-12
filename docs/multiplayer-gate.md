# Multiplayer gate

The plan ([docs/multiplayer-checklist.md](multiplayer-checklist.md)) ends the multiplayer work with a gate:

> - The versus game played by two people on two machines, one in a browser and one on the desktop, through the relay on klab, over a link made slow on purpose, without falling out of step.
> - An agent with only the docs adds a feature to the versus game and checks it with a two-player scenario.
> - Every example still gives the same run on this PC, klab, and Chrome.

This file records that run, begun on 2026-10-21.

## The parts

| Part | Where it stands | Where to look |
|---|---|---|
| Duel played by two people on two machines, a browser and a desktop, through klab's relay over a slow link | Met, 2026-11-12 (below) | "Two people", below |
| An agent with only the docs adds a feature to Duel and checks it with a two-player scenario | Met (below) | `docs/multiplayer-gate/duel-block.diff` |
| Every example gives the same run on this PC, klab, and Chrome | Met (below) | `tools/determinism/ci.sh` |

## The trial

One agent (Opus 5.5) was given a copy of Duel outside the repository, an `argus` built from the commit before (424a891a), and the docs that tool gives. It was asked to add a **block**: a button that, held, walks at half speed, stops attacks and dashes, and turns a swing from the front into no point, both fighters pushed apart; at most 1.5 seconds at a time, then a second's rest. It must not read the engine's source, and done meant scenarios for every rule, at least one of them a two-player match online over a bad link with both players' input scripted, and the whole suite passing.

It built it. 9 new scenarios, among them two players over `t:net` (60 ms, 30 ms jitter, 5% loss) each blocking the other's swing from the front, then a swing from behind scoring through a block; both players rolled back (8 and 12 times), and it passed on seeds 1 to 20. It broke six of the rules one at a time on purpose and saw the matching scenarios fail. The suite ended `22 scenarios, 22 runs, 0 failed`, which I reran myself with the same tool. I read its transcript: 153 tool calls, none of them reaching the engine's source or the repository.

Its changes are in `docs/multiplayer-gate/duel-block.diff`, against Duel as committed (pictures left out). Whether the block joins Duel is Kevin's to decide: it is a good feature, and it makes the example longer to read.

What it found missing, and what became of each:

- `argus docs` with no topic prints the whole reference, 4,392 lines; the chapter list is `argus docs list`. `argus help docs` now says so.
- An event row's own `by` is the system that emitted it, so a game's event field named `by` is `row.fields.by`: testing.md says so.
- `q:play`'s event, `sound.play`, and its `sound` field were not described: presentation.md says what it is and how a scenario counts it.
- `audio describe` measures a variation before the sound's `gain`: said now.
- Art-script frames are packed edge to edge, so a frame drawn large can show a line of its neighbour: presentation.md says to leave a clear border. Its cause in the software rasteriser is a task of its own.
- Whether `argus test` picks up a changed component without a `scene load`: it does, each run loads its scene afresh, and testing.md says so.

## The same run everywhere

CI's det step, at 424a891a: all 16 long runs (Duel's two among them) ended every checkpoint on the same world on this PC, klab, and Chrome, Swarm's on the first two (it has no web build); all 361 scenario runs, Duel's 13 among them with its online match, ended on the same world on klab as here.

## Two people

What it needs, all in place:

- klab runs two relays on the tailnet: `ws://100.70.114.55:7879`, and `ws://100.70.114.55:7880`, which holds every message 80 ms and up to 40 more (`tools/relay/deploy.sh`).
- Duel's web build, served on the tailnet.
- Duel's desktop build for Windows.

Before the people, a check: `tools/net/check.mjs` had a page in headless Chrome host a match and the desktop build join it through the slow relay. 600 ticks, all 9 digest checks agreed on both sides, the desktop rolling back 32 times.

Then Kevin played it on 2026-11-12: the desktop build on this PC, hosting, and the web build in a browser on another machine, joining, both through the slow relay. They never fell out of step.

What it felt like:

- Laggy, as a link that holds every message 80 to 120 ms should be.
- The other player's swing often did not show, or only its end. A swing is out for 8 ticks, and the other player's input arrives about 5 to 8 ticks late on this link, of which the match's input delay (2 ticks, fixed by the desktop and web runners) hides only 2. By the time a rollback learns of the swing, most or all of its 8 ticks have passed, so its slash is drawn briefly or not at all.

Two things follow, neither part of the gate: a game should choose its own input delay, as the plan says it does (the runners fix it at 2 for now), and Duel could draw a swing learned late from its start.
