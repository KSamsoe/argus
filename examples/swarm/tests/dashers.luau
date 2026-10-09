-- The dasher (sim.dash in plugins/swarm-hot, its looks systems/dash_look.luau):
-- from Director.dash_from on (wave 2), a share of every wave are dashers.
-- A dasher walks with the swarm, then stands and swells yellow (the
-- wind-up), charges red along the line it fixed when the wind-up began,
-- through where the player was then, and stands again to recover. A player
-- who steps aside during the wind-up is missed; one who stands still is
-- run through.

local WINDUP = 40 -- Dasher.windup's default, ticks
local RECOVER = 40 -- Dasher.recover's default, ticks
local OVERSHOOT = 3.0 -- Dasher.overshoot's default, units
local RANGE = 14.0 -- Dasher.range's default, units
-- Dasher.state
local WALKING, WINDING_UP, CHARGING, RECOVERING = 0, 1, 2, 3
-- The looks: walking, winding up, charging, recovering.
local VIOLET, YELLOW, RED, DIM = "#B45CFF", "#FFE14D", "#FF3355", "#6B4C8F"

local function get(t, id)
  return t:call("world.get", { entity = id }).components
end

local function events(t, name)
  return t:call("obs.events", { name = name, limit = 10000 }).rows
end

-- Fails the run, naming the line that called it.
local function fail(msg)
  error(msg, 2)
end

local function dist(a, b)
  return math.sqrt((a[1] - b[1]) ^ 2 + (a[2] - b[2]) ^ 2)
end

local function fmt(a)
  return string.format("(%.3f, %.3f)", a[1], a[2])
end

-- Where p is against the line from a along the unit (dx, dy): how far
-- along it, and how far off it.
local function on_line(p, a, dx, dy)
  local rx, ry = p[1] - a[1], p[2] - a[2]
  return rx * dx + ry * dy, math.abs(rx * dy - ry * dx)
end

scenario "dashers come with the waves from wave 2, and every charge follows a wind-up" {
  scene = "arena", seeds = range(1, 4),
  run = function(t)
    -- Wave 1: the swarm as it always was, no dashers.
    t:run_until { event = "wave.spawned", max_frames = 5 }
    t.expect:count({ has = { "Enemy" } }):eq(200)
    t.expect:count({ has = { "Dasher" } }):eq(0)
    t.expect:events("wave.spawned", { wave = 1, dashers = 0 }):count():eq(1)
    -- Wave 2: a tenth of it dashers, violet and three bullets tough.
    t:run_until { event = "wave.cleared", max_frames = 1500 }
    t:run_until { event = "wave.spawned", max_frames = 100 }
    t.expect:events("wave.spawned", { wave = 2, count = 400, dashers = 40 }):count():eq(1)
    t.expect:count({ has = { "Enemy" } }):eq(400)
    t.expect:count({ has = { "Dasher", "Enemy" } }):eq(40)
    t.expect:count({ has = { "Dasher" }, ["Health.max"] = 6 }):eq(40)
    t.expect:count({ has = { "Dasher" }, ["Sprite.color"] = VIOLET }):eq(40)
    t.expect:count({ has = { "Dasher" }, ["Dasher.state"] = WALKING }):eq(40)
    -- Wave 3 too.
    t:run_until { event = "wave.cleared", max_frames = 2000 }
    t:run_until { event = "wave.spawned", max_frames = 100 }
    t.expect:events("wave.spawned", { wave = 3, count = 800, dashers = 80 }):count():eq(1)
    t.expect:count({ has = { "Dasher" } }):eq(80)
    -- Every charge so far came a wind-up after the dasher's wind-up began,
    -- from the very spot it began on (it stood), at the spot fixed then.
    local windups = {}
    for _, r in ipairs(events(t, "dash.windup")) do
      windups[r.fields.entity .. "@" .. r.frame] = r.fields
    end
    local charges = events(t, "dash.charge")
    if #charges < 10 then fail("only " .. #charges .. " charges by wave 3") end
    for _, r in ipairs(charges) do
      local w = windups[r.fields.entity .. "@" .. (r.frame - WINDUP)]
      if w == nil then
        fail(string.format("%s charged on frame %d without a wind-up on frame %d", r.fields.entity, r.frame, r.frame - WINDUP))
      end
      if dist(w.at, r.fields.at) > 1e-5 then
        fail(string.format("%s moved during its wind-up: %s to %s", r.fields.entity, fmt(w.at), fmt(r.fields.at)))
      end
      if dist(w.target, r.fields.target) > 1e-5 then
        fail(string.format("%s's target moved during its wind-up", r.fields.entity))
      end
    end
    -- And the dashers of this moment look their phase.
    local looks = { [WALKING] = VIOLET, [WINDING_UP] = YELLOW, [CHARGING] = RED, [RECOVERING] = DIM }
    for state, color in pairs(looks) do
      local n = t:call("world.query", { select = { "count()" }, where = { has = { "Dasher" }, ["Dasher.state"] = state } }).rows[1][1]
      t.expect:count({ has = { "Dasher" }, ["Dasher.state"] = state, ["Sprite.color"] = color }):eq(n)
    end
  end,
}

-- One dasher alone in wave 1, too tough to shoot down, so its whole dash
-- is seen. The player walks up for a second first, so the spot it charges
-- at is not the origin. Returns the dasher and its wind-up event's fields,
-- the tick the wind-up began on having just run.
local function lone_dasher(t)
  t:call("sim.set", { entity = "@director", path = "Director.first", value = 1 })
  t:call("sim.set", { entity = "@director", path = "Director.dash_from", value = 1 })
  t:call("sim.set", { entity = "@director", path = "Director.dash_share", value = 1 })
  t:step(1)
  local rows = t:call("world.query", { select = { "id" }, where = { has = { "Dasher" } } }).rows
  if #rows ~= 1 then fail("wave 1 is " .. #rows .. " dashers, not one") end
  local id = rows[1][1]
  t.expect:count({ has = { "Enemy" } }):eq(1)
  t:call("sim.set", { entity = id, path = "Health.cur", value = 100000 })
  t.input:hold("move", { 0, 1 }, 60)
  t:step(60)
  -- It walks at the player like the rest of the swarm until it is in range.
  local d0 = dist(get(t, id).Transform.pos, get(t, "@player").Transform.pos)
  t:step(30)
  local d1 = dist(get(t, id).Transform.pos, get(t, "@player").Transform.pos)
  if d1 >= d0 - 1 then fail(string.format("the dasher does not walk at the player: %.2f then %.2f", d0, d1)) end
  t:run_until { event = "dash.windup", max_frames = 1500 }
  local w = events(t, "dash.windup")
  if #w ~= 1 then fail(#w .. " wind-ups") end
  w = w[1].fields
  if w.entity ~= id then fail("the wind-up is " .. tostring(w.entity) .. "'s, not " .. id .. "'s") end
  local c, pl = get(t, id), get(t, "@player")
  -- The spot is where the player is as the wind-up begins, within range.
  if dist(w.target, pl.Transform.pos) > 1e-5 then
    fail("the wind-up's target " .. fmt(w.target) .. " is not the player's spot " .. fmt(pl.Transform.pos))
  end
  if dist(w.at, w.target) > RANGE + 1e-3 then fail("wound up out of range") end
  if c.Dasher.state ~= WINDING_UP or not c.Enemy.hold then fail("not held in its wind-up: state " .. c.Dasher.state) end
  if c.Sprite.color ~= YELLOW then fail("the wind-up does not show: " .. c.Sprite.color) end
  return id, w
end

-- The charge's line: from the wind-up's start toward its target.
local function line(w)
  local len = dist(w.at, w.target)
  return (w.target[1] - w.at[1]) / len, (w.target[2] - w.at[2]) / len, len
end

scenario "a dasher winds up, charges along a fixed line, and misses a player who steps aside" {
  scene = "arena", seeds = range(1, 5),
  run = function(t)
    local id, w = lone_dasher(t)
    local dx, dy, len = line(w)
    -- The player steps aside, square to the line, all through the wind-up.
    t.input:hold("move", { -dy, dx }, WINDUP)
    -- The wind-up: it stands where it was and swells.
    local size = get(t, id).Transform.scale[1]
    for i = 1, WINDUP - 1 do
      t:step(1)
      local c = get(t, id)
      if c.Dasher.state ~= WINDING_UP then fail("the wind-up ended after " .. i .. " ticks") end
      if dist(c.Transform.pos, w.at) > 1e-6 then fail("it moved during its wind-up, tick " .. i) end
      if c.Transform.scale[1] <= size then fail("it does not swell, tick " .. i) end
      size = c.Transform.scale[1]
    end
    if size < 1.5 then fail(string.format("swelled only to %.2f", size)) end
    t:step(1)
    -- The charge: red, at the spot fixed when the wind-up began, though
    -- the player has gone from it.
    local c = get(t, id)
    if c.Dasher.state ~= CHARGING then fail("no charge after the wind-up: state " .. c.Dasher.state) end
    if c.Sprite.color ~= RED then fail("the charge does not show: " .. c.Sprite.color) end
    local ch = events(t, "dash.charge")
    if #ch ~= 1 or dist(ch[1].fields.target, w.target) > 1e-6 then fail("the charge is not at the wind-up's spot") end
    local away = dist(get(t, "@player").Transform.pos, w.target)
    if away < 3.5 then fail(string.format("the player stepped only %.2f aside", away)) end
    local walk = c.Enemy.speed / 60
    -- Every tick of it on the line, onward, many times its walk.
    local last, ticks = 0, 0
    while c.Dasher.state == CHARGING do
      local along, off = on_line(c.Transform.pos, w.at, dx, dy)
      if off > 1e-3 then fail(string.format("off its line by %.5f on charge tick %d", off, ticks)) end
      if along - last < 5 * walk then fail(string.format("charge tick %d went %.3f, its walk goes %.3f", ticks, along - last, walk)) end
      last = along
      ticks += 1
      if ticks > 120 then fail("the charge does not end") end
      t:step(1)
      c = get(t, id)
    end
    -- It ran through the spot and overshoot beyond it, and stops there.
    if c.Dasher.state ~= RECOVERING then fail("after the charge: state " .. c.Dasher.state) end
    if c.Sprite.color ~= DIM then fail("the recovery does not show: " .. c.Sprite.color) end
    local along, off = on_line(c.Transform.pos, w.at, dx, dy)
    if off > 1e-3 or math.abs(along - (len + OVERSHOOT)) > 1e-3 then
      fail(string.format("the charge ended %.3f along and %.4f off; the spot was %.3f along", along, off, len))
    end
    t.expect:events("dash.recover"):count():eq(1)
    -- Missed: the player was never touched.
    t.expect:events("damage"):count():eq(0)
    t.expect:field("@player", "Health.cur"):eq(10)
    -- It recovers standing, then walks with the swarm again, violet.
    local at = c.Transform.pos
    t:step(RECOVER - 1)
    c = get(t, id)
    if c.Dasher.state ~= RECOVERING or dist(c.Transform.pos, at) > 1e-6 then fail("it does not stand to recover") end
    t:step(1)
    t.expect:events("dash.recovered"):count():eq(1)
    t.expect:field(id, "Dasher.state"):eq(WALKING)
    t.expect:field(id, "Enemy.hold"):eq(false)
    t.expect:field(id, "Sprite.color"):eq(VIOLET)
    t.expect:field(id, "Transform.scale.x"):near(1.0, 1e-6)
    local d0 = dist(get(t, id).Transform.pos, get(t, "@player").Transform.pos)
    t:step(30)
    local d1 = dist(get(t, id).Transform.pos, get(t, "@player").Transform.pos)
    if d1 >= d0 - 1 then fail(string.format("it does not walk again: %.2f then %.2f", d0, d1)) end
  end,
}

scenario "a player who stands still through the wind-up is hit by the charge" {
  scene = "arena", seeds = range(1, 5),
  run = function(t)
    local id, w = lone_dasher(t)
    t.expect:events("damage"):count():eq(0)
    t:run_until { event = "dash.recover", max_frames = 200 }
    local charged = events(t, "dash.charge")[1].frame
    local recovered = events(t, "dash.recover")[1].frame
    local hit = false
    for _, r in ipairs(events(t, "damage")) do
      if r.fields.by == id and r.frame >= charged and r.frame <= recovered then hit = true end
    end
    if not hit then fail("the charge ran past a player standing on its spot") end
    t.expect:field("@player", "Health.cur"):lt(10)
  end,
}
