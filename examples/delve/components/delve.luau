-- Delve: a grid roguelike core on the discrete clock. One turn is one
-- sim.advance: the hero acts on its command, then every monster acts in
-- initiative order.

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
}

component "Monster" {
  doc = "Hostile; acts after the hero, highest initiative first",
  kind = field.str { default = "rat" },
  atk = field.i32 { default = 1, range = {0, 100} },
  initiative = field.u8 { default = 5, range = {0, 20} },
  sight = field.u8 { default = 60, range = {0, 255}, doc = "Manhattan distance at which it notices the hero" },
}

component "Stairs" {
  doc = "Descend when the hero stands here and presses descend",
}

component "Floor" {
  doc = "The current floor: walkability as one character per cell, row-major, '.' floor '#' wall",
  map = field.str { default = "" },
  w = field.u8 { default = 40, range = {8, 200} },
  h = field.u8 { default = 20, range = {8, 200} },
  n = field.u32 { default = 0, doc = "Floors generated so far; 0 means none yet" },
  rooms = field.u8 { default = 0 },
}

component "Turn" {
  doc = "Turn counter",
  n = field.u32 { default = 0 },
}

component "Tile" {
  doc = "A drawn floor cell; regenerated with the floor",
}
