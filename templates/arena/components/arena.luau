-- The game's own components. Each field has a type, a default, and a doc;
-- `tune = true` puts it in the editor's tuning panel, so you can try values
-- while the game runs. The numbers to change first are marked tune.

component "Player" {
  doc = "The hero: walks by the move action and fires at the nearest enemy on its own",
  speed = field.f32 { default = 7.0, range = {0, 50}, tune = true, doc = "Walking speed, units per second" },
  fire_every = field.u8 { default = 12, range = {1, 255}, tune = true, doc = "Ticks between shots (60 ticks a second)" },
  range = field.f32 { default = 12.0, range = {0, 100}, tune = true, doc = "It fires only at an enemy this close; 0 never fires" },
  bullet_speed = field.f32 { default = 22.0, range = {1, 200}, tune = true, doc = "Bullet speed, units per second" },
  grace = field.u8 { default = 45, range = {0, 255}, tune = true, doc = "Ticks it cannot be hurt again after a hit (it blinks)" },
  cooldown = field.u8 { default = 0, doc = "Ticks until the next shot (written by sim.fire)" },
  safe = field.u8 { default = 0, doc = "Ticks of grace left (written by sim.hits)" },
  score = field.u32 { default = 0, doc = "Enemies killed this run" },
  dead = field.bool { default = false, doc = "Out of health: the game stands still until restart" },
}

component "Health" {
  doc = "Hit points, on the hero and on every enemy; zero is dead",
  cur = field.i32 { default = 5, range = {0, 1000}, doc = "Now" },
  max = field.i32 { default = 5, range = {1, 1000}, tune = true, doc = "Full; restart fills it" },
}

component "Enemy" {
  doc = "Walks straight at the hero (sim.chase); touching the hero hurts it (sim.hits)",
  speed = field.f32 { default = 3.0, range = {0, 50}, doc = "Units per second (sim.waves picks one per enemy)" },
  damage = field.i32 { default = 1, range = {0, 100}, doc = "Health a touch takes from the hero" },
}

component "Bullet" {
  doc = "Flies straight until the arena's wall or ttl ticks, and is spent on the first enemy it hits",
  vel = field.vec2 { doc = "Velocity, units per second" },
  ttl = field.u8 { default = 50, range = {0, 255}, doc = "Ticks left before it is gone" },
  damage = field.i32 { default = 1, range = {0, 100}, doc = "Health a hit takes from an enemy" },
}

component "Waves" {
  doc = "The wave director, on the arena: when a wave is cleared, waits gap ticks and sends the next, bigger one in from the arena's edges",
  first = field.u32 { default = 6, range = {1, 1000}, tune = true, doc = "Enemies in wave 1" },
  more = field.u32 { default = 4, range = {0, 1000}, tune = true, doc = "Enemies each wave adds over the one before" },
  enemy_health = field.i32 { default = 2, range = {1, 100}, tune = true, doc = "Hits an enemy takes to die" },
  slowest = field.f32 { default = 2.5, range = {0, 50}, tune = true, doc = "The slowest enemy's speed, units per second" },
  fastest = field.f32 { default = 3.5, range = {0, 50}, tune = true, doc = "The fastest enemy's speed; each one is somewhere between" },
  gap = field.u32 { default = 90, range = {0, 100000}, tune = true, doc = "Ticks between a cleared wave and the next" },
  timer = field.u32 { default = 60, doc = "Ticks until the next wave comes (the first waits a second)" },
  wave = field.u32 { default = 0, doc = "Waves sent so far: the HUD's wave number" },
  alive = field.u32 { default = 0, doc = "Enemies alive now: the HUD's count" },
}
