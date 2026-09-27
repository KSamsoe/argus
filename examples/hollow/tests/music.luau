-- Music (phase 3 item 7): each part of the hollow has its track, chosen by
-- the hero's room; a change waits for the next bar of the track playing
-- and starts on it; intensity brings layers in (up the shaft, down the
-- deep). All of it is in the event log and audio.status: the human hears
-- it, the agent reads it.

local function set(t, entity, path, value)
  local tx = t:call("edit.begin", { author = "test:hollow", note = "the music" }).tx
  t:call("edit.set", { tx = tx, entity = entity, path = path, value = value })
  t:call("edit.commit", { tx = tx })
end

local function music(t)
  return t:call("audio.status", {}).music
end

local function last(t, name)
  local ev = t:call("obs.events", { name = name, limit = 1000 })
  return ev.rows[#ev.rows]
end

-- Step a tick at a time until the track sounds; the ticks it took.
local function until_started(t, track)
  local before = t:call("obs.events", { name = "music.started", limit = 1000 }).total
  for n = 1, 600 do
    t:step(1)
    local ev = t:call("obs.events", { name = "music.started", limit = 1000 })
    if ev.total > before and ev.rows[#ev.rows].fields.track == track then return n end
  end
  error(track .. " never started", 0)
end

return scenario "each part of the hollow has its music, and a change waits for the bar" {
  scene = "hollow", seed = 1,
  run = function(t)
    t:step(2)
    local m = music(t)
    if m == nil then error("the cave's music plays", 0) end
    if m.track ~= "cave" or m.bpm ~= 80 then error("the cave: " .. tostring(m.track) .. " at " .. tostring(m.bpm), 0) end

    -- Into the hall: the change is asked for and waits for the cave's bar
    -- (3 s at 80 beats a minute); it starts on it, within a frame.
    set(t, "@player", "Transform.pos", { 60, 1.8 })
    t:step(2)
    local q = last(t, "music.queued")
    if q == nil then error("the change to the hall is queued for the bar", 0) end
    if q.fields.track ~= "hall" or q.fields.from ~= "cave" or q.fields.on ~= "bar" then
      error("queued: " .. tostring(q.fields.track) .. " from " .. tostring(q.fields.from), 0)
    end
    local in_ms = q.fields.in_ms
    if in_ms <= 0 or in_ms > 3000 then error("within a bar: " .. in_ms, 0) end
    until_started(t, "hall")
    local took_ms = (last(t, "music.started").frame - q.frame) * 1000 / 60
    t:log(string.format("into the hall: asked for %.0f ms before the cave's bar; the hall started %.0f ms later", in_ms, took_ms))
    if math.abs(took_ms - in_ms) > 1000 / 60 + 1 then
      error("the hall started on the bar: " .. took_ms .. " ms against " .. in_ms, 0)
    end
    t.expect:events("music.stopped", { track = "cave" }):count():eq(1)
    m = music(t)
    if m.sounding ~= "hall" or m.bpm ~= 100 or m.bar ~= 1 then
      error("the hall plays from its first bar: " .. tostring(m.sounding) .. " bar " .. tostring(m.bar), 0)
    end

    -- Seven rooms down the deep: its drone, and its line in.
    set(t, "@player", "Transform.pos", { -(3 + 1) * 24 + 5, -14 * 7 + 1.8 })
    t:step(2)
    until_started(t, "deep")
    t:step(2)
    t.expect:field("@music", "Music.intensity"):near(7 / 9, 0.001)
    t.expect:events("music.layer", { track = "deep", layer = "music/deep.line", on = true }):count():eq(1)

    -- Up the shaft: the cave again, with its high line in.
    set(t, "@player", "Transform.pos", { 30, 20.8 })
    t:step(2)
    until_started(t, "cave")
    t:step(2)
    t.expect:field("@music", "Music.intensity"):gt(0.8)
    t.expect:events("music.layer", { track = "cave", layer = "music/cave.high", on = true }):exists()
    -- The agent reads where the music is.
    m = music(t)
    t:log("audio.status: " .. m.sounding .. " at " .. m.bpm .. " bpm, bar " .. m.bar .. ", beat " .. m.beat)
  end,
}
