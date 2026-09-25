-- The menu's buttons (phase 3 item 1): Resume closes it; New run closes it
-- and starts again on a new first floor, drawn on this same turn.
return system "sim.menu_buttons" {
  stage = "sim",
  after = { "ui.input" },
  unfrozen = true,
  reads = { "UiNode", "Player", "Health", "Floor", "Turn" },
  writes = { "UiNode", "Player", "Health", "Floor", "Turn" },
  doc = "Closes the menu on Resume; on New run also resets the hero and the floor count, so sim.floor makes a new first floor.",
  run = function(q)
    local resume = #q:events("menu.resume") > 0
    local new_run = #q:events("menu.new_run") > 0
    if not (resume or new_run) then return end
    q:entity("@menu"):get("UiNode").visible = false
    q:emit("menu.closed", {})
    if new_run then
      local pe = q:entity("@player")
      local pl = pe:get("Player")
      pl.floor = 0
      pl.kills = 0
      pl.dead = false
      pl.descend = false
      local hp = pe:get("Health")
      hp.cur = hp.max
      q:entity("@floor"):get("Floor").n = 0
      q:entity("@turn"):get("Turn").n = 0
      q:emit("run.new", {})
    end
  end,
}
