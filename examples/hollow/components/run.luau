-- How this run of the hollow is going, and what there is to find in it.
-- Run is on the HUD entity; sim.run keeps both.
component "Run" {
  doc = "How this run of the hollow is going: when it began, how often the hero fell, what they found, and whether they found the way out",
  started = field.f32 { default = 0, doc = "Sim seconds when this run began" },
  falls = field.u32 { default = 0, doc = "Times the hero fell into the pit this run" },
  gems = field.u32 { default = 0, doc = "Gems the hero has taken this run" },
  done = field.bool { default = false, doc = "The hero reached the way out" },
  time = field.f32 { default = 0, doc = "Seconds the run took, once done" },
  restart = field.bool { default = false, doc = "A new run was asked for (the pause menu); sim.run starts it" },
}

return component "Gem" {
  doc = "Something to find: taken when the hero touches it, and counted on the HUD. Off (active false), it is neither drawn nor taken",
  active = field.bool { default = false, doc = "In the game; off, it is neither drawn nor taken" },
  taken = field.bool { default = false, doc = "The hero has it this run" },
}
