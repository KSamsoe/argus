#!/usr/bin/env bash
# The same run on every machine, checked (docs/multiplayer-checklist.md
# item 0): the determinism spike's runs and every example's scenarios,
# played on this machine and on a Linux host, and the runs again in
# headless Chrome; any difference between the three fails.
#
#   tools/determinism/ci.sh
#
# The Linux host is $ARGUS_DET_HOST (default klab), reached with ssh and a
# git bundle of HEAD, as tools/release/release.sh does; it builds in
# ~/argus-ci. So what is checked there is the last commit: the script
# refuses a tree with uncommitted changes, which would check two things.
# Swarm has no browser build (its chase is a Rust plugin), so its runs are
# compared on the two native machines only.
#
# Writes target/determinism/; the comparisons are det-runs.json and
# det-scenarios.json there.
set -uo pipefail
cd "$(dirname "$0")/../.."

HOST=${ARGUS_DET_HOST:-klab}
OUT=target/determinism
NATIVE=windows
[ "${OS:-}" = "Windows_NT" ] || NATIVE=linux
GAMES=(hollow delve swarm outpost toybox puppet)
WEB_GAMES=(hollow delve outpost toybox puppet)

git update-index -q --refresh > /dev/null 2>&1 || true
if [ -n "$(git status --porcelain --untracked-files=no)" ]; then
  echo "det: the tree has uncommitted changes, and the Linux host checks the last commit" >&2
  exit 1
fi
SHA=$(git rev-parse HEAD)
rm -rf "$OUT"
mkdir -p "$OUT"

# The Linux host works while this machine does: its half starts first.
echo "=== linux ($HOST), at $SHA: started"
git bundle create "$OUT/head.bundle" HEAD > /dev/null 2>&1
scp -q -o BatchMode=yes "$OUT/head.bundle" "$HOST:/tmp/argus-ci.bundle" || {
  echo "det: cannot reach $HOST" >&2
  exit 1
}
# ssh joins its arguments into one command line, so the games go as one
# word each after the commit.
ssh -o BatchMode=yes "$HOST" bash -s -- "$SHA" "${GAMES[@]}" > "$OUT/klab.log" 2>&1 <<'REMOTE' &
set -uo pipefail
SHA=$1
shift
export PATH="$HOME/.cargo/bin:$PATH"
[ -d ~/argus-ci/.git ] || git init -q ~/argus-ci
cd ~/argus-ci
git fetch -q /tmp/argus-ci.bundle HEAD && git checkout -q --force "$SHA" || exit 1
git clean -qfdx -e target
rm -rf target/determinism
bash tools/determinism/native.sh klab || exit 1
bash tools/determinism/scenarios.sh klab "$@"
tar -C target/determinism -czf /tmp/argus-ci-det.tar.gz .
REMOTE
REMOTE_PID=$!

echo "=== $NATIVE: the runs and the scenarios"
bash tools/determinism/native.sh "$NATIVE" || exit 1
bash tools/determinism/scenarios.sh "$NATIVE" "${GAMES[@]}"

# After the scenarios: a web build opens the project's daemon, which a
# scenario run beside it would share (one job a project at a time).
echo "=== Chrome: the runs, from each game's web build"
cargo build -q -p argus-cli || exit 1
EXE=""
[ "${OS:-}" = "Windows_NT" ] && EXE=.exe
for g in "${WEB_GAMES[@]}"; do
  "target/debug/argus$EXE" --project "examples/$g" build --target web --release --out "target-web/det/$g" > "$OUT/web-build-$g.log" 2>&1 || {
    echo "det: $g's web build failed ($OUT/web-build-$g.log)"
    tail -n 20 "$OUT/web-build-$g.log"
    exit 1
  }
  "target/debug/argus$EXE" --project "examples/$g" daemon stop > /dev/null 2>&1
  timeout 1800 node tools/determinism/web.mjs "target-web/det/$g" tools/determinism/runs/"$g"-*.json || exit 1
done

echo "=== linux ($HOST): waiting for its half"
wait "$REMOTE_PID" || {
  echo "det: $HOST's half failed:"
  tail -n 30 "$OUT/klab.log"
  exit 1
}
cat "$OUT/klab.log"
scp -q -o BatchMode=yes "$HOST:/tmp/argus-ci-det.tar.gz" "$OUT/klab.tar.gz" || exit 1
# -m: the two clocks differ by seconds, and tar warns of files from the future.
tar --force-local -m -C "$OUT" -xzf "$OUT/klab.tar.gz" || exit 1

echo "=== the comparison"
failed=0
node tools/determinism/compare.mjs "$NATIVE" klab web > "$OUT/det-runs.json" || failed=1
node tools/determinism/compare.mjs --scenarios "$NATIVE" klab > "$OUT/det-scenarios.json" || failed=1
node -e '
const fs = require("fs");
const runs = JSON.parse(fs.readFileSync(process.argv[1]));
for (const r of runs) {
  const show = (v) => (typeof v === "string" ? v : `differs from tick ${v.first_differs_at}`);
  console.log(`${r.run}: klab ${show(r.klab)}, Chrome ${show(r.web)}`);
}
for (const g of JSON.parse(fs.readFileSync(process.argv[2]))) {
  console.log(`${g.game} scenarios: ${g.runs ?? 0} runs, ${g.same ?? 0} the same on klab` + (g.only_in_one ? `, ${g.only_in_one} run on one machine only` : "") + (g.differ ? `, ${g.differ} differ: ${g.first_differing.join("; ")}` : ""));
}' "$OUT/det-runs.json" "$OUT/det-scenarios.json"
# Swarm is never measured in Chrome; anything else not measured everywhere
# is a failure, as is a game whose scenarios did not all run on both.
if node -e '
const fs = require("fs");
const runs = JSON.parse(fs.readFileSync(process.argv[1]));
const games = JSON.parse(fs.readFileSync(process.argv[2]));
const bad = [
  ...runs.filter((r) => r.klab === "not measured" || (r.web === "not measured" && !r.run.startsWith("swarm-"))).map((r) => `${r.run}: not measured on every platform`),
  ...process.argv.slice(3).filter((g) => !games.some((x) => x.game === g && x.runs > 0 && !x.only_in_one)).map((g) => `${g}: its scenarios did not all run on both machines`),
];
for (const b of bad) console.log(b);
process.exit(bad.length ? 1 : 0);' "$OUT/det-runs.json" "$OUT/det-scenarios.json" "${GAMES[@]}"; then :; else failed=1; fi
exit $failed
