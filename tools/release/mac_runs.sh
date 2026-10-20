#!/usr/bin/env bash
# What tools/release/mac_expected.py reads, made with an install's own argus
# (release.sh passes the staged Windows install's, the release's own build):
#
#   tools/release/mac_runs.sh <argus> <runs dir> <scratch dir>
#
# In <runs dir>: every example game's scenarios, run with --json, each with
# the digest its world ended on (<game>.json); and, from a platformer
# project made in <scratch dir> where the Mac testing packet's section 7
# ran, the generate answers (gen-sound.json, gen-sprite.json) and the
# preview's (gen-preview.json, which mac_expected.py does not read). Every
# path is made absolute first: section 7 runs inside the project.
set -euo pipefail
cd "$(dirname "$0")/../.."
[ $# -eq 3 ] || { echo "usage: mac_runs.sh <argus> <runs dir> <scratch dir>" >&2; exit 2; }
abs() { mkdir -p "$1" && (cd "$1" && pwd); }
A=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
RUNS=$(abs "$2")
SCRATCH=$(abs "$3")
[ -x "$A" ] || { echo "mac_runs.sh: no $A" >&2; exit 1; }
rm -rf "${RUNS:?}"/* "${SCRATCH:?}/platformer"
for g in delve hollow outpost puppet swarm toybox; do
  t0=$SECONDS
  # Each game's suite in a copy, so the checkout's examples stay as they are.
  rm -rf "${SCRATCH:?}/$g"
  cp -r "examples/$g" "$SCRATCH/$g"
  rm -rf "${SCRATCH:?}/$g/.argus"
  if ! "$A" --project "$SCRATCH/$g" --json test > "$RUNS/$g.json" 2> "$SCRATCH/$g.err"; then
    echo "mac_runs.sh: $g's scenarios did not all pass (see $SCRATCH/$g.err)" >&2
    "$A" --project "$SCRATCH/$g" daemon stop > /dev/null 2>&1 || true
    exit 1
  fi
  "$A" --project "$SCRATCH/$g" daemon stop > /dev/null 2>&1 || true
  echo "  $g: $(python -c 'import json,sys; r=json.load(open(sys.argv[1],encoding="utf-8")); print(r["passed_runs"], "runs passed")' "$RUNS/$g.json") ($((SECONDS - t0)) s)"
done
"$A" new "$SCRATCH/platformer" --template platformer > "$SCRATCH/new.log" 2>&1
(
  cd "$SCRATCH/platformer"
  "$A" generate name=sfx/zap sound=laser seed=3 --json > "$RUNS/gen-sound.json"
  "$A" generate name=creatures/blob sprite=creature seed=2 --json > "$RUNS/gen-sprite.json"
  "$A" asset preview creatures/blob --json > "$RUNS/gen-preview.json"
  "$A" daemon stop > /dev/null 2>&1 || true
)
echo "  section 7: the platformer's sound, sprite, and preview"
