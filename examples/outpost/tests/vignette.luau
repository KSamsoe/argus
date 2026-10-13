-- A colonist lost (phase 3 item 5): a grave where they fell, and the lost
-- timeline, an event vignette. The view glides to the grave and closes in,
-- then glides back to where the player left it, while the colony goes on.

local function set(t, entity, path, value)
  local tx = t:call("edit.begin", { author = "test:outpost", note = "a colonist lost" }).tx
  t:call("edit.set", { tx = tx, entity = entity, path = path, value = value })
  t:call("edit.commit", { tx = tx })
end

local function one(t, select, where)
  local q = t:call("world.query", { select = select, where = where })
  if #q.rows ~= 1 then error("one row for " .. select[1] .. ", not " .. #q.rows, 0) end
  return q.rows[1]
end

return scenario "a colonist lost: a grave, and the view goes there and back" {
  scene = "valley", seed = 1,
  run = function(t)
    t:step(10)
    local home = one(t, { "id" }, { has = { "Camera" } })[1]
    local ada = one(t, { "id" }, { ["Colonist.name"] = "Ada" })[1]
    set(t, ada, "Needs.fed", 0)
    t:run_until { event = "colonist.died", max_ticks = 10 }
    t:step(1)
    t.expect:events("system.fault"):count():eq(0)
    t.expect:events("timeline.started", { row = "lost" }):count():eq(1)
    local g = one(t, { "Grave.name", "Describe.text", "Transform.pos" }, { has = { "Grave" } })
    if g[1] ~= "Ada" or not string.find(g[2], "Ada lies here, starved on day 0", 1, true) then
      error("Ada's grave: " .. tostring(g[1]) .. ", " .. tostring(g[2]), 0)
    end
    t:log(g[2])
    -- The vignette camera draws, gliding in from the colony's view.
    local cam = one(t, { "id" }, { has = { "Vignette" } })[1]
    t.expect:field(cam, "Camera.drew"):eq(true)
    t.expect:field(cam, "Camera.target"):eq(one(t, { "id" }, { has = { "Grave" } })[1])
    t.expect:events("camera.cut", { to = cam }):count():eq(1)
    -- Closed in on the grave.
    t:step(150)
    t.expect:field(cam, "Camera.height"):near(9, 0.01)
    t.expect:field(cam, "Transform.pos.x"):near(g[3][1], 1.5)
    t.expect:field(cam, "Transform.pos.y"):near(g[3][2], 1.5)
    -- Back: the colony's camera glides home, and the vignette is over.
    t:run_until { event = "timeline.ended", max_ticks = 120 }
    t.expect:field(cam, "Camera.active"):eq(false)
    t.expect:field(home, "Camera.drew"):eq(true)
    t.expect:events("camera.cut", { to = home }):count():eq(1)
    t.expect:count({ has = { "Timeline" } }):eq(0)
    -- The colony went on beneath it; the news says who was lost.
    t.expect:events("colonist.died"):count():eq(1)
    t.expect:events("job.done"):exists()
    local notice = one(t, { "UiText.text" }, { ["Hud.kind"] = "notice" })[1]
    if notice ~= "Ada starved." then error("the notice: " .. tostring(notice), 0) end
  end,
}
