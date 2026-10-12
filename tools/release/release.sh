#!/usr/bin/env bash
# Make a release of Argus (distribution item 5), from this Windows machine
# and a Linux one over ssh, which also cross-builds the Mac's:
#
#   tools/release/release.sh [--host user@linux] [--publish]
#
# 1. Here: stage the Windows install with the web runtime (stage.sh),
#    check that it releases every example (check.sh).
# 2. On the Linux host: the commit, sent as a git bundle into
#    ~/argus-release (no GitHub access needed there), staged without the
#    web runtime and checked the same way; then the macOS install,
#    cross-built there (stage.sh --target macos, docs/macos-checklist.md),
#    which nothing here can run. Both stages come back here.
# 3. Each install gets the other platforms' runtimes and the web one, so
#    any of them can release a game for all three (item 1); manifests are
#    rewritten. The Windows install then releases the examples for the
#    Mac (check.sh --for macos), which checks the Mac runtime's packs.
# 4. In target/release/v<version>/: the Windows zip, the Linux and macOS
#    tar.gz, the Claude Desktop bundles for Windows and the Mac (mcpb.py),
#    the two installers (a first install starts from one), the example
#    games released for each platform by the checks (example-games-*, to
#    play with nothing installed; not named argus-*, which the installers
#    look for), and SHA256SUMS.
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

# Files checked out a moment ago can look changed until git looks again.
git update-index -q --refresh > /dev/null 2>&1 || true
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
MAC=target/stage/argus-$VERSION-macos
GWIN=target/stage/example-games-$VERSION-windows
GLIN=target/stage/example-games-$VERSION-linux
GMAC=target/stage/example-games-$VERSION-macos
echo "== release Argus $VERSION ($REV)"
rm -rf "${OUT:?}" "${LIN:?}" "${GLIN:?}" "${MAC:?}" "${GMAC:?}"
mkdir -p "$OUT"

echo "== Windows: stage and check"
bash tools/release/stage.sh "$WIN"
bash tools/release/check.sh "$WIN" --keep "$GWIN"

echo "== Linux ($HOST): the commit, staged and checked there, and the Mac's cross-built"
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
bash tools/release/check.sh "target/stage/argus-$VERSION-linux" --no-web \
  --keep "target/stage/example-games-$VERSION-linux"
bash tools/release/stage.sh "target/stage/argus-$VERSION-macos" --target macos
tar -C target/stage -czf /tmp/argus-release-linux.tar.gz \
  "argus-$VERSION-linux" "example-games-$VERSION-linux" "argus-$VERSION-macos"
REMOTE
scp_ "$HOST:/tmp/argus-release-linux.tar.gz" "target/release/linux-stage.tar.gz"
tar --force-local -C target/stage -xzf target/release/linux-stage.tar.gz

echo "== each install with every platform's runtimes"
cp -r "$WIN/runtime/web" "$LIN/runtime/"
cp -r "$WIN/runtime/web" "$MAC/runtime/"
cp -r "$LIN/runtime/linux" "$WIN/runtime/"
cp -r "$LIN/runtime/linux" "$MAC/runtime/"
cp -r "$WIN/runtime/windows" "$LIN/runtime/"
cp -r "$WIN/runtime/windows" "$MAC/runtime/"
cp -r "$MAC/runtime/macos" "$WIN/runtime/"
cp -r "$MAC/runtime/macos" "$LIN/runtime/"
bash tools/release/manifest.sh "$WIN" "$VERSION" "$REV" windows
bash tools/release/manifest.sh "$LIN" "$VERSION" "$REV" linux
bash tools/release/manifest.sh "$MAC" "$VERSION" "$REV" macos

echo "== the examples released for the Mac, from the Windows install"
bash tools/release/check.sh "$WIN" --for macos --keep "$GMAC"

echo "== archives"
cp tools/release/games-readme.txt "$GWIN/README.txt"
cp tools/release/games-readme.txt "$GLIN/README.txt"
cp tools/release/games-readme-macos.txt "$GMAC/README.txt"
zipdir() { # src out
python - "$1" "$2" <<'PY'
import os, sys, zipfile
src, out = sys.argv[1], sys.argv[2]
top = os.path.basename(src)
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
    for d, _, fs in os.walk(src):
        for f in sorted(fs):
            full = os.path.join(d, f)
            z.write(full, os.path.join(top, os.path.relpath(full, src)).replace(os.sep, "/"))
PY
}
zipdir "$WIN" "$OUT/argus-$VERSION-windows-x86_64.zip"
zipdir "$GWIN" "$OUT/example-games-$VERSION-windows-x86_64.zip"
# The modes by path (tarball.py): this disk keeps no execute bits.
python tools/release/tarball.py "$LIN" "$OUT/argus-$VERSION-linux-x86_64.tar.gz"
python tools/release/tarball.py "$GLIN" "$OUT/example-games-$VERSION-linux-x86_64.tar.gz"
python tools/release/tarball.py "$MAC" "$OUT/argus-$VERSION-macos-universal.tar.gz"
python tools/release/tarball.py "$GMAC" "$OUT/example-games-$VERSION-macos-universal.tar.gz"
python tools/release/mcpb.py "$WIN" "$OUT/argus-$VERSION.mcpb"
python tools/release/mcpb.py "$MAC" "$OUT/argus-$VERSION-macos.mcpb" --tools-from "$WIN/bin/argus.exe"
cp tools/release/install.ps1 tools/release/install.sh "$OUT/"
( cd "$OUT" && sha256sum argus-* example-games-* install.* | sed 's/ \*/  /' > SHA256SUMS )
cat "$OUT/SHA256SUMS"
ls -la "$OUT"

if [ "$PUBLISH" = 1 ]; then
  echo "== a draft release on GitHub"
  gh release create "v$VERSION" "$OUT"/argus-* "$OUT"/example-games-* "$OUT"/install.* "$OUT/SHA256SUMS" \
    --draft --target "$SHA" --title "Argus $VERSION" \
    --notes "Argus $VERSION ($REV). Install: install.ps1 (Windows) or install.sh (Linux and macOS) from the archive, or with a GitHub token from this release; see docs/install.md. The macOS build is cross-built, universal (Apple Silicon and Intel), and new: see docs/macos-checklist.md. To play the example games with nothing installed: example-games-$VERSION for Windows, Linux, or macOS (its README.txt has the controls)."
fi
echo "== done: $OUT"
