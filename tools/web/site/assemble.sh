#!/usr/bin/env bash
# Put Argus's front page on a site that already has the games' web builds
# in it (hollow/, delve/, outpost/; tools/web/publish-games.sh):
#
#   tools/web/site/assemble.sh <site dir>
#
# The front page (tools/web/site/index.html) becomes the site's index and
# runs Hollow live from the site's own hollow/ build through live.js beside
# it. The page of the games (tools/web/games/index.html) moves to games/,
# its links pointed one folder up, so each game keeps its address. img/
# gets the README's clips and stills that both pages show.
set -euo pipefail
cd "$(dirname "$0")/../../.."
SITE=${1:?usage: tools/web/site/assemble.sh <site dir>}
[ -f "$SITE/hollow/data/packs.json" ] || { echo "$SITE has no hollow/ web build" >&2; exit 1; }
mkdir -p "$SITE/img" "$SITE/games"
cp tools/web/site/index.html tools/web/site/live.js "$SITE/"
sed -e 's#href="\(hollow\|delve\|outpost\)/"#href="../\1/"#g' -e 's#src="img/#src="../img/#g' \
  tools/web/games/index.html > "$SITE/games/index.html"
for g in hollow delve swarm outpost; do
  cp "docs/img/clip-$g.webp" "docs/img/game-$g.png" "$SITE/img/"
done
cp docs/img/templates.png "$SITE/img/"
