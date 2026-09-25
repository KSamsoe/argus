-- How this run of the hollow is going. On the HUD entity; sim.run keeps it.
return component "Run" {
  doc = "How this run of the hollow is going: when it began, how often the hero fell, and whether they found the way out",
  started = field.f32 { default = 0, doc = "Sim seconds when this run began" },
  falls = field.u32 { default = 0, doc = "Times the hero fell into the pit this run" },
  done = field.bool { default = false, doc = "The hero reached the way out" },
  time = field.f32 { default = 0, doc = "Seconds the run took, once done" },
}
