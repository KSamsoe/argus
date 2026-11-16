#!/usr/bin/env bash
# Put Argus's front page on a site that already has the games' web builds
# in it (hollow/, delve/, outpost/; tools/web/publish-games.sh):
#
#   tools/web/site/assemble.sh <site dir>
#
# The front page (tools/web/site/index.html) becomes the site's index and
# runs Hollow live from the site's own hollow/ build through live.js beside
# it. The page of the games (games.html, in the front page's style, its
# links one folder up) is games/; each game keeps its own address. img/
# gets the README's clips and stills that both pages show, and og.png, the
# picture a link to the site shows when shared.
set -euo pipefail
cd "$(dirname "$0")/../../.."
SITE=${1:?usage: tools/web/site/assemble.sh <site dir>}
[ -f "$SITE/hollow/data/packs.json" ] || { echo "$SITE has no hollow/ web build" >&2; exit 1; }
mkdir -p "$SITE/img" "$SITE/games"
cp tools/web/site/index.html tools/web/site/live.js "$SITE/"
cp tools/web/site/games.html "$SITE/games/index.html"
for g in hollow delve swarm outpost; do
  cp "docs/img/clip-$g.webp" "docs/img/game-$g.png" "$SITE/img/"
done
cp docs/img/templates.png docs/img/game-duel.png "$SITE/img/"
cp tools/web/site/og.png "$SITE/img/og.png"
cp tools/web/site/byogm.png "$SITE/img/byogm.png"
# The installers, at argusengine.org/install.ps1 and /install.sh, as text:
# each fetches the latest release from GitHub, so a copy here stays good.
cp tools/release/install.ps1 tools/release/install.sh "$SITE/"
printf '%s\n' "/install.ps1" "  Content-Type: text/plain; charset=utf-8" "/install.sh" "  Content-Type: text/plain; charset=utf-8" > "$SITE/_headers"
