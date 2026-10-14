-- Delve: a grid roguelike core on the discrete clock. One turn is one
-- sim.advance: the hero acts on its command, then every monster acts in
-- initiative order.

component "LogLine" {
  doc = "A past turn's line in the message log (scenes/delve/log.toml), its order the turn it came",
}

component "Health" {
  doc = "Hit points; zero is dead",
  cur = field.i32 { default = 10, range = {0, 1000}, doc = "Current" },
  max = field.i32 { default = 10, doc = "Maximum" },
}

component "Player" {
  doc = "The hero: acts first each turn on the move, wait, or descend command",
  atk = field.i32 { default = 3, range = {0, 100}, tune = true, doc = "Damage per hit" },
  floor = field.u32 { default = 0, doc = "Floor number, 1 up" },
  kills = field.u32 { default = 0 },
  dead = field.bool { default = false },
  descend = field.bool { default = false, doc = "Asked to take the stairs; sim.floor answers" },
  potions = field.u32 { default = 0, doc = "Healing potions carried; the drink command drinks one" },
  potion_heal = field.i32 { default = 4, range = {0, 100}, tune = true, doc = "Hit points a potion gives back, never above Health.max" },
}

component "Monster" {
  doc = "Hostile; acts after the hero, highest initiative first",
  kind = field.str { default = "rat" },
  atk = field.i32 { default = 1, range = {0, 100} },
  initiative = field.u8 { default = 5, range = {0, 20} },
  sight = field.u8 { default = 60, range = {0, 255}, doc = "Steps at which it notices the hero; it comes every turn when no wall is between them (a ray from cell to cell), else every other turn" },
  drop = field.u8 { default = 35, range = {0, 100}, doc = "Chance in a hundred that it leaves a potion where the hero kills it (the loot stream rolls it)" },
}

component "Potion" {
  doc = "A healing potion lying on the floor where a monster fell; the hero picks it up by walking onto it (sim.potions)",
}

component "Stairs" {
  doc = "Descend when the hero stands here and presses descend",
}

component "Floor" {
  doc = "The current floor: how many there have been and its rooms. Its cells are the runtime map this entity's Tilemap places (maps/floor.tilemap.toml: a walls layer, # in the semantic frame, and a floor layer, drawn), painted by sim.floor",
  n = field.u32 { default = 0, doc = "Floors generated so far; 0 means none yet" },
  rooms = field.u8 { default = 0 },
}

component "Turn" {
  doc = "Turn counter",
  n = field.u32 { default = 0 },
}

component "DebugView" {
  doc = "What the game draws for its developer (2D staples item 1): q:draw shapes, which a release build never shows. Flip a field with argus edit or the editor's inspector",
  sight = field.bool { default = false, doc = "Each monster in range's line to the hero: green where it sees the hero, red to the wall in the way" },
}
