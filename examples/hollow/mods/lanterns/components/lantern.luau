-- A lantern (the lanterns mod): dark until the keeper lights them all,
-- then flickering. Named lanterns_ first, as everything a mod adds is.
return component "lanterns_Lantern" {
  doc = "A lantern the keeper lights: dark until then, then flickering",
  lit = field.bool { default = false, doc = "Burning" },
  phase = field.f32 { default = 0.0, doc = "Where its flicker starts, so no two flicker together" },
}
