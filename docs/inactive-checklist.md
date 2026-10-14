# Inactive entities: checklist

GitHub issue 18 asked for runtime rooms: Bring Your Own Game Master makes each floor's 5 or 6 rooms from a seed, one shown at a time, each keeping its state while the player is elsewhere, and had every system filter by the current room. It offered two ways: rooms a script makes, streamed like authored ones, or a way to park entities so queries, drawing, and the frame skip them.

Decided with Kevin, 2026-10-13: parking, as a general feature rather than a room-shaped one. Switching entities off is what pools, prepared menus and levels, held items, actors waiting offstage, and open worlds without authored rooms all need; runtime rooms serve one kind of game, and would mean rooms that are not files, new state in keyframes, replays, and saves, and a script API to transition inside the sim. A game that makes its rooms keeps them in its own code, on top of parking.

## 0. The engine

- An entity switched off (`Inactive`) and everything under it (`InactiveUnder`, kept by the engine) stay in the world, saved, in the digest, and put back by a seek, but every query skips them unless it asks: systems, scripts, plugins, drawing, physics, the UI, audio, the broadphase, and the semantic frame.
- A child switched off by itself stays off when its parent comes back on. Reparenting and spawning under an inactive parent are settled at the hierarchy stage; a switch, a scene or room load, and an edit settle at once.

## 1. Scripts, files, and the protocol

- `e:set_active(on)`, `e:active()`, `q:inactive(...)`; the system declares `writes = { "Inactive" }`. An entity file starts one off with `tags = ["Inactive"]`.
- The protocol's `world.query` shows the world as it is, inactive entities included; `not: ["Inactive", "InactiveUnder"]` leaves them out. An edit that adds or removes `Inactive` switches the entity and its children.

## Gate

- Tests: a subtree switched off and on, a child off by itself, reparenting, the broadphase, the digest, a save, and a snapshot; a script swapping two rooms, the frame following, `q:count` and `q:inactive`, the protocol's query, a seek, an undeclared write refused, an entity file written inactive with its child, and an edit switching it on.
- Then Bring Your Own Game Master's own rooms (tag each room's entities, switch off all but the player's) are the game's to do.
