#!/usr/bin/env bash
# Build the web page for one project:
#
#   tools/web/build.sh [project] [site]      (defaults: examples/hollow, target-web/site)
#
# This is `argus build --target web --release` (phase 2 item 7), which
# makes the page, the wasm module, and the project's release packs, and
# checks that the packs play as the project does. It needs wasi-sdk
# (WASI_SDK; see env.sh for why), the wasm32-unknown-unknown Rust target,
# and wasm-bindgen-cli at the wasm-bindgen version in Cargo.lock. Serve the
# site with `node tools/web/serve.mjs [site]`.
set -euo pipefail
cd "$(dirname "$0")/../.."
PROJECT=${1:-examples/hollow}
SITE=${2:-target-web/site}
cargo run -q -p argus-cli -- --project "$PROJECT" build --target web --release --out "$SITE"
