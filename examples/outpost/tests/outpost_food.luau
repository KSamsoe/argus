-- Section 9's balance scenario, fast-forwarded headless: a six-colonist
-- start does not starve in thirty days, on twenty seeds (a seed lays out
-- a different valley: trees, when each field was sown, the weather).
--
-- The series is declared before the run rather than after it as section
-- 9 writes it: declared late, the harness runs the seed again to sample
-- it from the start, which here would be thirty more days a seed.
return scenario "a 6-colonist start does not starve in 30 days" {
  scene = "valley", seeds = range(1, 20),
  run = function(t)
    local food = t.expect:series("Stockpile.food", { every = days(1) })
    t:run_until { sim_time = days(30), speed = "max" }
    t.expect:sum("Stockpile.food"):gte(60)
    t.expect:events("colonist.died", { cause = "starvation" }):count():eq(0)
    food:trend():gte(0)
  end,
}
