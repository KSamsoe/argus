-- Section 17's Outpost numbers: "sim tick at 10x speed, 200 colonists,
-- 20k items, under 1.6 ms: ten ticks per render frame with pathfinding,
-- jobs, and needs all running". The stress scene founds 200 colonists and
-- scatters 20,000 stones for them to haul on a 96 x 64 valley. The number
-- itself is measured in a release build (docs/phase3-checklist.md); a
-- debug build is held to a loose bound, so a slide is still caught.
return scenario "200 colonists and 20,000 items" {
  scene = "stress", seed = 1,
  run = function(t)
    t.expect:count({ has = { "Colonist", "Worker", "NavAgent" } }):eq(200)
    t.expect:count({ has = { "Item" } }):eq(20000)
    t:run_until { frames = 900 }
    t.expect:events("job.done", { kind = "haul" }):exists()
    t.expect:count({ has = { "Colonist" } }):eq(200)
    local busy = t:call("world.query", { select = { "count()" }, where = { has = { "Worker" }, ["Worker.idle"] = "" } })
    t:log(busy.rows[1][1] .. " of 200 at work")
    t.expect:perf("tick"):p95_ms():lt(12.0)
  end,
}
