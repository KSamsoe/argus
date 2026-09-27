-- The valley's ambience (phase 3 item 7): the day track by day, the night
-- track after dusk, each change on the bar of the track playing; the birds
-- are loudest at dawn and gone by late afternoon. The agent reads it in the
-- log and audio.status.

local function last(t, name)
  local ev = t:call("obs.events", { name = name, limit = 1000 })
  return ev.rows[#ev.rows]
end

-- The mixer's answers reach the log when a step ends, so step in short
-- runs until one comes (run_until watches what systems emit).
local function until_logged(t, name, ticks)
  local before = t:call("obs.events", { name = name, limit = 1000 }).total
  for _ = 1, math.ceil(ticks / 10) do
    t:step(10)
    if t:call("obs.events", { name = name, limit = 1000 }).total > before then return end
  end
  error(name .. " never came", 0)
end

-- Step a tick at a time until the track sounds.
local function until_started(t, track)
  local before = t:call("obs.events", { name = "music.started", limit = 1000 }).total
  for _ = 1, 600 do
    t:step(1)
    local ev = t:call("obs.events", { name = "music.started", limit = 1000 })
    if ev.total > before and ev.rows[#ev.rows].fields.track == track then return end
  end
  error(track .. " never started", 0)
end

return scenario "the valley's ambience follows the day and turns at dusk on the bar" {
  scene = "valley", seed = 1,
  run = function(t)
    t:step(40)
    local m = t:call("audio.status", {}).music
    if m == nil or m.track ~= "day" or m.bpm ~= 60 then error("the day's ambience", 0) end
    t.expect:events("music.started", { track = "day" }):count():eq(1)
    -- Morning: the birds sing; by late afternoon they have stopped.
    t.expect:field("@colony", "Music.intensity"):gt(0.9)
    t.expect:events("music.layer", { track = "day", layer = "amb/birds", on = true }):exists()
    until_logged(t, "music.layer", 60 * 60)
    local off = last(t, "music.layer")
    if off.fields.layer ~= "amb/birds" or off.fields.on ~= false then error("the birds stop before dusk", 0) end
    t:log(string.format("the birds stop %.1f s into the day", off.frame / 60))
    -- Dusk: night is asked for, and starts on the day's next bar (4 s).
    local dusk = t.expect:field("@colony", "Colony.dusk"):get()
    until_logged(t, "music.queued", 60 * 60)
    local q = last(t, "music.queued")
    if q.fields.track ~= "night" or q.fields.from ~= "day" or q.fields.on ~= "bar" then
      error("at dusk: " .. tostring(q.fields.track) .. " from " .. tostring(q.fields.from), 0)
    end
    until_started(t, "night")
    local took = (last(t, "music.started").frame - q.frame) * 1000 / 60
    t:log(string.format("dusk at %.2f of the day: night asked for %.0f ms before the day's bar, started %.0f ms later", dusk, q.fields.in_ms, took))
    if math.abs(took - q.fields.in_ms) > 1000 / 60 + 1 then error("night started on the bar", 0) end
    t.expect:events("music.stopped", { track = "day" }):count():eq(1)
    -- Dawn: the day again.
    t:run_until { event = "day.turned", max_ticks = 60 * 60 }
    until_started(t, "day")
    t.expect:events("music.started", { track = "day" }):count():eq(2)
  end,
}
