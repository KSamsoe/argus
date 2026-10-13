-- Outpost's first week (phase 3 item 0): six colonists eat from what the
-- fields yield, nobody starves on any of five seeds, and the game saves
-- itself each time a day turns.
return scenario "a week without starving, saved every day" {
  scene = "valley", seeds = range(1, 5),
  run = function(t)
    t:run_until { sim_time = days(7) + 1 }
    t.expect:field("@colony", "Colony.day"):eq(7)
    t.expect:count({ has = { "Colonist" } }):eq(6)
    t.expect:events("colonist.died"):count():eq(0)
    t.expect:field("@colony", "Stockpile.food"):gte(6)
    t.expect:events("day.turned"):count():eq(7)
    t.expect:events("game.saved", { save = "auto" }):count():eq(7)
  end,
}
