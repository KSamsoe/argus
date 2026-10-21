# The Argus manual

This manual explains how Argus works and how to do things with it, for people and for the AI agents working with them. It is the guide; the reference, every tool, function, file key, and component, is `argus docs`, generated from the engine itself, and each project's `ARGUS.md` carries the parts for that game. Each chapter here says where in the reference to look next.

An AI agent connected to Argus reads this manual with the `docs` tool: `docs` alone lists the chapters, and `docs topic=<chapter>` (the file's name, such as `scripting`) gives one.

New to Argus? Read **[Concepts](concepts.md)**, then follow **[Your first game](first-game.md)**.

## Making a game

| Chapter | What it covers |
|---|---|
| [Concepts](concepts.md) | How Argus works: the project, the daemon and its protocol, entities, components, systems, the clocks, determinism, the change feed |
| [Your first game](first-game.md) | A tutorial: a game from a template, then a feature added end to end, tested, and built |
| [Scripting](scripting.md) | Writing a game's Luau: components, systems, the `q` handle, and the patterns that come up again and again |
| [Scenes](scenes.md) | Scenes and entity files, prefabs, rooms and streaming for big worlds, tilemaps |
| [Gameplay](gameplay.md) | Input, movement and collision, rigid bodies, navigation and jobs, events, randomness, timers and tweens, pausing, saves |
| [Presentation](presentation.md) | Sprites and animation, particles, materials, lighting, the camera, UI and fonts, sound and music |
| [Content](content.md) | Def tables, dialogue, timelines, strings and languages, accessibility, the player's settings |
| [Multiplayer](multiplayer.md) | Matches between machines: prediction and rollback, what a game needs, hosting and joining, the relay, testing a match |

## Working on it

| Chapter | What it covers |
|---|---|
| [Testing](testing.md) | Scenarios, seeds, goldens, reading a failure, and the replay a failing run saves |
| [Playtesting](playtesting.md) | Playing, the editor, marks and reports, the review page, proposals, the change feed and undo |
| [Tuning and performance](tuning-and-performance.md) | Tunable fields, sweeps, profiling, performance targets, and when to write a Rust plugin |
| [Working with AI agents](agents.md) | Connecting agents over MCP, the tools they get, how an agent should work on a game |

## Shipping it

| Chapter | What it covers |
|---|---|
| [Releasing](releasing.md) | `argus build`, the web, the other platform, patches, mods, plugins, running a relay, Steam, reports from players |
| [Reference](reference.md) | Where each part of the reference is, and how to read it from a shell, a project, or an agent |

Installing Argus and connecting each AI tool is in [docs/install.md](../install.md). Changing Argus itself is in [docs/developing.md](../developing.md).
