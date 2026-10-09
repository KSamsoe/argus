-- The game's own components: what the level knows that the engine does not.
-- The engine already has the platformer's body (KinematicBody, Collider,
-- CharacterController); these say what the hero is trying to do and what
-- the things in the level are for.
--
-- Each field has a type, a default, and a doc. `tune = true` puts a field
-- in the editor's tuning panel, so it can be changed while the game runs.
-- A component's fields cannot change while a world runs: after editing
-- this file, `argus scene load` starts the level again with the new ones.

-- On the hero (scenes/level/player.toml): where it starts again, and how
-- this go at the level is going. systems/ read and write it; the HUD
-- (systems/hud.luau) shows it.
component "Hero" {
  doc = "The hero's run of the level: where it starts again after a hazard, the coins it has, how often it was sent back, and whether it reached the flag",
  start = field.vec2 { default = {3, 1}, doc = "Where the hero is put back after a hazard or a restart, in world units" },
  fall_y = field.f32 { default = -6, range = {-100, 0}, tune = true, doc = "Below this height the hero has fallen into a pit" },
  coins = field.u32 { default = 0, doc = "Coins taken this run (written by sim.pickups)" },
  falls = field.u32 { default = 0, doc = "Times a hazard sent the hero back this run (written by sim.hazards)" },
  done = field.bool { default = false, doc = "The hero reached the flag (written by sim.goal)" },
}

-- On spikes (scenes/level/spikes.toml). Give any entity with a Collider a
-- Hazard and touching it sends the hero back to the start.
component "Hazard" {
  doc = "Touching it sends the hero back to the start. Off (active false), it is harmless",
  active = field.bool { default = true, doc = "Hurts on touch; false makes it harmless (spikes that come and go would flip this)" },
}

-- On each coin (prefabs/coin.toml). Taken on touch and counted.
component "Coin" {
  doc = "Something to pick up: taken when the hero touches it, then hidden until the level starts again",
  taken = field.bool { default = false, doc = "The hero has it this run (written by sim.pickups)" },
}

-- On the flag at the end (scenes/level/goal.toml).
return component "Goal" {
  doc = "The end of the level: touching it finishes the run",
  reached = field.bool { default = false, doc = "The hero has touched it this run (written by sim.goal)" },
}
