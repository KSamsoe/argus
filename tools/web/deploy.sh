#!/usr/bin/env bash
# Put a built web page on the playtest server (klab):
#
#   tools/web/deploy.sh <site> [ssh host]      (host default: $ARGUS_PLAYTEST_HOST, else kevin@192.168.1.176)
#
# <site> is what `argus build --target web --release --out <site>` wrote. The
# page is copied beside the one being served and swapped in with one rename,
# so a player loading the page never gets half of each. The server
# (argus-play.service, docs/playtest.md) reads files per request and needs
# no restart; reports already sent stay in ~/argus-play/marks. When
# tools/web/serve.mjs differs from the server's copy, it is copied too and
# the service restarted (it began serving brotli in phase 5 item 0).
set -euo pipefail
SITE=${1:?usage: tools/web/deploy.sh <site dir> [ssh host]}
HOST=${2:-${ARGUS_PLAYTEST_HOST:-kevin@192.168.1.176}}
test -f "$SITE/index.html" && test -f "$SITE/data/packs.json" || {
  echo "$SITE is not a built page (no index.html or data/packs.json)" >&2
  exit 2
}
ssh -o BatchMode=yes "$HOST" 'rm -rf ~/argus-play/site.new'
scp -q -o BatchMode=yes -r "$SITE" "$HOST:argus-play/site.new"
ssh -o BatchMode=yes "$HOST" 'cd ~/argus-play && rm -rf site.old && mv site site.old && mv site.new site && rm -rf site.old && cat site/data/packs.json && echo'
SERVER="$(dirname "$0")/serve.mjs"
if [ "$(ssh -o BatchMode=yes "$HOST" 'sha256sum < ~/argus-play/serve.mjs')" != "$(sha256sum < "$SERVER")" ]; then
  scp -q -o BatchMode=yes "$SERVER" "$HOST:argus-play/serve.mjs.new"
  ssh -o BatchMode=yes "$HOST" 'cd ~/argus-play && mv serve.mjs.new serve.mjs && systemctl --user restart argus-play && echo "the server changed: copied and restarted"'
fi
