"""Make argus.mcpb, the MCP Bundle for Claude Desktop (distribution item 4),
from a staged install (tools/release/stage.sh):

    python tools/release/mcpb.py [stage dir] [out file]

A bundle is a zip with manifest.json (manifest_version 0.3, a "binary"
server) beside the program. It holds bin/argus and templates/: the MCP
server is argus itself, which starts the project's daemon as itself, and
new_project needs the templates. The runtimes stay out (no MCP tool builds
a release), which keeps it small. Claude Desktop asks for the project
folder when the bundle is installed; left empty, the server starts on no
project and offers them.

The manifest's tools are the server's own tools/list, asked of the staged
argus, so the bundle names what it serves.
"""

import json
import os
import subprocess
import sys
import zipfile

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))


def version():
    for line in open(os.path.join(ROOT, "Cargo.toml"), encoding="utf-8"):
        if line.startswith("version = "):
            return line.split('"')[1]
    raise SystemExit("mcpb.py: no version in Cargo.toml")


def tools(argus):
    """The server's tools, from tools/list on a server with no project."""
    req = [
        {"jsonrpc": "2.0", "id": 1, "method": "initialize",
         "params": {"protocolVersion": "2025-06-18", "capabilities": {},
                    "clientInfo": {"name": "mcpb.py", "version": "0"}}},
        {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}},
    ]
    env = dict(os.environ)
    env.pop("ARGUS_PROJECT", None)
    out = subprocess.run(
        [argus, "mcp"],
        input="".join(json.dumps(r) + "\n" for r in req),
        capture_output=True, text=True, cwd=os.path.dirname(argus), env=env, timeout=120,
    ).stdout
    for line in out.splitlines():
        msg = json.loads(line)
        if msg.get("id") == 2:
            return [
                {"name": t["name"], "description": t["description"].split("\n")[0]}
                for t in msg["result"]["tools"]
            ]
    raise SystemExit(f"mcpb.py: {argus} mcp did not list its tools: {out[:500]}")


def manifest(v, rev, tool_list):
    return {
        "manifest_version": "0.3",
        "name": "argus",
        "display_name": "Argus",
        "version": v,
        "description": "Make 2D games with an agent: the Argus engine's tools on a live game, its scenarios, and its marks.",
        "long_description": (
            "Argus is a protocol-first 2D game engine: the game runs in a daemon that holds its world, and these tools read "
            "and change it, run time forward and back, look at frames, and run the project's scenarios. Pick a project folder "
            "to open it, or leave it empty and ask for the projects: the server lists them, opens one, or makes a new one "
            f"from a template. Built from {rev}."
        ),
        "author": {"name": "Kevin"},
        "server": {
            "type": "binary",
            "entry_point": "bin/argus.exe",
            "mcp_config": {
                "command": "${__dirname}/bin/argus.exe",
                "args": ["mcp", "--project", "${user_config.project}"],
            },
        },
        "tools": tool_list,
        "user_config": {
            "project": {
                "type": "directory",
                "title": "Argus project",
                "description": "The game's folder, the one with argus.toml. Leave it empty to choose a project, or make one, in the chat.",
                "required": False,
            },
        },
        "compatibility": {"platforms": ["win32"]},
        "keywords": ["game", "engine", "2d", "argus"],
    }


def main():
    v = version()
    stage = sys.argv[1] if len(sys.argv) > 1 else os.path.join(ROOT, "target", "stage", f"argus-{v}-windows")
    out = sys.argv[2] if len(sys.argv) > 2 else os.path.join(os.path.dirname(stage), f"argus-{v}.mcpb")
    argus = os.path.join(stage, "bin", "argus.exe")
    if not os.path.isfile(argus):
        raise SystemExit(f"mcpb.py: no {argus}; run tools/release/stage.sh first (the bundle is for Windows)")
    staged = json.load(open(os.path.join(stage, "manifest.json"), encoding="utf-8"))
    m = manifest(v, staged["revision"], tools(argus))
    files = [("bin/argus.exe", argus)]
    tdir = os.path.join(stage, "templates")
    for dirpath, _, names in os.walk(tdir):
        for n in sorted(names):
            full = os.path.join(dirpath, n)
            files.append(("templates/" + os.path.relpath(full, tdir).replace(os.sep, "/"), full))
    tmp = out + ".tmp"
    with zipfile.ZipFile(tmp, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("manifest.json", json.dumps(m, indent=2) + "\n")
        for arc, full in files:
            z.write(full, arc)
    os.replace(tmp, out)
    print(f"{out}: {len(files) + 1} files, {os.path.getsize(out) // 1024} KiB, {len(m['tools'])} tools, Argus {v} ({staged['revision']})")


if __name__ == "__main__":
    main()
