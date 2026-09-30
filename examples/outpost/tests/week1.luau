-- The save from the first week of phase 3 (tests/saves/week1.save, made at
-- day 7 on 2026-09-24 by Outpost's first skeleton) still loads. The phase 3
-- gate asks that it load at the end of the phase through at least one
-- migration: since item 3 it goes through four. Colony drops the rule the
-- fields replaced and, since phase 4, the speed its HUD used to show (the
-- clock's is q:speed()), and Stockpile counts its food as meals (both declared);
-- Colonist and Needs gained fields and come field by field. The skeleton
-- had no valley, so the next tick lays one out around its six colonists,
-- and the colony plays on.
return scenario "the first week's save still loads" {
  scene = "valley", seed = 1,
  run = function(t)
    local loaded = t:call("sim.load", { path = "tests/saves/week1.save" })
    t.expect:events("component.migrated"):count():eq(4)
    t.expect:events("component.migrated", { component = "Colony", declared = true, from = 1, to = 3 }):count():eq(1)
    t.expect:events("component.migrated", { component = "Stockpile", declared = true, from = 1, to = 2 }):count():eq(1)
    t.expect:field("@colony", "Colony.day"):eq(7)
    t.expect:count({ has = { "Colonist" } }):eq(6)
    t.expect:field("@colony", "Stockpile.food"):gte(6)
    local food = t.expect:field("@colony", "Stockpile.food"):get()
    t.expect:field("@colony", "Stockpile.meal"):eq(food)
    -- The next tick lays the valley out and fits the colonists out.
    t:step(1)
    t.expect:events("colony.laid_out", { day = 7, colonists = 6 }):count():eq(1)
    t.expect:count({ has = { "Colonist", "Worker", "NavAgent", "Transform" } }):eq(6)
    t.expect:count({ has = { "Field" } }):eq(10)
    t.expect:count({ has = { "JobKind" } }):eq(5)
    -- A day more: nobody starves, and the colony works.
    t:step(3600)
    t.expect:field("@colony", "Colony.day"):eq(8)
    t.expect:count({ has = { "Colonist" } }):eq(6)
    t.expect:events("colonist.died"):count():eq(0)
    t.expect:events("day.turned"):count():eq(1)
    t.expect:events("job.done", { kind = "harvest" }):exists()
    t.expect:events("job.done", { kind = "eat" }):exists()
    return loaded
  end,
}
