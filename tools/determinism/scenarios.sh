#!/usr/bin/env bash
# The determinism spike's scenario tier (phase 5 item 11): every scenario of
# every game over its seeds, on this machine, each run's final world digest
# kept (test.run's rows carry it), written as
# target/determinism/<label>-scenarios-<game>.json:
#
#   tools/determinism/scenarios.sh <label> [game...]
#
# The four reference games, and toybox (rigid bodies, on Rapier) and puppet
# (skeletons, the engine's heaviest use of sin, cos, and atan2) beside them.
# `compare.mjs --scenarios <label> <label>` lines two machines up.
set -uo pipefail
cd "$(dirname "$0")/../.."
LABEL=${1:?usage: scenarios.sh <label> [game...]}
shift
GAMES=("$@")
[ ${#GAMES[@]} -eq 0 ] && GAMES=(hollow delve swarm outpost toybox puppet duel)
cargo build -q --release -p argus-cli || exit 1
ARGUS=target/release/argus
[ -x "$ARGUS.exe" ] && ARGUS="$ARGUS.exe"
mkdir -p target/determinism
for g in "${GAMES[@]}"; do
  t0=$SECONDS
  "$ARGUS" --project "examples/$g" test --json --software > "target/determinism/$LABEL-scenarios-$g.json" 2> "target/determinism/$LABEL-scenarios-$g.err"
  code=$?
  "$ARGUS" --project "examples/$g" daemon stop > /dev/null 2>&1
  echo "$g: exit $code in $((SECONDS - t0)) s"
done
