-- The feather in the cave: a double jump. Taken on touch, it gives the hero
-- a jump in mid-air for the rest of the run, back each time they land; a
-- new run takes it away and puts the feather back. Feather is on the
-- pickup (rooms/a/feather.toml), AirJump on the hero; sim.run takes the
-- feather and sim.air_jump makes the jump (systems/feather.luau).
component "Feather" {
  doc = "A feather to take: touched, it gives the hero its jumps in mid-air (AirJump.max) for the rest of the run. Taken while the hero has them, so it is not drawn until a new run",
  jumps = field.u32 { default = 1, doc = "Jumps in mid-air it gives the hero, each back when they land" },
  taken = field.bool { default = false, doc = "The hero has its jumps this run (written by sim.run)" },
}

return component "AirJump" {
  doc = "The hero's jumps in mid-air, the feather's gift: how many they have, how many are left before they land (each comes back then), and how high one goes",
  max = field.u32 { default = 0, doc = "Jumps in mid-air the hero has, each back on landing: 0 until the feather is taken this run" },
  left = field.u32 { default = 0, doc = "Jumps in mid-air left before the hero lands (written by sim.air_jump and sim.run)" },
  height = field.f32 { default = 0, range = { 0, 20 }, tune = true, doc = "Height of a full jump in mid-air, in world units above where it starts; 0 is the CharacterController's jump_height" },
}
