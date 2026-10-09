-- The game's own components. Each field has a type, a default, and a doc;
-- `tune = true` puts it in the tuning panel.

component "Player" {
  doc = "The one player: walks by the move action",
  speed = field.f32 { default = 6.0, range = {0, 50}, tune = true, doc = "Units per second" },
}
