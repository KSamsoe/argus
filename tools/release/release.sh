#!/usr/bin/env bash
# Make a release of Argus (distribution item 5), from this Windows machine
# and a Linux one over ssh:
#
#   tools/release/release.sh [--host user@linux] [--publish]
#
# 1. Here: stage the Windows install with the web runtime (stage.sh),
#    check that it releases every example (check.sh).
# 2. On the Linux host: the commit, sent as a git bundle into
#    ~/argus-release (no GitHub access needed there), staged without the
#    web runtime and checked the same way; the stage comes back here.
# 3. Each install gets the other platform's runtimes and the web one, so
#    either can release a game for both (item 1); manifests are rewritten.
# 4. In target/release/v<version>/: the Windows zip, the Linux tar.gz, the
#    Claude Desktop bundle (mcpb.py), the two installers (a first install
#    starts from one), and SHA256SUMS.
# 5. With --publish: a draft release v<version> on the private repository
#    (gh), with those files. Publishing the draft is a person's click.
#
# The tree must be clean: a release is a commit. The Linux host defaults
# to $ARGUS_RELEASE_HOST; $ARGUS_SSH_OPTS are added to ssh and scp (this
# machine reaches klab over the tailnet with a HostKeyAlias).
set -euo pipefail
cd "$(dirname "$0")/../.."

HOST=${ARGUS_RELEASE_HOST:-kevin@100.70.114.55}
PUBLISH=0
while [ $# -gt 0 ]; do
  case "$1" in
    --host) HOST=$2; shift 2 ;;
    --publish) PUBLISH=1; shift ;;
    *) echo "release.sh: unknown argument $1" >&2; exit 2 ;;
  esac
done
read -r -a SSH_OPTS <<< "${ARGUS_SSH_OPTS:-}"
ssh_() { ssh -o BatchMode=yes "${SSH_OPTS[@]}" "$HOST" "$@"; }
scp_() { scp -q -o BatchMode=yes "${SSH_OPTS[@]}" "$@"; }

if [ -n "$(git status --porcelain --untracked-files=no)" ]; then
  echo "release.sh: the tree has uncommitted changes; a release is a commit" >&2
  exit 1
fi
VERSION=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)
REV=$(git rev-parse --short=12 HEAD)
SHA=$(git rev-parse HEAD)
OUT=target/release/v$VERSION
WIN=target/stage/argus-$VERSION-windows
LIN=target/stage/argus-$VERSION-linux
echo "== release Argus $VERSION ($REV)"
rm -rf "${OUT:?}" "${LIN:?}"
mkdir -p "$OUT"

echo "== Windows: stage and check"
bash tools/release/stage.sh "$WIN"
bash tools/release/check.sh "$WIN"

echo "== Linux ($HOST): the commit, staged and checked there"
git bundle create "target/release/argus-$REV.bundle" HEAD > /dev/null 2>&1
scp_ "target/release/argus-$REV.bundle" "$HOST:/tmp/argus-release.bundle"
ssh_ bash -s -- "$SHA" "$VERSION" <<'REMOTE'
set -euo pipefail
SHA=$1; VERSION=$2
export PATH="$HOME/.cargo/bin:$PATH"
[ -d ~/argus-release/.git ] || git init -q ~/argus-release
cd ~/argus-release
git fetch -q /tmp/argus-release.bundle HEAD
git checkout -q --force "$SHA"
git clean -qfd -e target
bash tools/release/stage.sh "target/stage/argus-$VERSION-linux" --no-web
bash tools/release/check.sh "target/stage/argus-$VERSION-linux" --no-web
tar -C target/stage -czf /tmp/argus-release-linux.tar.gz "argus-$VERSION-linux"
REMOTE
scp_ "$HOST:/tmp/argus-release-linux.tar.gz" "target/release/linux-stage.tar.gz"
tar --force-local -C target/stage -xzf target/release/linux-stage.tar.gz

echo "== each install with both platforms' runtimes"
cp -r "$WIN/runtime/web" "$LIN/runtime/"
cp -r "$LIN/runtime/linux" "$WIN/runtime/"
cp -r "$WIN/runtime/windows" "$LIN/runtime/"
bash tools/release/manifest.sh "$WIN" "$VERSION" "$REV" windows
bash tools/release/manifest.sh "$LIN" "$VERSION" "$REV" linux

echo "== archives"
python - "$WIN" "$OUT/argus-$VERSION-windows-x86_64.zip" <<'PY'
import os, sys, zipfile
src, out = sys.argv[1], sys.argv[2]
top = os.path.basename(src)
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
    for d, _, fs in os.walk(src):
        for f in sorted(fs):
            full = os.path.join(d, f)
            z.write(full, os.path.join(top, os.path.relpath(full, src)).replace(os.sep, "/"))
PY
# tar keeps the Linux programs' execute bits, which a zip from here loses.
tar --force-local -C target/stage -czf "$OUT/argus-$VERSION-linux-x86_64.tar.gz" "argus-$VERSION-linux"
python tools/release/mcpb.py "$WIN" "$OUT/argus-$VERSION.mcpb"
cp tools/release/install.ps1 tools/release/install.sh "$OUT/"
( cd "$OUT" && sha256sum argus-* install.* | sed 's/ \*/  /' > SHA256SUMS )
cat "$OUT/SHA256SUMS"
ls -la "$OUT"

if [ "$PUBLISH" = 1 ]; then
  echo "== a draft release on GitHub"
  gh release create "v$VERSION" "$OUT"/argus-* "$OUT"/install.* "$OUT/SHA256SUMS" \
    --draft --target "$SHA" --title "Argus $VERSION" \
    --notes "Argus $VERSION ($REV). Install: install.ps1 (Windows) or install.sh (Linux) from the archive, or with a GitHub token from this release; see docs/install.md."
fi
echo "== done: $OUT"
