-- The save from the first week of phase 3 (tests/saves/week1.save, made at
-- day 7 on 2026-09-24) still loads. The phase 3 gate asks that it load at
-- the end of the phase through at least one migration: every change to
-- Outpost's components from here on needs one, or this fails.
return scenario "the first week's save still loads" {
  scene = "valley", seed = 1,
  run = function(t)
    local loaded = t:call("sim.load", { path = "tests/saves/week1.save" })
    t.expect:field("@colony", "Colony.day"):eq(7)
    t.expect:count({ has = { "Colonist" } }):eq(6)
    t.expect:field("@colony", "Stockpile.food"):gte(6)
    -- The colony plays on from the save: a day more, nobody starves.
    t:step(3600)
    t.expect:field("@colony", "Colony.day"):eq(8)
    t.expect:count({ has = { "Colonist" } }):eq(6)
    t.expect:events("colonist.died"):count():eq(0)
    t.expect:events("day.turned"):count():eq(1)
    t.expect:events("component.migrated"):count():eq(0)
    return loaded
  end,
}
