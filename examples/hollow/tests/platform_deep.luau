-- The deep diver (phase 5 item 9): each deeper row of the deep is progress
-- shown on the platform, the bottom row the achievement; friends see the
-- room the hero is in.

local function set(t, entity, path, value)
  local tx = t:call("edit.begin", { author = "test:hollow", note = "the deep" }).tx
  t:call("edit.set", { tx = tx, entity = entity, path = path, value = value })
  t:call("edit.commit", { tx = tx })
end

-- Stand the hero in a deep room (the grid runs left from the cave, 24
-- wide, and down, 14 high), as the deep's own scenario does.
local function into(t, c, r)
  set(t, "@player", "Transform.pos", { -(c + 1) * 24 + 5, -14 * r + 1.8 })
  t:step(2)
end

return scenario "the deep's rows are the deep diver's progress, and friends see the room" {
  scene = "hollow", seed = 1,
  run = function(t)
    t:call("platform.reset", {})
    t:step(2)
    into(t, 0, 3)
    t.expect:events("achievement.progress", { id = "deep_diver", current = 3, max = 9 }):count():eq(1)
    local s = t:call("platform.status", {})
    if s.state.presence.status ~= "In the deep, 3 rooms down" then
      error("friends see: " .. tostring(s.state.presence.status), 0)
    end
    if s.state.progress.ACH_TRAVEL_FAR_ACCUM[1] ~= 3 then
      error("the platform's progress: " .. tostring(s.state.progress.ACH_TRAVEL_FAR_ACCUM), 0)
    end
    -- Back up a row is no progress; the bottom row is the achievement.
    into(t, 0, 2)
    t.expect:events("achievement.progress"):count():eq(1)
    into(t, 0, 9)
    t.expect:events("achievement.unlock", { id = "deep_diver" }):count():eq(1)
    t.expect:field("@hud", "Feats.deepest"):eq(9)
  end,
}
