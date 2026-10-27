#!/usr/bin/env bash
# Put an assembled site (assemble.sh) on https://argusengine.org/, through
# Cloudflare with wrangler.toml beside this:
#
#   tools/web/site/deploy.sh <site dir>
#
# The site is the front page, the page of the games, the games' web builds
# and the pictures, as tools/web/publish-games.sh makes it. Wrangler must be
# signed in to Kevin's Cloudflare account once (`npx wrangler login`, which
# asks him in his browser); it then deploys the whole folder, and Cloudflare
# keeps the domain's DNS record and HTTPS certificate itself.
set -euo pipefail
cd "$(dirname "$0")/../../.."
SITE=${1:?usage: tools/web/site/deploy.sh <site dir>}
for f in index.html live.js games/index.html hollow/data/packs.json img/og.png; do
  [ -f "$SITE/$f" ] || { echo "$SITE is not an assembled site: no $f" >&2; exit 1; }
done
npx --yes wrangler@4.153.0 deploy --config tools/web/site/wrangler.toml --assets "$SITE"
