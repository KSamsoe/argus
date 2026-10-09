-- Hunger: colonists get hungry, walk to the stockpile and eat when they
-- are, and with the food they gather nobody starves in three days. Run
-- fast-forwarded: three days are 10,800 ticks a seed.
return scenario "colonists eat when hungry, and none starves in three days" {
  scene = "main", seeds = range(1, 3),
  run = function(t)
    t:run_until { sim_time = days(3), speed = "max" }
    t.expect:events("colonist.died"):count():eq(0)
    t.expect:count({ has = { "Colonist" } }):eq(4)
    -- Each ate, more than once a day, at an eat job that was theirs alone.
    for _, who in ipairs({ "@colonist.ada", "@colonist.bram", "@colonist.cleo", "@colonist.dov" }) do
      t.expect:events("colonist.ate", { colonist = who }):count():gte(4)
      t.expect:events("job.claimed", { kind = "eat", worker = who }):count():gte(4)
    end
    local fed = t:call("world.query", { select = { "min(Colonist.fed)" } }).rows[1][1] or 0
    t:log(string.format("least fed after three days: %.2f", fed))
    if fed < 0.25 then error("everyone is fed above a quarter, not " .. fed, 0) end
    t.expect:field("@colony", "Stockpile.food"):gt(0)
  end,
}
