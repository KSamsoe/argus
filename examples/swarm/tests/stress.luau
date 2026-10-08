-- The systems a closed pause menu runs (phase 5 item 2): its own two look
-- at one flag, and the UI's walk its few hidden nodes.
local CLOSED = { "input.pause", "sim.pause_menu", "ui.input", "ui.layout", "input.rebind" }

return scenario "fifty thousand enemies tick" {
  scene = "stress", seed = 1,
  run = function(t)
    t:run_until { event = "wave.spawned", max_frames = 5 }
    t.expect:count({ has = {"Enemy"} }):eq(50000)
    t:run_until { frames = 30 }
    t.expect:count({ has = {"Enemy"} }):gte(49000)
    t.expect:field("@player", "Health.cur"):gte(1)
    -- The Luau ceiling (design section 19): the number itself is the finding.
    t.expect:perf("sim"):p95_ms():lt(300.0)
    -- The closed menu costs next to nothing: half a millisecond a debug
    -- tick at most, together, where the swarm's chase alone takes fifty.
    t.expect:field("@pause", "UiRect.shown"):eq(false)
    local p = t:call("prof.systems", { last = 30 })
    local closed = 0
    for _, row in ipairs(p.rows) do
      if table.find(CLOSED, row[1]) then closed += row[3] end
    end
    if closed >= 0.5 then error(string.format("the closed menu: %.3f ms a tick", closed), 0) end
  end,
}
