-- The duel's data. Each field has a type, a default, and a doc; `tune = true`
-- puts it in the editor's tuning panel, so you can try values while the
-- game runs. The numbers to change first are marked tune. Times are in
-- ticks, 60 a second.

component "Fighter" {
  doc = "A player: moves, dashes, and swings by its slot's actions (sim.move, sim.attack)",
  speed = field.f32 { default = 7.0, range = {0, 50}, tune = true, doc = "Walking speed, units a second" },
  dash_speed = field.f32 { default = 24.0, range = {0, 100}, tune = true, doc = "Dashing speed, units a second" },
  dash_ticks = field.u8 { default = 10, range = {1, 120}, tune = true, doc = "How long a dash lasts; nothing hits the fighter meanwhile" },
  dash_every = field.u8 { default = 45, range = {1, 255}, tune = true, doc = "Ticks from the start of one dash to the next" },
  swing_ticks = field.u8 { default = 8, range = {1, 60}, tune = true, doc = "How long a swing's hitbox is out" },
  swing_every = field.u8 { default = 30, range = {1, 255}, tune = true, doc = "Ticks from one swing to the next" },
  reach = field.f32 { default = 0.8, range = {0, 5}, tune = true, doc = "How far in front of the fighter a swing's middle is, units" },
  radius = field.f32 { default = 0.7, range = {0.1, 5}, tune = true, doc = "A swing's radius: it hits a fighter whose middle is within this and half a body of it" },
  knockback = field.f32 { default = 14.0, range = {0, 100}, tune = true, doc = "How fast a hit pushes the one hit away, units a second (it slows each tick)" },
  home = field.vec2 { doc = "Where it starts each round" },
  facing = field.vec2 { default = {1, 0}, doc = "The way it last moved, one unit long: dashes and swings go this way" },
  dash = field.u8 { default = 0, doc = "Ticks of dash left, this one included (written by sim.move)" },
  dash_wait = field.u8 { default = 0, doc = "Ticks until it can dash again (written by sim.move)" },
  swing_wait = field.u8 { default = 0, doc = "Ticks until it can swing again (written by sim.attack)" },
  push = field.vec2 { doc = "The knockback it is moving by, units a second (written by sim.attack, slowed by sim.move)" },
  flash = field.u8 { default = 0, doc = "Ticks left that it shows it was hit (written by sim.attack)" },
}

component "Swing" {
  doc = "A swing's hitbox, a circle in front of its fighter for a few ticks (sim.attack spawns it from prefabs/swing.toml)",
  owner = field.entity { doc = "The fighter swinging" },
  dir = field.vec2 { default = {1, 0}, doc = "The way it swings" },
  ttl = field.u8 { default = 0, doc = "Ticks left after this one" },
}

component "Slash" {
  doc = "A swing's slash as it is drawn, presentation only (late.looks spawns it from prefabs/slash.toml on a swing event): it sweeps while the swing's hitbox is out, then stays, fading, so a swing learned of late online still shows",
  owner = field.entity { doc = "The fighter swinging" },
  dir = field.vec2 { default = {1, 0}, doc = "The way it swings" },
  age = field.u8 { default = 0, doc = "Ticks since its swing began" },
}

return component "Match" {
  doc = "The match, on @arena: the score, first to `to` points, and where the round is (sim.round)",
  to = field.u8 { default = 3, range = {1, 99}, tune = true, doc = "Points that win the match" },
  pause = field.u16 { default = 60, range = {0, 600}, tune = true, doc = "Ticks the round stands still after a point, and the win screen before a press starts a new match" },
  one = field.u8 { default = 0, doc = "Player 1's points" },
  two = field.u8 { default = 0, doc = "Player 2's points" },
  state = field.str { default = "menu", doc = "menu (the title menu shows), start (a match begins on the next tick), fight, point (one was scored: wait, then a new round), won (the win screen)" },
  wait = field.u16 { default = 0, doc = "Ticks left of a pause (written by sim.round)" },
  winner = field.u8 { default = 0, doc = "The slot that won, 0 until one has" },
  online = field.bool { default = false, doc = "The match is played online: q:net() says playing (written by input.menu)" },
}
