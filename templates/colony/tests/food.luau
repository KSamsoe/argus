-- A scenario: the game run headless, and what must be true after. `argus
-- test` runs every file in tests/, each seed a fresh load of the scene.
--
-- The core loop: in the first day the colonists gather the ripe bushes and
-- fell the trees (only the woodcutters), haul it all to the stockpile, eat
-- from it, and still end the day with more food than they began with. The
-- HUD says so.
return scenario "food and wood in the stockpile rise in the first day" {
  scene = "main", seeds = range(1, 3),
  run = function(t)
    local food = t.expect:field("@colony", "Stockpile.food"):get()
    t:run_until { sim_time = days(1) + 1 }
    t.expect:events("job.done", { kind = "gather" }):exists()
    t.expect:events("job.done", { kind = "chop" }):exists()
    t.expect:events("colony.stored", { gives = "food" }):exists()
    t.expect:events("colony.stored", { gives = "wood" }):exists()
    t.expect:events("colonist.ate"):count():gte(4)
    t.expect:field("@colony", "Stockpile.food"):gt(food)
    t.expect:field("@colony", "Stockpile.wood"):gt(0)
    -- Chopping needs the chop skill: Ada and Cleo have none.
    for _, who in ipairs({ "@colonist.ada", "@colonist.cleo" }) do
      t.expect:events("job.claimed", { kind = "chop", worker = who }):count():eq(0)
    end
    -- The line on screen: the day, and the food.
    local line = t:call("world.get", { entity = "@hud" }).components.UiText.text
    t:log(line)
    if not string.find(line, "Day 2", 1, true) or not string.find(line, "food", 1, true) then
      error("the HUD shows the day and the food: " .. line, 0)
    end
  end,
}
