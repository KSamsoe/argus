-- The way out's outro (phase 3 item 5): a timeline in defs/timeline.toml.
-- The view cuts to a wide camera that glides in and pulls back over the
-- hall, the hermit calls after the hero and the outro waits for the line
-- to be read, then the hero's camera glides home. R skips it. It can be
-- scrubbed (timeline.seek, the editor's scrubber), and the farewell knows
-- about the gem.

local function set(t, entity, path, value)
  local tx = t:call("edit.begin", { author = "test:hollow", note = "the outro" }).tx
  t:call("edit.set", { tx = tx, entity = entity, path = path, value = value })
  t:call("edit.commit", { tx = tx })
end

-- The words the conversation's box shows, top to bottom.
local function box(t)
  local q = t:call("world.query", { select = { "UiRect.pos", "UiText.shown" }, where = { has = { "TalkPart", "UiText" } } })
  table.sort(q.rows, function(a, b) return a[1][2] < b[1][2] end)
  local out = {}
  for _, r in ipairs(q.rows) do table.insert(out, r[2]) end
  return table.concat(out, " | ")
end

-- From past the pit, run into the way out.
local function finish(t)
  set(t, "@player", "Transform.pos", { 62, 1.8 })
  t.input:hold("move", { 1, 0 }, 120)
  t:run_until { event = "run.finished", max_frames = 240 }
end

local function playing(t)
  return t:call("timeline.list", { name = "outro" }).playing[1]
end

-- A JSON-shaped value in one line, for a failure's message.
local function show(v, depth)
  depth = depth or 0
  if type(v) ~= "table" or depth > 3 then return tostring(v) end
  local parts = {}
  for k, x in pairs(v) do table.insert(parts, tostring(k) .. "=" .. show(x, depth + 1)) end
  return "{" .. table.concat(parts, ", ") .. "}"
end

scenario "the way out plays the outro: the hall from above, a farewell, home" {
  scene = "hollow", seed = 1,
  run = function(t)
    finish(t)
    t:step(1)
    t.expect:events("timeline.started", { row = "outro" }):count():eq(1)
    -- The wide camera draws, gliding in from the hero's view.
    t:step(10)
    t.expect:field("@wide", "Camera.active"):eq(true)
    t.expect:field("@wide", "Camera.drew"):eq(true)
    t.expect:field("@wide", "Camera.blend_left"):gt(0)
    t.expect:events("camera.cut", { blend = 1 }):count():eq(1)
    -- Pulled back over the hall.
    t:step(150)
    t.expect:field("@wide", "Camera.height"):near(14, 0.01)
    t.expect:field("@wide", "Transform.pos.x"):near(58, 0.01)
    -- Three seconds in, the hermit calls after the hero, and the outro
    -- waits until the line is read.
    t:run_until { event = "timeline.held", max_frames = 60 }
    t:step(2)
    local b = box(t)
    t:log(b)
    if b ~= "The hermit | You found the way out. Go well, and come back some time. | Continue" then
      error("the farewell: " .. b, 0)
    end
    local at = playing(t)
    t:step(60)
    local now = playing(t)
    if not now.held or now.time ~= at.time then
      error("held while the line is up: " .. at.time .. " then " .. now.time, 0)
    end
    t.input:tap("ui_confirm")
    t:step(2)
    t.expect:count({ has = { "Talk" } }):eq(0)
    t.expect:events("timeline.resumed", { row = "outro" }):count():eq(1)
    -- Five seconds in it is over, and the hero's camera glides back.
    t:run_until { event = "timeline.ended", max_frames = 180 }
    t:step(1)
    t.expect:events("timeline.ended", { row = "outro", skipped = false }):count():eq(1)
    t.expect:field("@wide", "Camera.active"):eq(false)
    t.expect:field("@camera", "Camera.drew"):eq(true)
    t.expect:field("@camera", "Camera.blend_left"):gt(0)
    t.expect:count({ has = { "Timeline" } }):eq(0)

    -- Scrubbed: a second and a half in, half way pulled back; the replay
    -- is the run.
    local s = t:call("timeline.seek", { name = "outro", time = 1.5 })
    if not s.reached or s.diverged ~= nil then
      error("scrubbed to 1.5 s: reached " .. tostring(s.reached) .. ", diverged " .. show(s.diverged), 0)
    end
    local h = t.expect:field("@wide", "Camera.height"):get()
    if h <= 12.2 or h >= 13.8 then error("half way up at 1.5 s: " .. h, 0) end
    t:log(string.format("scrubbed to %.3f s: frame %d, the wide camera %.2f high", s.time, s.frame, h))
    -- To the farewell: the outro holds on it, the conversation begun.
    s = t:call("timeline.seek", { name = "outro", time = 3 })
    if not s.held or s.diverged ~= nil then error("at 3 s it waits on the line: " .. show(s), 0) end
    t.expect:count({ has = { "Talk" } }):eq(1)
    -- Back to the start: the wide camera just cut to.
    s = t:call("timeline.seek", { name = "outro", time = 0 })
    t.expect:field("@wide", "Camera.active"):eq(true)
    t.expect:count({ has = { "Talk" } }):eq(0)
  end,
}

return scenario "R skips the outro, and the farewell knows about the gem" {
  scene = "hollow", seed = 1,
  run = function(t)
    finish(t)
    t:step(30)
    t.expect:field("@wide", "Camera.active"):eq(true)
    t.input:hold("restart", true, 2)
    t:step(3)
    t.expect:events("timeline.ended", { row = "outro", skipped = true }):count():eq(1)
    t.expect:field("@wide", "Camera.active"):eq(false)
    -- (Back in the cave, the room change's fade plays; the outro is over.)
    t.expect:count({ has = { "Timeline" }, ["Timeline.name"] = "timeline:outro" }):eq(0)
    t.expect:count({ has = { "Talk" } }):eq(0)
    t.expect:field("@player", "Transform.pos.x"):near(3, 0.5)
    -- A skipped outro's tweens are at their ends.
    t.expect:field("@wide", "Camera.height"):near(14, 0.001)
    -- Again, with the gem found and kept: he says so.
    set(t, "@hud", "Run.gems", 1)
    finish(t)
    t:run_until { event = "timeline.held", max_frames = 240 }
    t:step(2)
    local b = box(t)
    if b ~= "The hermit | You found the way out, and a gem besides. Go well. | Continue" then
      error("with the gem kept: " .. b, 0)
    end
  end,
}
