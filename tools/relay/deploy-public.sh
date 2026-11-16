#!/usr/bin/env bash
# The relay anyone can reach, for Duel on https://argusengine.org/duel/:
#
#   tools/relay/deploy-public.sh [version]     (default: the latest release)
#
# On klab ($ARGUS_RELAY_HOST): the release's own Linux argus, checked
# against its SHA256SUMS, in ~/argus-relay-public; argus-relay-public.service
# (a user unit) running `argus relay local:7881`, on that machine only; and
# Tailscale Funnel publishing it with TLS on port 10000, so a page served
# over https reaches it as wss://klab.tailde2255.ts.net:10000. Port 443 is
# the site's old address (tools/web/publish-games.sh) and 8443 is taken on
# the tailnet. Then checks it from here with tools/relay/check.mjs.
# What a public relay exposes: docs/manual/releasing.md, "Running a relay".
set -euo pipefail
cd "$(dirname "$0")/../.."
HOST=${ARGUS_RELAY_HOST:-klab}
VERSION=${1:-}
if [ -z "$VERSION" ]; then
  VERSION=$(gh release view --repo KSamsoe/argus --json tagName --jq .tagName | sed 's/^v//')
fi
URL=https://github.com/KSamsoe/argus/releases/download/v$VERSION
echo "== argus $VERSION's relay on $HOST"
ssh -o BatchMode=yes "$HOST" bash -s -- "$URL" "$VERSION" <<'REMOTE'
set -euo pipefail
URL=$1
VERSION=$2
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
cd "$TMP"
curl -fsSL -o a.tgz "$URL/argus-$VERSION-linux-x86_64.tar.gz"
curl -fsSL -o SUMS "$URL/SHA256SUMS"
want=$(grep " argus-$VERSION-linux-x86_64.tar.gz\$" SUMS | cut -d' ' -f1)
[ "$want" = "$(sha256sum a.tgz | cut -d' ' -f1)" ] || { echo "the archive does not match its checksum" >&2; exit 1; }
tar -xzf a.tgz
mkdir -p ~/argus-relay-public ~/.config/systemd/user
cp "argus-$VERSION-linux/bin/argus" ~/argus-relay-public/argus.new
mv ~/argus-relay-public/argus.new ~/argus-relay-public/argus
REMOTE
scp -q -o BatchMode=yes tools/relay/argus-relay-public.service "$HOST:.config/systemd/user/argus-relay-public.service"
ssh -o BatchMode=yes "$HOST" 'systemctl --user daemon-reload && systemctl --user enable -q argus-relay-public.service && systemctl --user restart argus-relay-public.service && sleep 1 && systemctl --user is-active argus-relay-public.service && tailscale funnel --bg --https=10000 http://127.0.0.1:7881 > /dev/null'
echo "== the relay: wss://klab.tailde2255.ts.net:10000"
node tools/relay/check.mjs wss://klab.tailde2255.ts.net:10000
