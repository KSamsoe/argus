// The determinism spike's runs (phase 5 item 11): one long run a game, its
// input made by a seeded walk, so every platform plays the same bytes:
//
//   node tools/determinism/runs.mjs        (writes tools/determinism/runs/)
//
// A run file is {game, project, scene, seed, ticks, every, log}; the log is
// an input log's rows, [tick, slot, action, value], a value held until the
// next row for the action. The runs are long enough to reach what each
// game does over time: Hollow's rooms and the deep, Delve's later floors,
// Swarm's later waves, Outpost's week.
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const out = new URL("./runs/", import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1");
mkdirSync(out, { recursive: true });

// mulberry32: the same numbers from the same seed, in any Node.
function rng(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
const between = (r, lo, hi) => lo + Math.floor(r() * (hi - lo + 1));
const pick = (r, list) => list[Math.floor(r() * list.length)];

// Hollow: run mostly right, stop and turn back now and then, and jump at
// random, as a player finding the way does.
function hollow(r, ticks) {
  const log = [];
  for (let t = 1; t < ticks; ) {
    const x = r() < 0.6 ? 1 : r() < 0.5 ? -1 : 0;
    log.push([t, 1, "move", [x, 0]]);
    const hold = between(r, 20, 120);
    for (let j = t + between(r, 5, 40); j < t + hold; j += between(r, 25, 70)) {
      log.push([j, 1, "jump", true], [j + between(r, 4, 16), 1, "jump", false]);
    }
    t += hold;
  }
  return dedupe(log);
}

// Delve: a step a turn, in a random direction, a wait now and then, and
// the stairs tried every so often (they work only on them).
function delve(r, turns) {
  const log = [];
  let last = null;
  for (let t = 1; t < turns; t++) {
    if (r() < 0.08) {
      log.push([t, 1, "wait", true], [t + 1, 1, "wait", false]);
      if (last !== "0,0") log.push([t, 1, "move", [0, 0]]), (last = "0,0");
      continue;
    }
    if (t % between(r, 15, 30) === 0) log.push([t, 1, "descend", true], [t + 1, 1, "descend", false]);
    const d = pick(r, [[1, 0], [-1, 0], [0, 1], [0, -1]]);
    if (String(d) !== last) log.push([t, 1, "move", d]), (last = String(d));
  }
  return dedupe(log);
}

// Swarm: keep moving, a new direction every second or two, stopping
// sometimes, as a player kiting the swarm does.
function swarm(r, ticks) {
  const dirs = [[1, 0], [-1, 0], [0, 1], [0, -1], [0.7071, 0.7071], [-0.7071, 0.7071], [0.7071, -0.7071], [-0.7071, -0.7071], [0, 0]];
  const log = [];
  for (let t = 1; t < ticks; t += between(r, 30, 120)) log.push([t, 1, "move", pick(r, dirs)]);
  return log;
}

// Outpost runs itself: no input, the colony's week at 1x (and toybox and
// puppet, which have no input map).
const outpost = () => [];

// Two rows for one action at one tick: the later wins, as it would live.
function dedupe(log) {
  const seen = new Map();
  for (const row of log) seen.set(`${row[0]}|${row[2]}`, row);
  return [...seen.values()].sort((a, b) => a[0] - b[0]);
}

// A few seeds a game: Delve's hero dies on the first floors and the game
// has no restart, so it has more, shorter runs, each a new dungeon.
const games = [
  { game: "hollow", scene: "hollow", seeds: [1, 2], ticks: 7200, every: 60, make: hollow },
  { game: "delve", scene: "delve", seeds: [1, 2, 3, 4, 5, 6], ticks: 500, every: 10, make: delve },
  { game: "swarm", scene: "arena", seeds: [1, 2], ticks: 10800, every: 60, make: swarm },
  { game: "outpost", scene: "valley", seeds: [1, 2], ticks: 25200, every: 120, make: outpost },
  // Beside the four: toybox's rigid bodies (Rapier) and puppet's
  // skeletons (sin, cos, and atan2 every bone, every tick) run themselves.
  { game: "toybox", scene: "toybox", seeds: [1], ticks: 3600, every: 30, make: outpost },
  { game: "puppet", scene: "stage", seeds: [1], ticks: 3600, every: 30, make: outpost },
];
for (const { make, seeds, ...g } of games) {
  for (const seed of seeds) {
    const run = { ...g, seed };
    const log = make(rng(seed * 7919 + g.game.length), g.ticks);
    const file = join(out, `${g.game}-${seed}.json`);
    writeFileSync(file, JSON.stringify({ ...run, project: `examples/${g.game}`, log }) + "\n");
    console.log(`${g.game} seed ${seed}: ${g.ticks} ticks, ${log.length} input rows`);
  }
}
