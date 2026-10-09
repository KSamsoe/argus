#!/usr/bin/env bash
# Stage an install of Argus for this machine's platform (distribution
# items 0 and 1): the programs, and the game runtimes `argus build` puts a
# game's packs beside, so releasing a game needs no Rust.
#
#   tools/release/stage.sh [out] [--no-web]
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
set -euo pipefail
cd "$(dirname "$0")/../.."

OUT=
WEB=1
for a in "$@"; do
  case "$a" in
    --no-web) WEB=0 ;;
    -*) echo "stage.sh: unknown flag $a" >&2; exit 2 ;;
    *) OUT=$a ;;
  esac
done

case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*) OS=windows; EXE=.exe; STEAMLIB=steam_api64.dll ;;
  Linux) OS=linux; EXE=; STEAMLIB=libsteam_api.so ;;
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
echo "staging Argus $VERSION ($REV) for $OS in $OUT"

rm -rf "${OUT:?}"
mkdir -p "$OUT/bin" "$OUT/runtime/$OS/standard" "$OUT/runtime/$OS/steam"

meta() { # dir target variant modules-json
  printf '{"version":"%s","revision":"%s","target":"%s","variant":"%s","modules":%s}\n' \
    "$VERSION" "$REV" "$2" "$3" "$4" > "$1/runtime.json"
}

# The programs.
cargo build -q --profile ship -p argus-cli -p argus-editor
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
cargo build -q --profile ship -p argus-game --features rigid
cp "$SHIP/argus-game$EXE" "$OUT/runtime/$OS/standard/"
meta "$OUT/runtime/$OS/standard" "$OS" standard '["rigid"]'

cargo build -q --profile ship -p argus-game --features rigid,steam
cp "$SHIP/argus-game$EXE" "$OUT/runtime/$OS/steam/"
# Steam's library, as steamworks-sys copied it out of Valve's SDK: the
# newest, when several builds left one.
LIB=$(ls -t "$SHIP"/build/steamworks-sys-*/out/"$STEAMLIB" 2>/dev/null | head -1)
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

# The manifest: every file with its size and SHA-256.
bash tools/release/manifest.sh "$OUT" "$VERSION" "$REV" "$OS"

echo "staged: $(grep -c '"path"' "$OUT/manifest.json") files, $(du -sh "$OUT" | cut -f1)"
"$OUT/bin/argus$EXE" version
