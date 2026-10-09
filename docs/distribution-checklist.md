# Distribution checklist

After phase 5, Argus runs only where it was built: a clone of the private repository, Rust on the GNU target, and a C++ compiler for Luau. This plan makes it something a person installs on a machine and connects to the AI tools they already use. The order and four decisions were agreed with Kevin on 2026-10-09.

## Decided with Kevin (2026-10-09)

- **Who first: Kevin's machines, then people he invites.** Releases live on the private GitHub repository. A collaborator installs with a GitHub token, and no license or public listing is needed yet. Going public later is a switch (a public releases repository, a license, the MCP registry), not a rebuild.
- **Windows and Linux first.** Both are built and tested here: this laptop, and klab as the Linux machine. macOS is recorded as not built, as in phase 5 item 11.
- **Every MCP client:** Claude Code and Claude Desktop, Cursor and VS Code, Codex CLI and Gemini CLI, and remote MCP for the web apps (claude.ai, ChatGPT).
- **Releasing a game needs no Rust.** The install carries the game runtime for each platform and the web player, and `argus build` packs a game around them. A game with Rust plugins still needs Rust, for the plugin only.

## What there is now

- `argus.exe` and `argus-editor.exe` start from any shell: the C and C++ runtimes are linked in (`.cargo/config.toml`).
- `argus mcp --project <dir>` is a stdio MCP server, with tools generated from the one schema (phase 2 item 8). `argus docs --for-agent` writes ARGUS.md, which now carries the scripting and scenario APIs (the phase 5 gate).
- `argus build` compiles a game's executable from the engine's crates: it needs the source tree (`ARGUS_ENGINE`, or the path the CLI was built at) and cargo. A web build also needs the wasm target, wasi-sdk, and a pinned wasm-bindgen-cli.
- There is no version (`argus --version` is an unknown flag), no `argus new`, and no installer.

## 0. Versions, and what a release holds

- `argus version`, and the version in `session.status`, ARGUS.md, a release's `build.json`, and marks and reports (they carry the code revision already).
- One version for the engine, the CLI, the editor, and the runtimes. A release is a version tag and an archive per platform with a manifest listing every file and its SHA-256.

## 1. Game runtimes in the install

- A prebuilt game runtime for each platform: `argus-game` with every module but Steam, and a Steam variant. A game without Steam must not need Steam's library.
- The prebuilt web player: the wasm module, its glue, and the page.
- `argus build` puts a game's packs beside the right runtime, with no compiler. The source path stays for engine work, behind a flag or `ARGUS_ENGINE`.
- The checks it makes now (the build plays and looks as the project does) run the same way.
- A release for the other platform (Linux from Windows) takes that platform's runtime from the install, so an install carries the runtimes of every supported platform, or fetches them on demand.
- Plugins: the runtime loads a game's plugin libraries as the daemon does, built with the installed SDK. Rust is needed for those alone, and `argus build` says so.

## 2. `argus new`, and a project an agent can start in

- `argus new <dir> [--template blank|platformer|roguelike|arena|colony]`: a working project from a template (the four games trimmed to a start), its first scenario passing.
- Each new project gets its ARGUS.md, and an `AGENTS.md` and a `CLAUDE.md` that point at it and at `argus docs`, so any agent knows where to read first.

## 3. MCP for the local clients

- `argus mcp setup <client>`, with `--project` and `--scope project|user`, for claude-code, claude-desktop, cursor, vscode, codex, and gemini. It writes that client's own file and key, merging with what is there, idempotent, with a backup:

  | Client | Project file | User file | Key |
  |---|---|---|---|
  | Claude Code | `.mcp.json` | `~/.claude.json` (or `claude mcp add`) | `mcpServers` |
  | Claude Desktop | (none) | `claude_desktop_config.json` | `mcpServers` |
  | Cursor | `.cursor/mcp.json` | `~/.cursor/mcp.json` | `mcpServers` |
  | VS Code | `.vscode/mcp.json` | the profile's `mcp.json` | `servers` |
  | Codex CLI | `.codex/config.toml` | `~/.codex/config.toml` | `[mcp_servers.argus]` |
  | Gemini CLI | `.gemini/settings.json` | `~/.gemini/settings.json` | `mcpServers` |

- `argus mcp setup` with no client lists the ones it finds installed. Each file's location is checked against the client's own docs when the item is built, since these move.
- A project-free MCP mode for the user-level clients: `argus mcp` with no project offers the projects it knows (recent, or under a folder), opens one, and makes a new one, so one global entry serves every project.

## 4. Claude Desktop in one click

- `argus.mcpb`, an MCP Bundle (manifest_version 0.3, server type `binary`), built with each Windows release. It asks for the project folder (a `directory` setting) and runs the bundled `argus mcp`. Double-clicking it installs it.

## 5. The release pipeline

- `tools/release/`: builds the Windows archive here and the Linux one on klab, checks them (each runtime builds and runs a game; the editor starts headless), writes the manifest and checksums, and makes a GitHub release on the private repository with `gh`.
- klab as a second self-hosted runner, so a release is one workflow. Installing the runner is a download, so it is Kevin's to allow.

## 6. Installing and updating

- `install.ps1` and `install.sh`: fetch the latest release from the private repository with a GitHub token (`gh auth token`, or `GITHUB_TOKEN`), check the checksums, install for the user alone (`%LOCALAPPDATA%\Argus`, `~/.local/share/argus` with `~/.local/bin`), and put `argus` on the PATH.
- `argus self-update` (the same, in place, keeping the old version until the new one starts) and `argus uninstall`.
- What a machine needs, said up front: a GPU driver with DX12 or Vulkan, or the software rasteriser (WARP is in Windows; Linux needs Mesa's lavapipe). On Linux, ALSA and X11 or Wayland libraries. Kept as few as the build allows.

## 7. Clean machines

- Install on machines that have never built Argus: a fresh Windows user profile here, and WSL's Ubuntu on this PC (which has no Rust). From there: `argus new`, the tests, an agent's session, `argus build`, and the built game running.
- klab as an installed (not built) Linux machine too.

## 8. Remote MCP for the web apps

- MCP over Streamable HTTP from the daemon's web listener (phase 5 item 10), with the scoped tokens. Local clients that speak HTTP (Claude Code, VS Code, Codex) can use it over the tailnet as is.
- claude.ai and ChatGPT connect from their own cloud, not from the user's machine, so a server they use must be reachable from the public internet. That means a tunnel (Tailscale Funnel is the natural one here), and authorization they support: OAuth 2.1 with dynamic client registration and PKCE, found through protected-resource metadata (RFC 9728).
- Item 10 kept the daemon to this machine and the tailnet on purpose, so opening it to the internet is Kevin's decision, taken at this item, with what it exposes written down.

## 9. Docs for someone new

- `docs/install.md`: installing, the first project, connecting each client, releasing a game.
- The README's top says what Argus is and how to install it, before the build history.

## The gate

- On a clean Windows profile and on WSL's Ubuntu, with no Rust:
  - install from a release on the private repository with a token;
  - `argus new`;
  - `argus mcp setup` for each local client (each file checked, and Claude Code connected for real);
  - an agent adds a feature through MCP;
  - `argus build` makes a release of it, which runs.
- `argus.mcpb` installs into Claude Desktop and opens a project.
- A claude.ai connector reaches a daemon through the chosen tunnel and its authorization, or this is recorded as not done, with why.
- Every earlier gate's tests still pass.
