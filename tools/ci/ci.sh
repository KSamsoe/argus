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
# in headless Chrome, its cold load over 20 Mbit included.
#
# Every step runs; the script says which failed and exits 1 if any did.
# Each step's output is in target/ci/<step>.log, which the workflow keeps.
# It runs on the reference laptop as a self-hosted GitHub Actions runner
# (docs/ci.md), and by hand the same way.
set -uo pipefail
cd "$(dirname "$0")/../.."

export PATH="${CARGO_HOME:-$USERPROFILE/.cargo}/bin:$PATH" CARGO_TERM_COLOR=never
LOGS=target/ci
mkdir -p "$LOGS"
EXE=""
[ "${OS:-}" = "Windows_NT" ] && EXE=.exe

# On the runner the build cache is cleaned past 60 GB: stale builds of
# every test binary pile up (target/debug once reached 213 GB);
# and the daemons of a run that was cancelled (a newer push cancels it) are
# stopped, since a running one locks its binary against the next build.
if [ "${CI:-}" = true ]; then
  if [ -x "target/debug/argus$EXE" ]; then
    for g in swarm delve hollow outpost; do
      "target/debug/argus$EXE" --project "examples/$g" daemon stop > /dev/null 2>&1
    done
  fi
  mb=$(du -sm target 2>/dev/null | cut -f1)
  if [ "${mb:-0}" -gt 61440 ]; then
    echo "--- the build cache is $((mb / 1024)) GB: cleaned (this run builds cold)"
    cargo clean -q
  fi
fi

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
  for g in swarm delve hollow outpost; do
    echo "=== $g"
    "target/debug/argus$EXE" --project "examples/$g" test --software || failed=1
    "target/debug/argus$EXE" --project "examples/$g" daemon stop
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
  node tools/web/check.mjs > "$LOGS/web-check.json" || { cat "$LOGS/web-check.json"; return 1; }
  node -e 'const r = JSON.parse(require("fs").readFileSync(process.argv[1])); console.log(`cold load ${r.cold_load.ms} ms, ${r.cold_load.bytes} bytes (${r.cold_load.link})`)' "$LOGS/web-check.json"
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
