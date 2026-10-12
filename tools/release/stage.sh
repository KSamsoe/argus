#!/usr/bin/env bash
# Stage an install of Argus for this machine's platform (distribution
# items 0 and 1): the programs, and the game runtimes `argus build` puts a
# game's packs beside, so releasing a game needs no Rust.
#
#   tools/release/stage.sh [out] [--no-web] [--target macos]
#
# out defaults to target/stage/argus-<version>-<os>. It holds:
#
#   bin/argus, bin/argus-editor          (argus is the daemon too)
#   runtime/<os>/standard/argus-game     every engine module but Steam
#   runtime/<os>/steam/argus-game        with Steam, and Steam's library
#   runtime/web/pkg/, index.html, main.js
#   templates/                           what `argus new` starts from
#   install.ps1 or install.sh            what `argus self-update` runs
#   manifest.json                        every file, its size and SHA-256
#
# Each runtime directory has runtime.json (version, revision, target,
# variant, modules), which `argus build` checks against itself. Every
# program is built with ARGUS_REVISION, the commit (and -dirty when the
# tree has uncommitted changes), so `argus version` and every report name
# it. The web runtime needs wasi-sdk (WASI_SDK, see tools/web/env.sh), the
# wasm32-unknown-unknown target, and wasm-bindgen-cli at Cargo.lock's
# version; --no-web skips it.
#
# --target macos, on Linux, cross-builds a macOS install instead: each
# program built for Apple Silicon and for Intel Macs through cargo-zigbuild
# and Apple's SDK (tools/release/macos-env.sh, docs/macos-checklist.md),
# the two joined into one (lipo.py),
# without the web runtime (release.sh adds the one built on Windows). It
# cannot run what it built, so it checks each program's architectures and
# signature in place of running `argus version`.
set -euo pipefail
cd "$(dirname "$0")/../.."

OUT=
WEB=1
CROSS=
while [ $# -gt 0 ]; do
  case "$1" in
    --no-web) WEB=0; shift ;;
    --target) CROSS=$2; shift 2 ;;
    -*) echo "stage.sh: unknown flag $1" >&2; exit 2 ;;
    *) OUT=$1; shift ;;
  esac
done

case "$(uname -s)/$CROSS" in
  MINGW*/|MSYS*/|CYGWIN*/) OS=windows; EXE=.exe; STEAMLIB=steam_api64.dll ;;
  Linux/) OS=linux; EXE=; STEAMLIB=libsteam_api.so ;;
  Linux/macos)
    OS=macos; EXE=; STEAMLIB=libsteam_api.dylib; WEB=0
    . tools/release/macos-env.sh
    ;;
  */macos) echo "stage.sh: the macOS install is cross-built on Linux" >&2; exit 1 ;;
  */?*) echo "stage.sh: no target $CROSS (macos is the one to cross-build)" >&2; exit 2 ;;
  *) echo "stage.sh: $(uname -s) is not a release platform yet (Windows and Linux are)" >&2; exit 1 ;;
esac

VERSION=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)
REV=$(git rev-parse --short=12 HEAD)
if [ -n "$(git status --porcelain --untracked-files=no)" ]; then
  REV="$REV-dirty"
fi
export ARGUS_REVISION=$REV
OUT=${OUT:-target/stage/argus-$VERSION-$OS}
TARGET=${CARGO_TARGET_DIR:-target}
SHIP=$TARGET/ship
if [ "$OS" = macos ]; then
  SHIP=$TARGET/macos-universal/ship
fi
echo "staging Argus $VERSION ($REV) for $OS in $OUT"

rm -rf "${OUT:?}"
mkdir -p "$OUT/bin" "$OUT/runtime/$OS/standard" "$OUT/runtime/$OS/steam"

# ship <program>... -- <cargo args>: build the programs for this machine,
# or for each of a Mac's two architectures, each pair joined into one
# program (lipo.py; cargo-zigbuild's own universal2 target once made an
# editor whose arm64 part was a fraction of its arm64 build).
ship() {
  local names=()
  while [ "$1" != -- ]; do names+=("$1"); shift; done
  shift
  if [ "$OS" = macos ]; then
    for t in aarch64-apple-darwin x86_64-apple-darwin; do
      cargo zigbuild -q --profile ship --target "$t" "$@"
    done
    mkdir -p "$SHIP"
    for n in "${names[@]}"; do
      python3 tools/release/lipo.py "$SHIP/$n" \
        "$TARGET/aarch64-apple-darwin/ship/$n" "$TARGET/x86_64-apple-darwin/ship/$n"
    done
  else
    cargo build -q --profile ship "$@"
  fi
}

meta() { # dir target variant modules-json
  printf '{"version":"%s","revision":"%s","target":"%s","variant":"%s","modules":%s}\n' \
    "$VERSION" "$REV" "$2" "$3" "$4" > "$1/runtime.json"
}

# The programs.
ship argus argus-editor -- -p argus-cli -p argus-editor
cp "$SHIP/argus$EXE" "$SHIP/argus-editor$EXE" "$OUT/bin/"

# The installer, which `argus self-update` and `argus uninstall` run.
if [ "$OS" = windows ]; then
  cp tools/release/install.ps1 "$OUT/"
else
  cp tools/release/install.sh "$OUT/"
fi

# The templates `argus new` starts from.
cp -r templates "$OUT/templates"
find "$OUT/templates" \( -name .argus -o -name dist \) -prune -exec rm -rf {} +

# The desktop runtimes. Each build of argus-game overwrites the last, so
# each is copied out before the next.
ship argus-game -- -p argus-game --features rigid
cp "$SHIP/argus-game$EXE" "$OUT/runtime/$OS/standard/"
meta "$OUT/runtime/$OS/standard" "$OS" standard '["rigid"]'

ship argus-game -- -p argus-game --features rigid,steam
cp "$SHIP/argus-game$EXE" "$OUT/runtime/$OS/steam/"
# Steam's library, as steamworks-sys copied it out of Valve's SDK: the
# newest, when several builds left one. Valve's Mac library is already
# universal, so either architecture's copy will do.
BUILT=$SHIP
[ "$OS" = macos ] && BUILT=$TARGET/aarch64-apple-darwin/ship
LIB=$(ls -t "$BUILT"/build/steamworks-sys-*/out/"$STEAMLIB" 2>/dev/null | head -1)
if [ -z "$LIB" ]; then
  echo "stage.sh: the steam build left no $STEAMLIB under $SHIP/build" >&2
  exit 1
fi
cp "$LIB" "$OUT/runtime/$OS/steam/"
meta "$OUT/runtime/$OS/steam" "$OS" steam '["rigid","steam"]'

# The web runtime: what `argus build --target web` compiles from source.
if [ "$WEB" = 1 ]; then
  . tools/web/env.sh
  mkdir -p "$OUT/runtime/web/pkg"
  CARGO_TARGET_DIR=target-web cargo rustc -q -p argus-web --release \
    --target wasm32-unknown-unknown --crate-type cdylib --features rigid
  wasm-bindgen --target web --out-dir "$OUT/runtime/web/pkg" \
    target-web/wasm32-unknown-unknown/release/argus_web.wasm
  cp tools/web/index.html tools/web/main.js "$OUT/runtime/web/"
  meta "$OUT/runtime/web" web web '["rigid"]'
fi

# A Mac runs nothing unsigned, and the ship profile's strip takes the
# linker's signature off: each program is signed again, ad hoc (signed by
# no one; enough to run). Steam's library keeps Valve's signature.
if [ "$OS" = macos ]; then
  for f in "$OUT"/bin/* "$OUT"/runtime/macos/*/argus-game; do
    log=$(rcodesign sign "$f" 2>&1) || { echo "stage.sh: signing $f failed: $log" >&2; exit 1; }
  done
fi

# The manifest: every file with its size and SHA-256.
bash tools/release/manifest.sh "$OUT" "$VERSION" "$REV" "$OS"

echo "staged: $(grep -c '"path"' "$OUT/manifest.json") files, $(du -sh "$OUT" | cut -f1)"
if [ "$OS" = macos ]; then
  # Not runnable here: each program must be both architectures, signed,
  # for macOS 11 on, loading only what a Mac has (macho_check.py).
  python3 tools/release/macho_check.py "$OUT"/bin/* "$OUT"/runtime/macos/*/argus-game \
    "$OUT"/runtime/macos/steam/"$STEAMLIB"
else
  "$OUT/bin/argus$EXE" version
fi
