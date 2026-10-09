-- Puppet: the skeletal module's example. The puppets are Spine skeletons
-- (assets/puppet/puppet.json); these are the show around them.

component "Show" {
  doc = "Cues the hero to wave every so often, paces it while it walks, and counts what the puppets did",
  every = field.u32 { default = 240, range = {1, 100000}, tune = true, doc = "Ticks between waves" },
  pace = field.f32 { default = 0.6, range = {0, 10}, tune = true, doc = "The hero's walking speed, world units a second" },
  timer = field.u32 { default = 0, doc = "Ticks since the last cue" },
  cues = field.u32 { default = 0, doc = "Waves cued" },
  waves = field.u32 { default = 0, doc = "Waves the hero finished (its waved events)" },
  steps = field.u32 { default = 0, doc = "Footsteps (step events) of every puppet" },
}

component "Cue" {
  doc = "What the show asks of a puppet this tick; its animator reads it",
  wave = field.bool { default = false, doc = "Wave now" },
}
