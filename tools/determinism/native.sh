#!/usr/bin/env bash
# The determinism spike's native half (phase 5 item 11): every run file,
# played by this machine's release build, its digests written as
# target/determinism/<label>-<game>-<seed>.json:
#
#   tools/determinism/native.sh <label> [run.json...]
#
# <label> names the platform in the comparison (windows, linux, klab). The
# probe is the argus-web example `determinism`, built here in release;
# Swarm's plugin is built by cargo when its run opens, so cargo must be on
# the PATH. compare.mjs lines the labels up.
set -uo pipefail
cd "$(dirname "$0")/../.."
LABEL=${1:?usage: native.sh <label> [run.json...]}
shift
RUNS=("$@")
[ ${#RUNS[@]} -eq 0 ] && RUNS=(tools/determinism/runs/*.json)
cargo build -q --release -p argus-web --features rigid --example determinism || exit 1
PROBE=target/release/examples/determinism
[ -x "$PROBE.exe" ] && PROBE="$PROBE.exe"
mkdir -p target/determinism
failed=0
for run in "${RUNS[@]}"; do
  name=$(basename "$run" .json)
  if "$PROBE" "$run" > "target/determinism/$LABEL-$name.json" 2> "target/determinism/$LABEL-$name.err"; then
    echo "$name: done"
  else
    echo "$name: FAILED (target/determinism/$LABEL-$name.err)"
    tail -n 5 "target/determinism/$LABEL-$name.err"
    failed=1
  fi
done
exit $failed
