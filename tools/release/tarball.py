"""The Linux and macOS release archives (distribution item 5), with their
modes set by path rather than read from the disk:

    python tools/release/tarball.py <stage dir> <out.tar.gz>

The Linux stage is built on Linux with its programs executable, but it is
put together on Windows, whose disk keeps no execute bits, so a tar made
there gave argus and argus-game mode 644 and install.sh refused the
archive (the first release's check). Here the programs, the libraries,
and the installer are 755, everything else 644, owned by no one, in a
fixed order, so the archive is the same wherever it is made.

The example-games archive goes through here too: each game is a folder
with its program named after it (`hollow/hollow`) and its libraries.
"""

import fnmatch
import gzip
import io
import os
import sys
import tarfile

# Paths under the stage that are run (or loaded) as programs.
EXECUTABLE = [
    "bin/*",
    "runtime/*/*/argus-game",
    "runtime/*/*/argus-game.exe",
    "*.so",
    "*.dylib",
    "install.sh",
    # A Mac app's programs (reaching players item 2): hollow/Hollow.app.
    "*.app/Contents/MacOS/*",
]


def mode(rel):
    parts = rel.split("/")
    game = len(parts) == 2 and parts[0] == parts[1]
    return 0o755 if game or any(fnmatch.fnmatch(rel, p) for p in EXECUTABLE) else 0o644


def main():
    stage, out = sys.argv[1], sys.argv[2]
    top = os.path.basename(os.path.normpath(stage))
    files = []
    for d, dirs, names in os.walk(stage):
        dirs.sort()
        for n in sorted(names):
            full = os.path.join(d, n)
            files.append((os.path.relpath(full, stage).replace(os.sep, "/"), full))
    tmp = out + ".tmp"
    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode="w", format=tarfile.PAX_FORMAT) as t:
        seen = set()
        for rel, full in files:
            # Each folder once, before what is in it.
            parts = rel.split("/")[:-1]
            for i in range(len(parts) + 1):
                folder = "/".join([top] + parts[:i])
                if folder not in seen:
                    seen.add(folder)
                    info = tarfile.TarInfo(folder)
                    info.type = tarfile.DIRTYPE
                    info.mode = 0o755
                    t.addfile(info)
            info = tarfile.TarInfo(f"{top}/{rel}")
            info.size = os.path.getsize(full)
            info.mode = mode(rel)
            info.mtime = int(os.path.getmtime(full))
            with open(full, "rb") as f:
                t.addfile(info, f)
    with open(tmp, "wb") as f:
        with gzip.GzipFile(fileobj=f, mode="wb", mtime=0) as g:
            g.write(raw.getvalue())
    os.replace(tmp, out)
    run = sum(1 for rel, _ in files if mode(rel) == 0o755)
    print(f"{out}: {len(files)} files, {run} executable, {os.path.getsize(out) // (1024 * 1024)} MB")


if __name__ == "__main__":
    main()
