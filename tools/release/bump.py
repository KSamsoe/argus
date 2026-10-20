"""Move Argus to a new version, the first step of a release:

    python tools/release/bump.py <new version> [--no-clean]

Every crate's version and every path pin between them, the plugin SDK's own
version and Swarm's plugin lock (Swarm's plugin keeps its own version), the
README's two sentences that name the version, and the Mac testing packet,
which names the release a tester downloads. Bytes in and bytes out, so
every file keeps its line endings. cargo updates Cargo.lock at the next
build.

Then, unless --no-clean: `cargo clean -p` for every Argus crate, debug,
release, and wasm. A new version gives every crate a new build hash, so
each test program would be built again beside its old copy; the 0.1.1
release's CI filled the disk that way (110 GB of old test programs). The
third-party crates stay built, so this costs no extra build time.
"""
import pathlib
import re
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]


def current():
    text = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    m = re.search(r'(?m)^version = "([^"]+)"', text)
    if not m:
        sys.exit("bump.py: no version in Cargo.toml")
    return m.group(1)


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    clean = "--no-clean" not in sys.argv
    if len(args) != 1 or not re.fullmatch(r"\d+\.\d+\.\d+", args[0]):
        sys.exit(__doc__)
    old, new = current(), args[0]
    if old == new:
        sys.exit(f"bump.py: already {new}")
    o, n = old.encode(), new.encode()
    changed = 0

    def write(p, b, count):
        nonlocal changed
        p.write_bytes(b)
        changed += count
        print(f"{p.relative_to(ROOT)}: {count}")

    # Path pins: a line with `path = "..."` and `version = "<old>"`, either order.
    pins = [
        re.compile(rb'^(.*path = "[^"]*".*version = ")' + re.escape(o) + rb'(".*)$', re.M),
        re.compile(rb'^(.*version = ")' + re.escape(o) + rb'(".*path = "[^"]*".*)$', re.M),
    ]
    for p in [ROOT / "Cargo.toml"] + sorted((ROOT / "crates").glob("*/Cargo.toml")):
        b = p.read_bytes()
        k = 0
        for pin in pins:
            b, c = pin.subn(rb"\g<1>" + n + rb"\g<2>", b)
            k += c
        # The package's own version: the workspace's, and the plugin SDK's.
        if p == ROOT / "Cargo.toml" or p.parent.name == "argus-plugin":
            b, c = re.subn(rb'(?m)^version = "' + re.escape(o) + rb'"', b'version = "' + n + b'"', b, count=1)
            k += c
        if k:
            write(p, b, k)

    # Swarm's plugin lock names the SDK's version.
    p = ROOT / "examples" / "swarm" / "plugins" / "swarm-hot" / "Cargo.lock"
    b = p.read_bytes()
    b2, k = re.subn(rb'(name = "argus-plugin"\r?\nversion = ")' + re.escape(o) + rb'"', rb"\g<1>" + n + b'"', b)
    if k:
        write(p, b2, k)
    else:
        print(f"note: {p.relative_to(ROOT)} does not name argus-plugin {old}")

    # The README's sentences that say which version Argus is.
    p = ROOT / "README.md"
    b = p.read_bytes()
    k = 0
    for sentence in [rb"Argus is young, at version ", rb"Argus is young: version "]:
        b, c = re.subn(re.escape(sentence) + re.escape(o) + rb"\b", sentence + n, b)
        k += c
    if k:
        write(p, b, k)
    else:
        print("note: README.md names no version to move; check it by hand")

    # The Mac testing packet names the release under test.
    p = ROOT / "docs" / "mac-testing.md"
    b = p.read_bytes()
    if b.count(o):
        write(p, b.replace(o, n), b.count(o))

    print(f"{old} -> {new}: {changed} changes. Next: docs/releases/{new}.md, history.md, the full CI.")

    if clean:
        cargo = shutil.which("cargo") or str(pathlib.Path.home() / ".cargo" / "bin" / "cargo")
        names = []
        for t in sorted((ROOT / "crates").glob("*/Cargo.toml")):
            m = re.search(r'(?m)^name = "(argus[^"]*)"', t.read_text(encoding="utf-8"))
            if m:
                names.append(m.group(1))
        packages = [x for name in names for x in ("-p", name)]
        for extra in ([], ["--release"], ["--release", "--target", "wasm32-unknown-unknown"]):
            r = subprocess.run([cargo, "clean", *extra, *packages], cwd=ROOT, capture_output=True, text=True)
            lines = (r.stderr or r.stdout).strip().splitlines()
            print(f"cargo clean {' '.join(extra) or '(debug)'}: {lines[-1] if lines else r.returncode}")


if __name__ == "__main__":
    main()
