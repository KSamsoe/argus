# The reference, and where to find each part

The manual explains; the reference lists everything, exactly. Argus writes its reference from the engine itself, so it cannot drift from what the engine does: the same tables make the command line's help, the MCP tools' descriptions, and these pages. This chapter says where each part is, from a shell, inside a project, and for an AI agent.

## The parts

| Part (topic) | What it lists |
|---|---|
| **Tools** (`tools`) | The verbs an agent uses (`summary`, `query`, `describe`, `observe`, `edit`, `run`, `seek`, `events`, `test`, `profile`, `reload`, `marks`, `propose`, `review`, and the project, file, and docs tools), each with its parameters and the protocol methods it calls |
| **Methods** (`methods`) | Every protocol method (`world.query`, `sim.step`, `obs.frame`, and about two hundred more), with parameters and what it returns. Every one is also a command: `argus world query`, `argus sim step` |
| **Scripting** (`scripting-api`) | The Luau API: what a component or system declaration takes, everything on `q` (the system's view of the world), entities, components, vectors, randomness, events, tweens |
| **Scenarios** (`scenario-api`) | The test API: `scenario`, `t:step`, `t:run_until`, `t:advance`, `t.input`, `t.expect` and its checks |
| **Files** (`files`) | Every file a project can have: where it lives, each key with its type, default, and meaning, an example, and the errors |
| **Components** (`components`) | The engine's own components (Transform, Sprite, Camera, CharacterController, NavAgent, UI, lights, and the rest), with their fields and which systems write and read them |
| **The project** (`project`) | A game's own components, systems in run order, prefabs, def tables, and scenes, read from its live daemon: its `ARGUS.md` |

## From a shell

```sh
argus docs                     # the whole reference: tools, scripting, scenarios, files, components, methods
argus docs list                # the topics: the manual's chapters and the reference's parts
argus docs scripting-api       # one part (tools, methods, scripting-api, scenario-api, files, components), or a chapter
argus docs files section=input.toml   # one section of it
argus docs find=NavAgent       # every section, in the reference and the manual, that mentions a word
argus docs project             # this project's ARGUS.md, made from its daemon now
argus help                     # every command and method, one line each
argus help world               # a namespace's methods in full
argus world query -h           # one method: usage, parameters, what it returns
argus docs --for-agent         # write the project's ARGUS.md from its daemon
```

## In a project

Every project has an `ARGUS.md`, written by `argus new` and rewritten by `argus docs --for-agent`. It holds the project's own parts (components, systems, prefabs, def tables, scenes) as the running game has them, then the scripting API, the scenario API, and the files reference. Regenerate it after changing components, systems, prefabs, or scenes; it is never edited by hand. If no daemon was running, it starts one to write it and stops it afterwards (unless another client has connected meanwhile), so a script can go straight on to something that will not share the project with a daemon. A daemon that was already running stays up.

`AGENTS.md` and `CLAUDE.md` beside it are short: they tell an agent what Argus is and to read `ARGUS.md` first.

## For an AI agent

An agent connected over MCP has the `docs` tool, so it can read all of this without a shell or a file system:

| Ask | Gets |
|---|---|
| `docs` | the list: the manual's chapters, the reference's parts, and `project` |
| `docs topic=scripting-api` | one part or chapter (`scripting` is the chapter, `scripting-api` the reference) |
| `docs topic=files section=input.toml` | one section of it (a long part, such as `methods`, answers with its sections first) |
| `docs find=CharacterController` | every section that mentions it, with its topic, for a second call |
| `docs topic=project` | the open game's `ARGUS.md`, made from its daemon now |

The tools' own descriptions (what `tools/list` gives an MCP client) are the tools part of the reference, word for word. `describe` answers for one component or system at a time, from the live game.

## Where to go next

- [Working with AI agents](agents.md): how an agent should use all of this.
- [Concepts](concepts.md): the ideas the reference is written in.
