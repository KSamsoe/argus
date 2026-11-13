"""The third-party notices every copy of Argus, and every game built with it,
carries: each crate it is built from, its license, and its license text.

    python tools/release/notices.py <out file>

Permissive licenses (MIT, BSD, Zlib, Apache) ask that a copy of the program
keep the dependency's copyright and license text. The crates are those in
Cargo.lock (`cargo metadata`), the vendored ones in vendor/ included. A
crate's own license files are copied as they are; for a crate that ships
none, the license its Cargo.toml names is written out from the standard text,
with its authors as the copyright holders. Identical texts are written once,
with every crate that uses them.
"""
import collections
import json
import os
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
NAMES = ("LICENSE", "LICENCE", "COPYING", "NOTICE", "UNLICENSE", "COPYRIGHT")

MIT = """Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE."""

BSD3 = """Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this
   list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.

3. Neither the name of the copyright holder nor the names of its contributors
   may be used to endorse or promote products derived from this software
   without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE."""

# The order a license is chosen in from an "A OR B" expression.
PREFER = ["MIT", "Apache-2.0", "Zlib", "BSD-3-Clause", "BSD-2-Clause", "ISC",
          "0BSD", "Unlicense", "CC0-1.0", "BSL-1.0", "Unicode-3.0"]


def choose(expr):
    """The license this copy takes from a crate's expression."""
    if not expr:
        return None
    expr = expr.replace("/", " OR ")
    alts = [a.strip("() ") for a in re.split(r"\s+OR\s+", expr)]
    simple = [a for a in alts if a in PREFER]
    if simple:
        return min(simple, key=PREFER.index)
    return expr


def main():
    out = pathlib.Path(sys.argv[1])
    meta = json.loads(subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked"],
        cwd=ROOT, capture_output=True, check=True).stdout.decode("utf-8"))
    members = set(meta["workspace_members"])
    vendor = (ROOT / "vendor").resolve()
    crates = []
    for p in meta["packages"]:
        dir_ = pathlib.Path(p["manifest_path"]).resolve().parent
        if p["id"] in members and vendor not in dir_.parents:
            continue
        if not p["source"] and vendor not in dir_.parents:
            continue
        crates.append((p, dir_))
    crates.sort(key=lambda c: (c[0]["name"], c[0]["version"]))

    texts = collections.OrderedDict()  # text -> [crate labels]
    listing = []
    for p, dir_ in crates:
        label = f'{p["name"]} {p["version"]}'
        files = []
        for root, _, names in os.walk(dir_):
            depth = len(pathlib.Path(root).relative_to(dir_).parts)
            if depth > 1:
                continue
            for n in sorted(names):
                if n.upper().startswith(NAMES):
                    files.append(pathlib.Path(root) / n)
        taken = choose(p.get("license"))
        listing.append(f'{label}  {p.get("license") or "(no license field)"}'
                       + (f'  {p["repository"]}' if p.get("repository") else ""))
        if files:
            for f in files:
                t = f.read_text(encoding="utf-8", errors="replace").strip()
                texts.setdefault(t, []).append(f"{label} ({f.relative_to(dir_).as_posix()})")
            continue
        who = ", ".join(re.sub(r"\s*<[^>]*>", "", a) for a in p.get("authors") or []) or f'the {p["name"]} authors'
        if taken == "MIT":
            t = f"MIT License\n\nCopyright (c) {who}\n\n{MIT}"
        elif taken == "BSD-3-Clause":
            t = f"BSD 3-Clause License\n\nCopyright (c) {who}\n\n{BSD3}"
        elif taken == "Apache-2.0":
            t = "Licensed under the Apache License, Version 2.0: the text is in ARGUS-LICENSE.txt beside this file."
        else:
            t = f"Licensed under {taken}, by {who}: https://spdx.org/licenses/{taken}.html"
        texts.setdefault(t, []).append(f"{label} (from its Cargo.toml: {p.get('license')})")

    lines = [
        "THIRD-PARTY NOTICES",
        "",
        "Argus, and every game built with it, includes the following software.",
        "Argus's own license is in ARGUS-LICENSE.txt, its credits in ARGUS-NOTICE.txt.",
        "Where a crate offers a choice of licenses, this copy takes MIT, else the",
        "first of Apache-2.0, Zlib, BSD, ISC that it offers.",
        "",
        f"{len(crates)} crates:",
        "",
        *listing,
        "",
    ]
    for t, who in texts.items():
        lines += ["=" * 78, *who, "-" * 78, t, ""]
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(("\n".join(lines) + "\n").encode("utf-8"))
    print(f"{out}: {len(crates)} crates, {len(texts)} license texts, {out.stat().st_size // 1024} KB")


if __name__ == "__main__":
    main()
