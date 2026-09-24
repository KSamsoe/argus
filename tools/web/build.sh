#!/usr/bin/env bash
# Build the web player and a site that plays one project:
#
#   tools/web/build.sh [project] [site]      (defaults: examples/hollow, target-web/site)
#
# Needs wasi-sdk (see env.sh), the wasm32-unknown-unknown Rust target, and
# wasm-bindgen-cli at the wasm-bindgen version in Cargo.lock. Serve the site
# with `node tools/web/serve.mjs [site]`.
set -euo pipefail
cd "$(dirname "$0")/../.."
PROJECT=${1:-examples/hollow}
SITE=${2:-target-web/site}
. tools/web/env.sh
# The code a mark from this build says it was made with (§6 run identity).
ARGUS_CODE="git:$(git rev-parse --short HEAD)$(git diff --quiet HEAD -- crates "$PROJECT" || echo +dirty)"
export ARGUS_CODE
# A target directory of its own, so a web build never waits on a native one.
CARGO_TARGET_DIR=target-web cargo rustc -q -p argus-web --release \
  --target wasm32-unknown-unknown --crate-type cdylib
mkdir -p "$SITE"
wasm-bindgen --target web --out-dir "$SITE/pkg" \
  target-web/wasm32-unknown-unknown/release/argus_web.wasm
cargo run -q -p argus-web --example pack -- "$PROJECT" "$SITE/game.pack"
cp tools/web/index.html tools/web/main.js "$SITE/"
ls -la "$SITE" "$SITE/pkg" | grep -E "wasm|pack|html|js$"
