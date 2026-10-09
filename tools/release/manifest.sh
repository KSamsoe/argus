#!/usr/bin/env bash
# Write an install's manifest.json: every file with its size and SHA-256
# (distribution item 0), so an installer or a person can check it.
#
#   tools/release/manifest.sh <install dir> <version> <revision> <os>
#
# stage.sh writes one for each stage, and release.sh again once each
# stage carries the other platform's runtimes.
set -euo pipefail
DIR=${1:?usage: manifest.sh <dir> <version> <revision> <os>}
VERSION=${2:?}
REV=${3:?}
OS=${4:?}
(
  cd "$DIR"
  printf '{"version":"%s","revision":"%s","os":"%s","files":[' "$VERSION" "$REV" "$OS"
  first=1
  while IFS= read -r f; do
    f=${f#./}
    [ "$f" = manifest.json ] && continue
    size=$(wc -c < "$f" | tr -d ' ')
    sum=$(sha256sum "$f" | cut -d' ' -f1)
    [ $first = 1 ] || printf ','
    first=0
    printf '\n  {"path":"%s","bytes":%s,"sha256":"%s"}' "$f" "$size" "$sum"
  done < <(find . -type f | LC_ALL=C sort)
  printf '\n]}\n'
) > "$DIR.manifest.tmp"
mv "$DIR.manifest.tmp" "$DIR/manifest.json"
