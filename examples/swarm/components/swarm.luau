-- Swarm: a bullet heaven core. Everything is an entity; the player is the
-- only authored one besides the director.

component "Health" {
  doc = "Hit points; zero is dead",
  cur = field.i32 { default = 10, range = {0, 100000}, doc = "Current" },
  max = field.i32 { default = 10, doc = "Maximum" },
}

component "Player" {
  doc = "The one player: moves by the move action, fires a rotating fan of bullets",
  speed = field.f32 { default = 6.0, range = {0, 50}, tune = true, doc = "Units per second" },
  fire_every = field.u8 { default = 6, range = {1, 255}, tune = true, doc = "Ticks between bursts" },
  bullets_per_burst = field.u8 { default = 12, range = {1, 64}, tune = true },
  cooldown = field.u8 { default = 0, doc = "Ticks until the next burst" },
  aim = field.f32 { default = 0.0, doc = "Fan rotation, radians" },
  iframes = field.u8 { default = 0, doc = "Ticks of invulnerability left after a hit" },
  kills = field.u32 { default = 0 },
  dead = field.bool { default = false },
}

component "Enemy" {
  doc = "Chases the player; touching the player hurts",
  speed = field.f32 { default = 3.0, range = {0, 50}, tune = true },
  damage = field.i32 { default = 1, range = {0, 1000} },
  wave = field.u32 { default = 0, doc = "Wave it spawned in" },
  hold = field.bool { default = false, doc = "The chase leaves it where it is (a dasher winding up, charging, or recovering moves itself)" },
}

-- The dasher's rules are Rust (sim.dash in plugins/swarm-hot, which
-- declares this component field for field), its looks Luau (sim.dash_look).
component "Dasher" {
  doc = "An enemy that walks like the others, then every few seconds stops, winds up (it swells and turns yellow), charges in a straight line through where the player was when the wind-up began, and recovers",
  state = field.u8 { default = 0, range = {0, 3}, doc = "0 walking, 1 winding up, 2 charging, 3 recovering" },
  timer = field.u32 { default = 0, doc = "Ticks left of the wind-up or the recovery; while walking, ticks until it may wind up again" },
  every = field.u32 { default = 150, range = {1, 100000}, tune = true, doc = "Ticks of walking between dashes" },
  range = field.f32 { default = 14.0, range = {0, 1000}, tune = true, doc = "It winds up only with the player this close" },
  windup = field.u32 { default = 40, range = {1, 100000}, tune = true, doc = "Ticks it stands and swells before it charges" },
  charge_speed = field.f32 { default = 30.0, range = {0, 500}, tune = true, doc = "Units per second while charging" },
  overshoot = field.f32 { default = 3.0, range = {0, 100}, tune = true, doc = "How far past the spot it charges at it runs" },
  recover = field.u32 { default = 40, range = {0, 100000}, tune = true, doc = "Ticks it stands after a charge" },
  target = field.vec2 { doc = "Where the player was when the wind-up began" },
  dir = field.vec2 { doc = "The charge's direction, fixed when the wind-up began" },
  left = field.f32 { default = 0.0, doc = "Units of the charge still to run" },
}

component "Bullet" {
  doc = "Flies straight for ttl ticks and dies on its first hit",
  ttl = field.u8 { default = 60, range = {0, 255} },
  damage = field.i32 { default = 2, range = {0, 1000} },
}

component "Velocity" {
  doc = "Units per second",
  v = field.vec2 {},
}

component "Director" {
  doc = "Wave state: spawns the next wave on a ring around the player once the last is cleared",
  wave = field.u32 { default = 0, doc = "Waves spawned so far" },
  first = field.u32 { default = 200, range = {1, 100000}, tune = true, doc = "Enemies in wave 1" },
  growth = field.f32 { default = 2.0, range = {1, 10}, tune = true, doc = "Wave size multiplier" },
  cap = field.u32 { default = 50000, range = {1, 200000}, doc = "Most enemies in one wave" },
  spawn_radius = field.f32 { default = 30.0, range = {1, 1000}, tune = true },
  gap = field.u32 { default = 30, range = {0, 100000}, tune = true, doc = "Ticks between a clear and the next wave" },
  timer = field.u32 { default = 0 },
  alive = field.u32 { default = 0, doc = "Enemies alive at the last tick" },
  cleared = field.bool { default = false },
  log_kills = field.bool { default = true, doc = "Emit a death event per enemy (off for stress scenes)" },
  dash_from = field.u32 { default = 2, range = {1, 100000}, tune = true, doc = "The first wave with dashers in it" },
  dash_share = field.f32 { default = 0.1, range = {0, 1}, tune = true, doc = "Share of a wave's enemies that are dashers, from dash_from on" },
}
