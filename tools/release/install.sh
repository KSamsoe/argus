#!/usr/bin/env sh
# Install Argus for this user (distribution item 6), from a release on the
# private GitHub repository, or from an archive already here:
#
#   sh install.sh [--version 0.0.1] [--archive <tar.gz>] [--repo owner/name] [--uninstall]
#
# A release needs a GitHub token that can read the repository: $GITHUB_TOKEN,
# else `gh auth token` (the GitHub CLI, signed in). The archive's checksum is
# checked against the release's SHA256SUMS before anything is put in place.
#
# Argus goes in ~/.local/share/argus/app (bin/, runtime/, templates/), with
# links to its programs in ~/.local/bin. The version before stays as
# app.old until the new one has started (`argus version`), then is removed;
# if the new one does not start, the old one is put back. Nothing needs
# root. --uninstall takes it all out again, except the projects and Argus's
# own data (~/.local/share/Argus).
set -eu

VERSION=latest
ARCHIVE=
REPO=KSamsoe/argus
INTO=${XDG_DATA_HOME:-$HOME/.local/share}/argus
LINKS=$HOME/.local/bin
UNINSTALL=0
WAITFOR=0
while [ $# -gt 0 ]; do
  case "$1" in
    --version) VERSION=$2; shift 2 ;;
    --archive) ARCHIVE=$2; shift 2 ;;
    --repo) REPO=$2; shift 2 ;;
    --into) INTO=$2; shift 2 ;;
    --links) LINKS=$2; shift 2 ;;
    --uninstall) UNINSTALL=1; shift ;;
    --wait-for) WAITFOR=$2; shift 2 ;;
    *) echo "install.sh: unknown argument $1" >&2; exit 2 ;;
  esac
done
APP=$INTO/app
say() { echo "argus install: $*"; }

if [ "$WAITFOR" -gt 0 ]; then
  n=0
  while kill -0 "$WAITFOR" 2>/dev/null && [ $n -lt 600 ]; do sleep 0.1; n=$((n + 1)); done
fi

stop_daemons() {
  # Daemons run from the install; stop them so its files can go.
  for p in /proc/[0-9]*; do
    exe=$(readlink "$p/exe" 2>/dev/null || true)
    case "$exe" in "$1"/*) kill "${p#/proc/}" 2>/dev/null || true ;; esac
  done
}

if [ "$UNINSTALL" = 1 ]; then
  [ -d "$APP" ] && stop_daemons "$APP"
  rm -rf "${INTO:?}/app" "${INTO:?}/app.old" "${INTO:?}/app.new" "${INTO:?}/downloads"
  for b in argus argus-editor; do
    [ -L "$LINKS/$b" ] && rm -f "$LINKS/$b"
  done
  say "removed $APP (your projects and ~/.local/share/Argus are kept)"
  exit 0
fi

mkdir -p "$INTO"
if [ -z "$ARCHIVE" ]; then
  TOKEN=${GITHUB_TOKEN:-}
  if [ -z "$TOKEN" ] && command -v gh > /dev/null 2>&1; then
    TOKEN=$(gh auth token 2>/dev/null || true)
  fi
  [ -n "$TOKEN" ] || { echo "install.sh: a release on $REPO needs a GitHub token: set GITHUB_TOKEN, or sign in with gh auth login" >&2; exit 1; }
  if [ "$VERSION" = latest ]; then REL=latest; else REL=tags/v$VERSION; fi
  api() { curl -fsSL -H "Authorization: Bearer $TOKEN" -H "X-GitHub-Api-Version: 2022-11-28" "$@"; }
  JSON=$(api "https://api.github.com/repos/$REPO/releases/$REL")
  # The asset's API url, the line after its name in the release's JSON.
  asset_url() {
    printf '%s\n' "$JSON" | tr ',' '\n' | awk -v want="$1" '
      /"url":/ { url = $0 }
      /"name":/ { if (index($0, want)) { sub(/.*"url": *"/, "", url); sub(/".*/, "", url); print url; exit } }'
  }
  NAME=$(printf '%s\n' "$JSON" | tr ',' '\n' | grep -o '"name": *"argus-[^"]*-linux-x86_64.tar.gz"' | head -1 | sed 's/.*"\(argus-[^"]*\)"/\1/')
  [ -n "$NAME" ] || { echo "install.sh: the release has no Linux archive" >&2; exit 1; }
  URL=$(asset_url "\"$NAME\"")
  SUMS=$(asset_url '"SHA256SUMS"')
  [ -n "$URL" ] && [ -n "$SUMS" ] || { echo "install.sh: the release lacks $NAME or SHA256SUMS" >&2; exit 1; }
  mkdir -p "$INTO/downloads"
  ARCHIVE=$INTO/downloads/$NAME
  say "downloading $NAME"
  api -H "Accept: application/octet-stream" -o "$ARCHIVE" "$URL"
  api -H "Accept: application/octet-stream" -o "$INTO/downloads/SHA256SUMS" "$SUMS"
  want=$(grep " $NAME\$" "$INTO/downloads/SHA256SUMS" | cut -d' ' -f1)
  have=$(sha256sum "$ARCHIVE" | cut -d' ' -f1)
  if [ -z "$want" ] || [ "$want" != "$have" ]; then
    rm -f "$ARCHIVE"
    echo "install.sh: $NAME does not match its checksum (expected $want, got $have); nothing was installed" >&2
    exit 1
  fi
fi

NEW=$INTO/app.new
OLD=$INTO/app.old
rm -rf "$NEW" "$OLD" "$INTO/unpack"
mkdir -p "$INTO/unpack"
tar -xzf "$ARCHIVE" -C "$INTO/unpack"
TOP=$(find "$INTO/unpack" -mindepth 1 -maxdepth 1 -type d | head -1)
[ -x "$TOP/bin/argus" ] || { echo "install.sh: $ARCHIVE is not an Argus archive (no argus-*/bin/argus in it)" >&2; exit 1; }
mv "$TOP" "$NEW"
rm -rf "$INTO/unpack"
"$NEW/bin/argus" version > /dev/null || { echo "install.sh: the new argus does not start" >&2; exit 1; }

if [ -d "$APP" ]; then
  stop_daemons "$APP"
  mv "$APP" "$OLD"
fi
mv "$NEW" "$APP"
if ! OK=$("$APP/bin/argus" version); then
  rm -rf "$APP"
  [ -d "$OLD" ] && mv "$OLD" "$APP" && say "the new version did not start; the old one is back"
  exit 1
fi
rm -rf "$OLD"
mkdir -p "$LINKS"
for b in argus argus-editor; do
  ln -sf "$APP/bin/$b" "$LINKS/$b"
done
say "installed $OK in $APP"
case ":$PATH:" in
  *":$LINKS:"*) ;;
  *) say "add $LINKS to your PATH (most shells' profiles do when it exists; open a new terminal), then: argus new my-game" ;;
esac
