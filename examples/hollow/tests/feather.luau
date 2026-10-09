-- The feather (a double jump), early in the cave: on the plank over the
-- hermit's fire. Without it, jump pressed again in mid-air does nothing
-- (the controller only remembers the press for landing). Taken, as a
-- player takes it, running under the plank's far end and jumping up
-- through it, a second press in mid-air jumps the hero again, once, and
-- higher than one jump goes; the jump in mid-air is back each time they
-- land, and the HUD says they have it. It is the run's: a fall into the
-- pit keeps it, a new run takes it away and puts the feather back, even
-- when the cave had streamed out.

local function set(t, entity, path, value)
  local tx = t:call("edit.begin", { author = "test:hollow", note = "the feather" }).tx
  t:call("edit.set", { tx = tx, entity = entity, path = path, value = value })
  t:call("edit.commit", { tx = tx })
end

local function y(t)
  return t.expect:field("@player", "Transform.pos.y"):get()
end

local function rising(t)
  return t.expect:field("@player", "KinematicBody.velocity.y"):get()
end

-- Jump from where the hero stands, holding it for a full jump, and press
-- jump again (held as long) at each of `presses` ticks after the take-off;
-- then come down. Returns how high above the take-off the hero got, and
-- how fast they rose (below 0: fell) just after each press.
local function jump(t, presses)
  local from = y(t)
  local top, after, tick = from, {}, 0
  local function step()
    t:step(1)
    tick += 1
    top = math.max(top, y(t))
  end
  t.input:hold("jump", true, 22)
  for _, at in ipairs(presses) do
    while tick < at do step() end
    t.expect:field("@player", "KinematicBody.grounded"):eq(false)
    t.input:hold("jump", true, 22)
    step()
    table.insert(after, rising(t))
  end
  -- Down again, onto what they took off from.
  for _ = 1, 180 do
    step()
    if t.expect:field("@player", "KinematicBody.grounded"):get() then break end
  end
  t:step(2)
  return top - from, after
end

-- From the cave mouth: run right under the plank's far end, where the
-- feather is, and jump up through the plank onto it.
local function take_the_feather(t)
  t.input:hold("move", { 1, 0 }, 70)
  t:step(90)
  t.expect:field("@player", "Transform.pos.x"):between(10.8, 11.6)
  t.input:hold("jump", true, 22)
  t:run_until { event = "feather.taken", max_frames = 40 }
  t:step(40)
  t.expect:field("@player", "KinematicBody.on_one_way"):eq(true)
end

scenario "the feather gives one more jump in mid-air, back each time the hero lands" {
  scene = "hollow", seeds = { 1, 2, 3 },
  run = function(t)
    t:step(5)
    t.expect:field("@player", "AirJump.max"):eq(0)
    t.expect:field("@feather", "Sprite.visible"):eq(true)
    t.expect:field("@feather", "Feather.taken"):eq(false)

    -- Without it: pressed again at the top of a jump, nothing; the hero
    -- comes down as from one jump.
    local single, plain = jump(t, { 24 })
    t:log(string.format("without the feather: %.2f up; pressed again at the top, falling at %.2f", single, plain[1]))
    if plain[1] >= 0 then error("without the feather a press in mid-air lifts the hero: " .. plain[1], 0) end
    t.expect:events("hero.air_jump"):count():eq(0)
    t.expect:field("@player", "Transform.pos.y"):near(1.8, 0.01)

    -- The feather, taken as a player takes it; the HUD says what it gives.
    take_the_feather(t)
    t.expect:events("feather.taken"):count():eq(1)
    t.expect:events("sound.played", { sound = "sfx/feather" }):count():eq(1)
    t.expect:field("@player", "AirJump.max"):eq(1)
    t.expect:field("@player", "AirJump.left"):eq(1)
    t.expect:field("@feather", "Feather.taken"):eq(true)
    t.expect:field("@feather", "Sprite.visible"):eq(false)
    t.expect:field("@hud", "UiText.shown"):eq("Hollow. Find the way out. No falls yet. The feather: jump again in mid-air.")

    -- With it, from the plank: pressed again at the top, the hero jumps
    -- again, as from the ground; a third press, on the way down, nothing.
    local double, winged = jump(t, { 24, 49 })
    t:log(string.format("with the feather: %.2f up (one jump: %.2f); rising at %.2f after the second press, %.2f after the third",
      double, single, winged[1], winged[2]))
    if winged[1] < 15 then error("the second press jumps the hero again: rising at " .. winged[1], 0) end
    if winged[2] >= 0 then error("a third press in mid-air lifts the hero: " .. winged[2], 0) end
    if double < single + 2.5 then error(string.format("two jumps go higher than one: %.2f against %.2f", double, single), 0) end
    t.expect:events("hero.air_jump"):count():eq(1)
    t.expect:events("sound.played", { sound = "sfx/flap" }):count():eq(1)

    -- Landed (on the plank again): the jump in mid-air is back, and goes
    -- as high again.
    t.expect:field("@player", "KinematicBody.on_one_way"):eq(true)
    t.expect:field("@player", "AirJump.left"):eq(1)
    local again = jump(t, { 24 })
    t.expect:events("hero.air_jump"):count():eq(2)
    if math.abs(again - double) > 0.05 then error(string.format("as high again: %.2f, then %.2f", double, again), 0) end
    t.expect:field("@player", "AirJump.left"):eq(1)
    local se = t:call("script.errors", {})
    if se.total ~= 0 then error("script errors: " .. tostring(se.rows[1].message), 0) end
  end,
}

return scenario "the feather lasts the run: a fall keeps it, a new run puts it back" {
  scene = "hollow", seeds = { 1, 2, 3 },
  run = function(t)
    t:step(5)
    take_the_feather(t)
    t.expect:field("@player", "AirJump.max"):eq(1)

    -- Into the hall's pit: back at the cave mouth, a fall counted, and the
    -- jump in mid-air still theirs.
    set(t, "@player", "Transform.pos", { 58, 3 })
    t:run_until { event = "player.fell", max_frames = 120 }
    t:step(30)
    t.expect:field("@hud", "Run.falls"):eq(1)
    t.expect:field("@player", "AirJump.max"):eq(1)
    t.expect:field("@feather", "Sprite.visible"):eq(false)
    local _, after = jump(t, { 24 })
    if after[1] < 15 then error("after a fall the feather's jump is still there: " .. after[1], 0) end
    t.expect:events("hero.air_jump"):count():eq(1)

    -- Far down the deep, the cave streams out, remembering its feather
    -- taken; a new run (R) brings the hero back to a feather to take.
    set(t, "@player", "Transform.pos", { -259, -82.2 })
    t:step(5)
    t.expect:field("@player", "InRoom.room"):eq("deep-10-6")
    t.expect:count({ has = { "Feather" } }):eq(0)
    t.input:hold("restart", true, 2)
    t:step(30)
    t.expect:events("run.restarted"):count():eq(1)
    t.expect:field("@player", "InRoom.room"):eq("a")
    t.expect:field("@player", "AirJump.max"):eq(0)
    t.expect:field("@feather", "Feather.taken"):eq(false)
    t.expect:field("@feather", "Sprite.visible"):eq(true)
    t.expect:field("@hud", "UiText.shown"):eq("Hollow. Find the way out. No falls yet.")

    -- The new run has no jump in mid-air until the feather is taken again.
    local _, plain = jump(t, { 24 })
    if plain[1] >= 0 then error("a new run has no jump in mid-air: " .. plain[1], 0) end
    t.expect:events("hero.air_jump"):count():eq(1)
    take_the_feather(t)
    t.expect:events("feather.taken"):count():eq(2)
    t.expect:field("@player", "AirJump.max"):eq(1)
  end,
}
