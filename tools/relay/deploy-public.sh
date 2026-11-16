#!/usr/bin/env bash
# The relay anyone can reach, for Duel on https://argusengine.org/duel/:
#
#   tools/relay/deploy-public.sh [version]     (default: the latest release)
#
# On klab ($ARGUS_RELAY_HOST), in ~/argus-relay-public:
#   - the release's own Linux argus, checked against its SHA256SUMS, run by
#     argus-relay-public.service (a user unit) as `argus relay local:7881`,
#     on that machine only;
#   - Cloudflare's cloudflared ($CLOUDFLARED, checked against the hash its
#     release notes give), run by argus-relay-tunnel.service, which puts the
#     relay on the internet as wss://relay.argusengine.org through the
#     Cloudflare Tunnel `argus-relay` (created here once, with its DNS
#     record). Creating it needs cloudflared signed in to the Cloudflare
#     account that holds argusengine.org, once, on klab:
#     ~/argus-relay-public/cloudflared tunnel login (it prints a link to open).
# Then checks the relay from here with tools/relay/check.mjs.
# What a public relay exposes: docs/manual/releasing.md, "Running a relay".
set -euo pipefail
cd "$(dirname "$0")/../.."
HOST=${ARGUS_RELAY_HOST:-klab}
CLOUDFLARED=2026.11.0
CLOUDFLARED_SHA=51f7c68b57ed7269164075d7e789efce5b59355982f06392deeb037d982a51fd
NAME=relay.argusengine.org
VERSION=${1:-}
if [ -z "$VERSION" ]; then
  VERSION=$(gh release view --repo KSamsoe/argus --json tagName --jq .tagName | sed 's/^v//')
fi
URL=https://github.com/KSamsoe/argus/releases/download/v$VERSION
echo "== argus $VERSION's relay on $HOST, as $NAME"
ssh -o BatchMode=yes "$HOST" bash -s -- "$URL" "$VERSION" "$CLOUDFLARED" "$CLOUDFLARED_SHA" "$NAME" <<'REMOTE'
set -euo pipefail
URL=$1
VERSION=$2
CF=$3
CF_SHA=$4
NAME=$5
DIR=~/argus-relay-public
mkdir -p "$DIR" ~/.config/systemd/user
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
cd "$TMP"
# The relay: the release's own argus.
curl -fsSL -o a.tgz "$URL/argus-$VERSION-linux-x86_64.tar.gz"
curl -fsSL -o SUMS "$URL/SHA256SUMS"
want=$(grep " argus-$VERSION-linux-x86_64.tar.gz\$" SUMS | cut -d' ' -f1)
[ "$want" = "$(sha256sum a.tgz | cut -d' ' -f1)" ] || { echo "the archive does not match its checksum" >&2; exit 1; }
tar -xzf a.tgz
cp "argus-$VERSION-linux/bin/argus" "$DIR/argus.new"
mv "$DIR/argus.new" "$DIR/argus"
# The tunnel: Cloudflare's cloudflared, when it is not this version yet.
if ! "$DIR/cloudflared" --version 2>/dev/null | grep -q "version $CF "; then
  curl -fsSL -o cf "https://github.com/cloudflare/cloudflared/releases/download/$CF/cloudflared-linux-amd64"
  echo "$CF_SHA  cf" | sha256sum -c --quiet
  chmod +x cf
  mv cf "$DIR/cloudflared"
fi
[ -f ~/.cloudflared/cert.pem ] || { echo "cloudflared is not signed in: run $DIR/cloudflared tunnel login on this machine and open its link" >&2; exit 1; }
"$DIR/cloudflared" tunnel info argus-relay > /dev/null 2>&1 || "$DIR/cloudflared" tunnel create argus-relay
ID=$("$DIR/cloudflared" tunnel list --name argus-relay --output json | python3 -c 'import json,sys; print(json.load(sys.stdin)[0]["id"])')
"$DIR/cloudflared" tunnel route dns argus-relay "$NAME"
cat > "$DIR/tunnel.yml" <<YML
tunnel: $ID
credentials-file: $HOME/.cloudflared/$ID.json
ingress:
  - hostname: $NAME
    service: http://127.0.0.1:7881
  - service: http_status:404
YML
REMOTE
scp -q -o BatchMode=yes tools/relay/argus-relay-public.service tools/relay/argus-relay-tunnel.service "$HOST:.config/systemd/user/"
ssh -o BatchMode=yes "$HOST" 'systemctl --user daemon-reload && systemctl --user enable -q argus-relay-public.service argus-relay-tunnel.service && systemctl --user restart argus-relay-public.service argus-relay-tunnel.service && sleep 3 && systemctl --user is-active argus-relay-public.service argus-relay-tunnel.service'
echo "== the relay: wss://$NAME"
for i in 1 2 3 4 5 6; do
  node tools/relay/check.mjs "wss://$NAME" && exit 0
  sleep 10
done
exit 1
