-- How the level looks: presentation only. Nothing in the game's rules reads
-- this; systems/looks.luau does, to draw.
--
-- On each layer of the backdrop (scenes/level/clouds.toml, hills.far.toml,
-- hills.near.toml). What to change first: follow, to bring a layer nearer
-- or push it farther away. The engine's own Parallax draws a sprite slower
-- than the world too; this one also repeats its strip and lets it drift.
return component "Backdrop" {
  doc = "A backdrop layer that slides by slower than the level as the camera moves, so it looks far away. Its Sprite is a strip whose ends meet, with a copy of it as a child `width` to its right; late.looks keeps the pair under the camera's view.",
  follow = field.f32 { default = 0.5, range = {0, 1}, tune = true, doc = "How much of the camera's movement the layer keeps: 0 moves with the level, 1 stays on the screen. Farther is nearer 1" },
  width = field.f32 { default = 24, range = {1, 1000}, doc = "World units after which the strip repeats (its Sprite's width). Keep it wider than the camera's view" },
  drift = field.f32 { default = 0, tune = true, doc = "Units a second the layer slides left on its own, as clouds do" },
  slid = field.f32 { default = 0, doc = "How far it has drifted so far (written by late.looks)" },
}
