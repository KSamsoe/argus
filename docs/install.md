# Installing Argus

Argus is a 2D game engine you build games in together with an AI agent. The game runs in a background program (the daemon) that holds the game's world, and both you and the agent work on that world through the same tools: you in the editor or the `argus` command, the agent through MCP, the protocol AI tools use to call other programs.

This page covers installing Argus, making a first game, connecting the AI tools you use, and releasing a game.

**Just want to play the example games?** Each release also has `example-games-<version>` for Windows (a zip), Linux, and the Mac (each a tar.gz): Hollow, Delve, Swarm, and Outpost (the Mac's has the first three), built by Argus, with nothing to install. Unpack it and run the program in a game's folder; its `README.txt` has the controls. Windows may stop the first run with "Windows protected your PC", because the programs are not signed: choose More info, then Run anyway.

```sh
gh release download --repo KSamsoe/argus --pattern "example-games-*-windows-x86_64.zip"
```

## What you need

- Windows 10 or 11 (64-bit), or Linux on x86-64 with glibc 2.39 or newer: Ubuntu 24.04, Debian 13, Fedora 40, or newer. (The Linux build is made on Ubuntu 24.04, and a game released for Linux needs the same of its players.)
- Or a Mac with macOS 11 or newer, Apple Silicon or Intel (one build runs on both). **The Mac build is new:** it is made on Linux and had not run on a Mac when it was released, so see [testing it on a Mac](mac-testing.md).
- A graphics card with DirectX 12, Vulkan, or Metal (every Mac since 2012). Without one, Argus draws in software: Windows has this built in, and on Linux it needs Mesa's software Vulkan (`mesa-vulkan-drivers`).
- On Linux, for sound and windows: ALSA (`libasound2`) and X11 or Wayland, which a desktop already has.
- Access to the Argus repository on GitHub, and a way to prove it: the GitHub CLI signed in (`gh auth login`), or a token in `GITHUB_TOKEN` that can read the repository.

You do not need Rust or a compiler, unless a game has Rust plugins (one of the examples does): those are built with Rust when the game is.

## Install

On Windows, in PowerShell:

```powershell
gh release download --repo KSamsoe/argus --pattern install.ps1
powershell -ExecutionPolicy Bypass -File install.ps1
```

On Linux or a Mac:

```sh
gh release download --repo KSamsoe/argus --pattern install.sh
sh install.sh
```

The installer downloads the latest release, checks it against the release's checksums, and installs it for you alone: in `%LOCALAPPDATA%\Argus` on Windows, `~/.local/share/argus` on Linux and the Mac (with `argus` linked into `~/.local/bin`). It puts `argus` on your PATH; open a new terminal afterwards. Check it with:

```sh
argus version
```

If you already have a release's archive (`argus-<version>-windows-x86_64.zip`, or the Linux or Mac `.tar.gz`), install from it without a download: `install.ps1 -Archive <file>`, or `sh install.sh --archive <file>`. On a Mac, the installer clears the mark a browser puts on what it downloads, which would otherwise make the Mac refuse the programs: they are signed, but not by an Apple developer account.

## Your first game

```sh
argus new my-game
cd my-game
argus test
argus editor
```

`argus new` makes a game from a template and checks that its tests pass before it says done. The templates are a starting point for a kind of game: `blank` (a square that walks, the default), `platformer`, `roguelike`, `arena`, and `colony`. Pick one with `--template`:

```sh
argus new my-game --template platformer
```

`argus projects` lists the games you have opened, and the templates.

A new game has three files for AI agents: `AGENTS.md` (how to work on an Argus game), `CLAUDE.md` (the same, for Claude), and `ARGUS.md` (the game's own components, systems, and scenes, written from the running game). An agent should read them first.

## Connect your AI tools

`argus mcp setup <tool>` adds Argus to an AI tool's settings, next to whatever is there already (a copy of a file it changes is kept beside it, ending `.argus-backup`). Run it in a game's folder to connect that game, or add `--scope user` to connect every game: then the agent starts with no game open, lists your games, opens one, or makes a new one.

| Tool | Command | Then |
|---|---|---|
| Claude Code | `argus mcp setup claude-code` | start `claude` in the game's folder; it asks once to use the game's tools |
| Claude Desktop | `argus mcp setup claude-desktop`, or install `argus-<version>.mcpb` from the release (below) | quit Claude Desktop fully (from the tray too) and start it again |
| Cursor | `argus mcp setup cursor` | turn Argus on under Settings, MCP, if it asks |
| VS Code | `argus mcp setup vscode` | trust the folder, then start Argus from the MCP servers view |
| Codex CLI | `argus mcp setup codex` | a game's own settings are read only once you trust its folder in Codex |
| Gemini CLI | `argus mcp setup gemini` | the same: trust the folder (`/permissions`) |

`argus mcp setup` on its own lists the tools and which ones it finds on this computer.

### Claude Desktop without installing Argus

Each release also has `argus-<version>.mcpb` (for Windows) and `argus-<version>-macos.mcpb` (for the Mac), a bundle Claude Desktop installs by itself, with Argus inside it. In Claude Desktop open **Settings**, then **Extensions**, then **Advanced settings**, and under **Extension Developer** choose **Install Extension…** and pick the file. It asks for a game's folder; leave it empty and the agent lists your games, opens one, or makes a new one.

Double-clicking the file does nothing in the Microsoft Store version of Claude Desktop: that version does not open `.mcpb` files itself, and passes a file it is started with to the running app, which ignores it.

## Release a game

```sh
argus build
```

This makes `dist/windows` (or `dist/linux`, or `dist/macos`): the game's executable, named after the game, and its data. It checks the release plays and looks exactly as the project does before it says done. Other targets:

```sh
argus build --target web      # a web page: dist/web, to put on any web server
argus build --target linux    # from Windows or a Mac: the Linux release, from the Linux runtime in the install
argus build --target macos    # the Mac's, from its runtime in the install (a game with Rust plugins is released on a Mac)
```

A Mac release is the game's executable beside its data, run from Terminal; it is not yet a Mac app you double-click.

## Update and uninstall

```sh
argus self-update             # the latest release, in place
argus self-update 0.0.6       # that version
argus uninstall               # removes Argus; your games and settings stay
```

The version you had stays until the new one has started, and comes back if it does not. On Windows the update finishes just after `argus` exits, and stops any Argus servers your AI tools have running; they start again when next used.

## When something is wrong

- **`argus` is not recognised:** open a new terminal, so it picks up the new PATH.
- **The installer says it needs a token:** sign in with `gh auth login`, or set `GITHUB_TOKEN` to a token that can read the repository.
- **A game with Rust plugins will not build:** install Rust (https://rustup.rs); `argus build` says when a game needs it.
- **An AI tool does not show the Argus tools:** run `argus mcp setup <tool>` again and read what it says to do next; most tools only load new servers when they start.
