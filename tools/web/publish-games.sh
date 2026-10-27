#!/usr/bin/env bash
# Put the example games on their public web site (klab, through Tailscale
# Funnel; docs/playtest.md, "The games in a browser"):
#
#   tools/web/publish-games.sh <argus> [ssh host]
#
# <argus> is an installed argus (an install's, so each page is what a
# release makes; the checkout's builds too, from source). Hollow, Delve,
# and Outpost are built for the web from copies of examples/, each checked
# by its build; Swarm has no web build (its chase is a Rust plugin). The
# site is Argus's front page (tools/web/site/, which runs Hollow live), the
# games' pages, the page of the games at games/
# (tools/web/games/index.html), and the README's clips and stills
# (tools/web/site/assemble.sh); it is copied beside the one being served
# and swapped in with one rename. The read-only server (argus-games.service, serve.mjs
# --read-only on 127.0.0.1:8766) is installed and started when it is not
# there, and restarted when serve.mjs changed. The host defaults to
# $ARGUS_PLAYTEST_HOST, else kevin@100.70.114.55; $ARGUS_SSH_OPTS are added
# to ssh and scp.
set -euo pipefail
cd "$(dirname "$0")/../.."
ARGUS=${1:?usage: tools/web/publish-games.sh <argus> [ssh host]}
HOST=${2:-${ARGUS_PLAYTEST_HOST:-kevin@100.70.114.55}}
read -r -a SSH_OPTS <<< "${ARGUS_SSH_OPTS:-}"
ssh_() { ssh -o BatchMode=yes "${SSH_OPTS[@]}" "$HOST" "$@"; }
scp_() { scp -q -o BatchMode=yes "${SSH_OPTS[@]}" "$@"; }

WORK=${TMPDIR:-/tmp}/argus-games-$$
rm -rf "${WORK:?}"
mkdir -p "$WORK/src" "$WORK/site/img"
trap 'rm -rf "${WORK:?}"' EXIT
echo "== the pages ($("$ARGUS" version))"
for g in hollow delve outpost; do
  cp -r "examples/$g" "$WORK/src/$g"
  rm -rf "${WORK:?}/src/$g/.argus" "${WORK:?}/src/$g/dist"
  "$ARGUS" --project "$WORK/src/$g" build --target web --release --out "$WORK/site/$g" | tail -1
  # The build's report names this machine's folders; the site does without.
  rm -f "$WORK/site/$g/build.json"
  # The site takes no reports, so the page does not offer F8's.
  sed -i 's|<span id="f8">[^<]*</span>|<span id="f8"></span>|' "$WORK/site/$g/index.html"
done
tools/web/site/assemble.sh "$WORK/site"

echo "== on $HOST"
ssh_ 'mkdir -p ~/argus-games ~/.config/systemd/user && rm -rf ~/argus-games/site.new'
scp_ -r "$WORK/site" "$HOST:argus-games/site.new"
ssh_ 'cd ~/argus-games && rm -rf site.old && { [ -d site ] && mv site site.old || true; } && mv site.new site && rm -rf site.old && ls site'
# The server: copied when it differs (the hashes alone; see deploy.sh),
# then the service installed, or restarted for a new server.
SERVER=tools/web/serve.mjs
UNIT=tools/web/argus-games.service
changed=0
if [ "$(ssh_ 'sha256sum < ~/argus-games/serve.mjs 2>/dev/null' | cut -d' ' -f1)" != "$(sha256sum < "$SERVER" | cut -d' ' -f1)" ]; then
  scp_ "$SERVER" "$HOST:argus-games/serve.mjs"
  changed=1
fi
if [ "$(ssh_ 'sha256sum < ~/.config/systemd/user/argus-games.service 2>/dev/null' | cut -d' ' -f1)" != "$(sha256sum < "$UNIT" | cut -d' ' -f1)" ]; then
  scp_ "$UNIT" "$HOST:.config/systemd/user/argus-games.service"
  ssh_ 'systemctl --user daemon-reload && systemctl --user enable argus-games.service'
  changed=1
fi
if [ "$changed" = 1 ]; then
  ssh_ 'systemctl --user restart argus-games.service && sleep 1 && systemctl --user is-active argus-games.service'
fi
ssh_ 'curl -s -o /dev/null -w "the site answers: %{http_code}\n" http://127.0.0.1:8766/ && tailscale funnel status 2>&1 | head -6'
