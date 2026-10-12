"""mac-expected.json, what the Mac testing packet (docs/mac-testing.md)
compares a Mac's results with, made from another machine's:

    python tools/release/mac_expected.py <runs> <game> > mac-expected.json

<runs> holds `argus --project examples/<game> test --json` for each
example game, as <game>.json. <game> is a project where the packet's
section 7 ran (`argus new <game> --template platformer`, then its
`generate` and `asset preview` commands with --json, saved as
gen-sound.json and gen-sprite.json in <runs>).

A world's digest, and what a recipe or an art script makes, are the
same on every machine where the engine is deterministic, so each is the
same here as on the Mac unless the Mac differs.
"""

import hashlib
import json
import os
import platform
import sys


def sha(path):
    return hashlib.sha256(open(path, "rb").read()).hexdigest()


def main():
    runs, game = sys.argv[1], sys.argv[2]
    digests = {}
    for name in sorted(os.listdir(runs)):
        if not name.endswith(".json") or name.startswith("gen-"):
            continue
        report = json.load(open(os.path.join(runs, name), encoding="utf-8"))
        digests[name[:-5]] = [
            {"scenario": s["name"], "seed": r["seed"], "digest": r.get("digest")}
            for s in report["scenarios"]
            for r in s["seeds"]
        ]
    sound = json.load(open(os.path.join(runs, "gen-sound.json"), encoding="utf-8"))
    sprite = json.load(open(os.path.join(runs, "gen-sprite.json"), encoding="utf-8"))
    out = {
        "made_on": f"{platform.system()} {platform.machine()}",
        "note": "Each digest is a run's world when it ended: game, scenario, seed. Puppet's differ between machines (its skeletons use the platform's sin and cos).",
        "digests": digests,
        "generate": {
            "sound_toml_sha256": sha(os.path.join(game, "assets", "sfx", "zap.toml")),
            "sound_heard": sound["heard"],
            "sprite_script_sha256": sha(os.path.join(game, "assets", "creatures", "blob.art.luau")),
            "sprite_sheet_sha256": sha(os.path.join(game, ".argus", "assets", "creatures", "blob.png")),
            "sprite_preview_sha256": sha(os.path.join(game, ".argus", "previews", "creatures_blob.png")),
            "sprite_drawn": sprite["drawn"],
        },
    }
    json.dump(out, sys.stdout, indent=1)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
