return scenario "a hero who waits is killed and the log says by whom" {
  scene = "delve", seeds = range(1, 5),
  run = function(t)
    t:advance { by = "turn", n = 120 }                -- no commands: the hero waits
    t.expect:events("player.died"):count():eq(1)
    t.expect:field("@player", "Health.cur"):eq(0)
    -- Why: each turn a monster struck, the event names it, its kind, the amount, the turn.
    t.expect:events("damage", { target = "@player" }):count():gte(3)
    t.expect:events("turn.end"):count():gte(3)
  end,
}
