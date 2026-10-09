-- The dasher's looks. Its rules are sim.dash (Rust, plugins/swarm-hot),
-- which also swells it through its wind-up on Transform.scale. Here each
-- phase gets its colour on the tick sim.dash's event says it began, so this
-- runs for the few dashers that changed, never for the whole swarm.
local LOOKS = {
  { "dash.windup", "#FFE14D" }, -- yellow: about to charge
  { "dash.charge", "#FF3355" }, -- red: charging
  { "dash.recover", "#6B4C8F" }, -- dim: standing, spent
  { "dash.recovered", "#B45CFF" }, -- violet: walking with the swarm, as sim.waves spawns it
}

return system "sim.dash_look" {
  stage = "sim",
  after = { "sim.dash" },
  reads = { "Dasher" },
  writes = { "Sprite" },
  doc = "Colours a dasher by its phase on the tick sim.dash says it began: yellow winding up, red charging, dim recovering, violet walking.",
  run = function(q)
    for _, look in ipairs(LOOKS) do
      for _, ev in ipairs(q:events(look[1])) do
        ev.entity:get("Sprite").color = look[2]
      end
    end
  end,
}
