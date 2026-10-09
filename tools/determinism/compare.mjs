// Line up the determinism spike's digests (phase 5 item 11): for each run,
// every platform's against the first label's, checkpoint by checkpoint:
//
//   node tools/determinism/compare.mjs windows web linux ...
//
// Reads target/determinism/<label>-<game>-<seed>.json (native.sh and
// web.mjs write them) and says, per run and label, "same" or the first
// checkpoint where the world's or the events' digest differs, which is
// where to dump the rows (--rows) and compare them. Prints JSON; exits 1
// when any run differs.
import { readFileSync, readdirSync, existsSync } from "node:fs";

const dir = "target/determinism";
// `--scenarios a b`: every scenario run's final digest, a against b.
if (process.argv[2] === "--scenarios") {
  const [a, b] = process.argv.slice(3);
  const rows = (label, game) => {
    const f = `${dir}/${label}-scenarios-${game}.json`;
    if (!existsSync(f)) return null;
    const r = JSON.parse(readFileSync(f, "utf8"));
    const m = new Map();
    for (const s of r.scenarios ?? r.result?.scenarios ?? []) for (const run of s.seeds ?? []) m.set(`${s.name} (seed ${run.seed})`, run.digest);
    return m;
  };
  const games = [...new Set(readdirSync(dir).filter((f) => f.includes("-scenarios-") && f.endsWith(".json")).map((f) => f.replace(/^.*-scenarios-|\.json$/g, "")))];
  const out = [];
  let differ = false;
  for (const g of games) {
    const [x, y] = [rows(a, g), rows(b, g)];
    if (!x || !y) {
      out.push({ game: g, [b]: "not measured" });
      continue;
    }
    const keys = [...x.keys()].filter((k) => y.has(k));
    const diff = keys.filter((k) => x.get(k) !== y.get(k) || !x.get(k));
    differ ||= diff.length > 0;
    out.push({ game: g, runs: keys.length, same: keys.length - diff.length, differ: diff.length, only_in_one: x.size + y.size - 2 * keys.length, first_differing: diff.slice(0, 8) });
  }
  console.log(JSON.stringify(out, null, 1));
  process.exit(differ ? 1 : 0);
}
const labels = process.argv.slice(2);
if (labels.length < 2) {
  console.error("usage: compare.mjs <label> <label>... | --scenarios <label> <label>");
  process.exit(2);
}
const runs = readdirSync("tools/determinism/runs").map((f) => f.replace(/\.json$/, ""));
// A run that failed left an empty file: not measured, like a missing one.
const read = (label, run) => {
  try {
    return JSON.parse(readFileSync(`${dir}/${label}-${run}.json`, "utf8"));
  } catch {
    return null;
  }
};
const report = [];
let differ = false;
for (const run of runs) {
  const base = read(labels[0], run);
  const row = { run, ticks: base?.ticks, checkpoints: base?.digests.length, entities: base?.entities };
  for (const label of labels.slice(1)) {
    const other = read(label, run);
    if (!base || !other) {
      row[label] = "not measured";
      continue;
    }
    const i = base.digests.findIndex((d, k) => JSON.stringify(d) !== JSON.stringify(other.digests[k]));
    if (i < 0 && base.digests.length === other.digests.length) {
      row[label] = "same";
      continue;
    }
    differ = true;
    const [a, b] = [base.digests[i], other.digests[i]];
    row[label] = {
      first_differs_at: a?.[0] ?? b?.[0],
      after: i > 0 ? base.digests[i - 1][0] : 0,
      world: a?.[1] !== b?.[1],
      events: a?.[2] !== b?.[2],
      checkpoints_same: i,
    };
  }
  report.push(row);
}
console.log(JSON.stringify(report, null, 1));
process.exit(differ ? 1 : 0);
