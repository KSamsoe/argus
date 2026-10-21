#!/usr/bin/env bash
# The relay on klab, for matches over the tailnet (docs/multiplayer-checklist.md
# item 2):
#
#   tools/relay/deploy.sh
#
# Builds the last commit's argus there (in ~/argus-ci, from a git bundle, as
# CI's det step does), puts it in ~/argus-relay, and installs and restarts
# argus-relay.service (a user unit, as the games site's is). Then checks
# the relay from here with tools/relay/check.mjs. $ARGUS_RELAY_HOST is the
# host (default klab).
set -euo pipefail
cd "$(dirname "$0")/../.."
HOST=${ARGUS_RELAY_HOST:-klab}
SHA=$(git rev-parse HEAD)
mkdir -p target
git bundle create target/relay.bundle HEAD > /dev/null 2>&1
scp -q -o BatchMode=yes target/relay.bundle "$HOST:/tmp/argus-relay.bundle"
echo "== building $SHA on $HOST"
ssh -o BatchMode=yes "$HOST" bash -s -- "$SHA" <<'REMOTE'
set -euo pipefail
SHA=$1
export PATH="$HOME/.cargo/bin:$PATH"
[ -d ~/argus-ci/.git ] || git init -q ~/argus-ci
cd ~/argus-ci
git fetch -q /tmp/argus-relay.bundle HEAD
git checkout -q --force "$SHA"
cargo build -q --release -p argus-cli
mkdir -p ~/argus-relay ~/.config/systemd/user
cp target/release/argus ~/argus-relay/argus.new
mv ~/argus-relay/argus.new ~/argus-relay/argus
REMOTE
scp -q -o BatchMode=yes tools/relay/argus-relay.service "$HOST:.config/systemd/user/argus-relay.service"
ssh -o BatchMode=yes "$HOST" 'systemctl --user daemon-reload && systemctl --user enable -q argus-relay.service && systemctl --user restart argus-relay.service && sleep 1 && systemctl --user is-active argus-relay.service'
IP=$(ssh -o BatchMode=yes "$HOST" 'tailscale ip -4 | head -1')
echo "== the relay: ws://$IP:7879"
node tools/relay/check.mjs "ws://$IP:7879"
