-- Picture goldens: the title menu, a fight (a slash out, a dash under way),
-- and the win screen, each drawn with the software rasteriser and compared
-- with its golden as a person would see it. A change to how Duel looks
-- fails here with a picture of where; `argus test looks --update` records
-- the new look (read the pictures in tests/goldens before keeping them).
local duel = require("duel")

return scenario "the menu, a fight, and the win screen look as they did" {
  scene = "arena", seed = 1,
  run = function(t)
    duel.menu(t)
    t.expect:image("menu")

    -- Player 1 swings at the air; player 2 dashes in from above, pale.
    t.input:tap("ui_confirm", true)
    t:step(2)
    duel.place(t, "@one", -2, 0, 1, 0)
    duel.place(t, "@two", 3, 2, -0.70710678, -0.70710678)
    t.input:tap("attack", true)
    t.input:slot(2):tap("dash", true)
    t:step(3)
    t.expect:events("point"):count():eq(0)
    t.expect:image("fight")

    -- Player 1's third point, and the win screen once the pause is over.
    t:call("sim.set", { entity = "@arena", path = "Match.one", value = 2 })
    t:step(40)
    duel.face_off(t, -1, 1.5)
    t.input:tap("attack", true)
    t:step(1)
    local r = t:run_until { event = "match.won", max_frames = 120 }
    assert(r.hit, "no match.won")
    t:step(2)
    t.expect:image("win")
  end,
}
