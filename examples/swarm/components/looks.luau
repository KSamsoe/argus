-- Swarm's looks: state the presentation keeps for itself (systems/looks.luau,
-- systems/hud.luau). No gameplay system or the plugin reads these.

component "FloorTile" {
  doc = "A tile of the deck under the swarm: its slot in the grid laid under the camera, and the world cell it shows now (12 units a cell)",
  col = field.i32 { default = 0, doc = "Column in the grid, 0 at the left" },
  row = field.i32 { default = 0, doc = "Row in the grid, 0 at the bottom" },
  cx = field.i32 { default = 0, doc = "The world cell it shows: x" },
  cy = field.i32 { default = 0, doc = "The world cell it shows: y" },
}

component "Hud" {
  doc = "The HUD's own state (systems/hud.luau): how long the wave's banner still shows, and how many HP pips are lit",
  banner = field.f32 { default = 0.0, doc = "Seconds the wave banner still shows" },
  lit = field.i32 { default = -1, doc = "HP pips lit now; -1 until the first tick" },
}
