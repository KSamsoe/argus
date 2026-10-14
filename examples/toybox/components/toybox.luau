-- Toybox: the rigid-body module's example. Everything that moves is a
-- RigidBody; the scripts only drop balls and clear them away.

component "Dropper" {
  doc = "Drops a ball from the sky every so often, while fewer than max are in play",
  every = field.u32 { default = 90, range = {1, 100000}, tune = true, doc = "Ticks between balls" },
  max = field.u32 { default = 12, range = {0, 1000}, tune = true, doc = "Balls in play at most" },
  timer = field.u32 { default = 0, doc = "Ticks to the next ball" },
  dropped = field.u32 { default = 0, doc = "Balls dropped so far" },
}

component "Ball" {
  doc = "A dropped ball",
  born = field.u32 { default = 0, doc = "The tick it was dropped" },
}

component "Laser" {
  doc = "A tripwire: a ray cast to the right from the entity, what it meets, and how often a ball broke it",
  length = field.f32 { default = 10, range = {0, 1000}, doc = "How far the ray reaches" },
  hit = field.str { doc = "What it meets: an entity's name, \"ball\", or empty for nothing" },
  distance = field.f32 { doc = "How far along the ray that is" },
  tripped = field.u32 { doc = "Times a ball broke the beam" },
}
