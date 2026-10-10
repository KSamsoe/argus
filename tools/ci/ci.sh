#!/usr/bin/env bash
# Everything CI checks, cheapest failure first (phase 5 item 0):
#
#   tools/ci/ci.sh [step...]     (steps: fmt clippy test games perf web; none runs all)
#
# fmt and clippy: the code as committed, no warnings. test: the workspace's
# tests, and the render tests again on the software rasteriser as a machine
# with no GPU draws (section 2). games: the four games' suites, failing runs' clips drawn with the
# software rasteriser (section 9). perf: the release-only tests, which gate
# every section 17 target but the web page's (the Outpost hand-tuning gate,
# 21 minutes, is left to phase gates). web: Hollow's page built and checked
# in headless Chrome, its cold load over 20 Mbit included, and the
# daemon's browser editor page checked as a phone uses it.
#
# Every step runs; the script says which failed and exits 1 if any did.
# Each step's output is in target/ci/<step>.log, which the workflow keeps.
# It runs on the reference laptop as a self-hosted GitHub Actions runner
# (docs/ci.md), and by hand the same way.
set -uo pipefail
cd "$(dirname "$0")/../.."

export PATH="${CARGO_HOME:-$USERPROFILE/.cargo}/bin:$PATH" CARGO_TERM_COLOR=never
LOGS=target/ci
# Where cargo builds (a worktree must not share another checkout's: same-named
# crates collide and a build links the other tree's code).
TARGET=${CARGO_TARGET_DIR:-target}
# The projects a run opens are not the person's to come back to: Argus's own
# data (the recent projects, argus projects) goes beside the logs.
export ARGUS_HOME="${ARGUS_HOME:-$PWD/$LOGS/argus-home}"
EXE=""
[ "${OS:-}" = "Windows_NT" ] && EXE=.exe

# On the runner the build cache is cleaned past 100 GB: stale builds of
# every test binary pile up (target/debug once reached 213 GB), and a
# working cache, debug and release, is about 85. And the daemons of a run
# that was cancelled (a newer push cancels it) are stopped, since a running
# one locks its binary against the next build.
if [ "${CI:-}" = true ]; then
  if [ -x "$TARGET/debug/argus$EXE" ]; then
    for g in swarm delve hollow outpost toybox puppet; do
      "$TARGET/debug/argus$EXE" --project "examples/$g" daemon stop > /dev/null 2>&1
    done
  fi
  # robocopy's listing on Windows: du takes minutes over the cache's
  # half a million files.
  if [ -n "$EXE" ]; then
    mb=$(MSYS_NO_PATHCONV=1 robocopy target 'C:\__nowhere__' /L /S /NJH /BYTES /NFL /NDL /NP /R:0 /W:0 /XJ 2>/dev/null |
      awk '/Bytes :/ { print int($3 / 1048576); exit }')
  else
    mb=$(du -sm target 2>/dev/null | cut -f1)
  fi
  if [ "${mb:-0}" -gt 102400 ]; then
    echo "--- the build cache is $((mb / 1024)) GB: cleaned (this run builds cold)"
    cargo clean -q
  fi
fi
# After the cleaning, which takes the whole of target/ with it.
mkdir -p "$LOGS"

step_fmt() { cargo fmt --all -- --check --color never; }
step_clippy() { cargo clippy --workspace --all-targets -q -- -D warnings; }
step_test() {
  local failed=0
  cargo test --workspace --no-fail-fast -q || failed=1
  echo "=== the render tests again, on the software rasteriser"
  ARGUS_SOFTWARE=1 cargo test -p argus-render --no-fail-fast -q || failed=1
  return $failed
}
step_games() {
  cargo build -q -p argus-cli || return 1
  local failed=0
  for g in swarm delve hollow outpost toybox puppet; do
    echo "=== $g"
    "$TARGET/debug/argus$EXE" --project "examples/$g" test --software || failed=1
    "$TARGET/debug/argus$EXE" --project "examples/$g" daemon stop
  done
  return $failed
}
# One test at a time: a test binary's tests otherwise share the machine,
# and a timing gate measured beside its siblings missed by a fifth.
step_perf() {
  cargo test --release --workspace --no-fail-fast -q -- --ignored --test-threads=1 \
    --skip a_person_tunes_outpost_at_10x_and_the_agent_sweeps_the_path
}
step_web() {
  bash tools/web/build.sh || return 1
  # The check stops itself in five minutes; ten is the last word (a run
  # once waited two hours on a Chrome that did not answer).
  timeout 600 node tools/web/check.mjs > "$LOGS/web-check.json" || { cat "$LOGS/web-check.json"; return 1; }
  node -e 'const r = JSON.parse(require("fs").readFileSync(process.argv[1])); console.log(`cold load ${r.cold_load.ms} ms, ${r.cold_load.bytes} bytes (${r.cold_load.link})`)' "$LOGS/web-check.json"
  # The browser editor's page (phase 5 item 10): a review answered by a
  # tap on a phone-size page, the viewport watched, over the daemon's web
  # listener on this machine.
  cargo build -q -p argus-cli || return 1
  ARGUS="$TARGET/debug/argus$EXE" timeout 300 node tools/web/editor-check.mjs > "$LOGS/editor-check.json" || { cat "$LOGS/editor-check.json"; return 1; }
}

STEPS=("$@")
[ ${#STEPS[@]} -eq 0 ] && STEPS=(fmt clippy test games perf web)
FAILED=()
for s in "${STEPS[@]}"; do
  case $s in
    fmt | clippy | test | games | perf | web) ;;
    *) echo "no step $s: fmt clippy test games perf web" >&2; exit 2 ;;
  esac
  t0=$SECONDS
  echo "--- $s"
  if "step_$s" > "$LOGS/$s.log" 2>&1; then
    echo "    passed in $((SECONDS - t0)) s"
  else
    echo "    FAILED in $((SECONDS - t0)) s; the last of $LOGS/$s.log:"
    tail -n 40 "$LOGS/$s.log" | sed 's/^/    | /'
    FAILED+=("$s")
  fi
done
if [ ${#FAILED[@]} -gt 0 ]; then
  echo "failed: ${FAILED[*]}"
  exit 1
fi
echo "all passed: ${STEPS[*]}"
