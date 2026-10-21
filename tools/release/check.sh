#!/usr/bin/env bash
# Check a staged install (distribution item 1): its own argus, with Rust
# out of reach (cargo off the PATH, CARGO unset, and CARGO_HOME an empty
# folder, since a plugin build also looks there) and the engine's source
# out of the way, releases each example from the install's runtimes, for
# this platform and the web.
# `argus build` checks each release itself: the packs play and look as the
# project does, and the executable, run headless, reaches the same world.
#
#   tools/release/check.sh [stage dir] [--no-web] [--keep <dir>] [--for macos]
#
# The stage dir defaults to stage.sh's. Swarm has a Rust plugin, which is
# the one part of a release that needs Rust: it is released with cargo on
# the PATH, and without, where the build must say that Rust is needed.
# --keep puts the four games' releases for this platform in <dir>/<game>,
# for the release's example-games archive.
#
# --for macos releases them for the Mac instead, from the install's macOS
# runtime: a Mac executable is not run here, so `argus build` checks the
# packs alone. Swarm must be refused, its plugin being compiled on the
# machine it runs on, and --keep keeps the other three games.
set -euo pipefail
cd "$(dirname "$0")/../.."

STAGE=
WEB=1
KEEP=
FOR=
while [ $# -gt 0 ]; do
  case "$1" in
    --no-web) WEB=0; shift ;;
    --keep) KEEP=$2; shift 2 ;;
    --for) FOR=$2; shift 2 ;;
    -*) echo "check.sh: unknown flag $1" >&2; exit 2 ;;
    *) STAGE=$1; shift ;;
  esac
done
case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*) OS=windows; EXE=.exe ;;
  *) OS=linux; EXE= ;;
esac
VERSION=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)
STAGE=${STAGE:-target/stage/argus-$VERSION-$OS}
STAGE=$(cd "$STAGE" && pwd)
ARGUS=$STAGE/bin/argus$EXE
[ -x "$ARGUS" ] || { echo "check.sh: no $ARGUS; run tools/release/stage.sh" >&2; exit 1; }
if [ "$WEB" = 1 ] && [ ! -f "$STAGE/runtime/web/serve.mjs" ]; then
  echo "check.sh: the install has no playtest server, runtime/web/serve.mjs" >&2
  exit 1
fi

# The PATH without any directory that has cargo in it.
NOCARGO=$(printf '%s' "$PATH" | tr ':' '\n' | while IFS= read -r d; do
  [ -n "$d" ] && { [ -e "$d/cargo" ] || [ -e "$d/cargo.exe" ]; } || printf '%s:' "$d"
done)
NOCARGO=${NOCARGO%:}

WORK=${TMPDIR:-/tmp}/argus-release-check-$$
rm -rf "${WORK:?}"
mkdir -p "$WORK"
trap 'rm -rf "${WORK:?}"' EXIT

mkdir -p "$WORK/no-rust"

failed=0
release() { # name target rust|no-rust [expect-failure-text]
  local g=$1 target=$2 rust=$3 expect=${4:-}
  local -a envs=(-u ARGUS_ENGINE -u ARGUS_RUNTIME)
  if [ "$rust" = no-rust ]; then
    envs+=(-u CARGO CARGO_HOME="$WORK/no-rust" PATH="$NOCARGO")
  fi
  local root=$WORK/$g out=$WORK/out/$g-$target log=$WORK/$g-$target.log
  if [ ! -d "$root" ]; then
    cp -r "examples/$g" "$root"
    rm -rf "${root:?}/.argus" "${root:?}/dist"
  fi
  local t0=$SECONDS
  if env "${envs[@]}" \
      "$ARGUS" --project "$root" --json build --target "$target" --out "$out" > "$log" 2>&1; then
    if [ -n "$expect" ]; then
      echo "  $g $target: built, and should have said: $expect"; failed=1
    elif grep -q '"same":true' "$log" && grep -q '"prebuilt":true' "$log"; then
      echo "  $g $target: released from the install's runtime, checked ($((SECONDS - t0)) s)"
    else
      echo "  $g $target: built, but not from the runtime or not the same:"; tail -c 2000 "$log"; failed=1
    fi
  elif [ -n "$expect" ] && grep -q "$expect" "$log"; then
    echo "  $g $target: refused, as it should be: $(grep -o "$expect[^\"]*" "$log" | head -1)"
  else
    echo "  $g $target: FAILED"; tail -c 2000 "$log"; failed=1
  fi
}

echo "checking $STAGE ($("$ARGUS" version))"
if [ -n "$FOR" ]; then
  [ "$FOR" = macos ] || { echo "check.sh: --for takes macos" >&2; exit 2; }
  for g in delve hollow outpost puppet toybox duel; do
    release "$g" macos no-rust
  done
  release swarm macos rust "compiled for the machine they run on"
  if [ "$failed" = 0 ]; then
    echo "the install releases every example for macos"
    if [ -n "$KEEP" ]; then
      rm -rf "${KEEP:?}"
      mkdir -p "$KEEP"
      for g in hollow delve outpost duel; do
        cp -r "$WORK/out/$g-macos" "$KEEP/$g"
      done
      echo "kept the games' macos releases in $KEEP"
    fi
    exit 0
  fi
  echo "the install does NOT release every example for macos"
  exit 1
fi
for g in delve hollow outpost puppet toybox duel; do
  release "$g" "$OS" no-rust
  [ "$WEB" = 1 ] && release "$g" web no-rust
done
release swarm "$OS" no-rust "no cargo to build the plugin with"
release swarm "$OS" rust

if [ "$failed" = 0 ]; then
  echo "the install releases every example"
  if [ -n "$KEEP" ]; then
    rm -rf "${KEEP:?}"
    mkdir -p "$KEEP"
    for g in hollow delve swarm outpost; do
      cp -r "$WORK/out/$g-$OS" "$KEEP/$g"
    done
    echo "kept the games' $OS releases in $KEEP"
  fi
else
  echo "the install does NOT release every example"
  exit 1
fi
