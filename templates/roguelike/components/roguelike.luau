-- The game's own components: what a turn reads and writes. Each field has
-- a type, a default, and a doc; `tune = true` puts it in the tuning panel,
-- so you can try a value while the game runs.
--
-- How a turn goes (one turn is one press of a key):
--   sim.dungeon    makes the dungeon when there is none (or on restart)
--   sim.hero_turn  the hero steps, attacks, or waits
--   sim.monsters   then every monster attacks or steps toward the hero
--   sim.messages   then the message line says what happened

component "Health" {
  doc = "Hit points, for the hero and the monsters; at zero they die",
  cur = field.i32 { default = 10, range = {0, 1000}, doc = "Hit points now" },
  max = field.i32 { default = 10, range = {1, 1000}, doc = "Hit points at full health" },
}

component "Player" {
  doc = "The hero: acts first every turn, on the move or wait action",
  atk = field.i32 { default = 2, range = {0, 100}, tune = true, doc = "Damage a hit does" },
  kills = field.u32 { default = 0, doc = "Monsters killed this run" },
  dead = field.bool { default = false, doc = "Killed: the run is over until a restart" },
}

component "Monster" {
  doc = "A monster: acts after the hero, biting when next to it, else stepping toward it",
  kind = field.str { default = "rat", doc = "What it is, for the messages (sim.dungeon's KINDS)" },
  atk = field.i32 { default = 1, range = {0, 100}, doc = "Damage a bite does" },
  sight = field.u8 { default = 10, range = {0, 255}, tune = true, doc = "Steps away at which it notices the hero and starts to chase" },
}

component "Dungeon" {
  doc = "The dungeon: this entity's Tilemap holds its cells (the walls layer blocks, the floor layer is drawn), painted by sim.dungeon",
  runs = field.u32 { default = 0, doc = "Runs started so far; 0 means the dungeon is not made yet" },
}

component "Turn" {
  doc = "The turn counter: one more each turn the hero acts",
  n = field.u32 { default = 0, doc = "Turns taken this run" },
}
