-- Under a finger (input.toml's [touch]): a tap on the menu's button is a
-- click, the stick at the bottom left moves player 1, and the Attack and
-- Dash buttons at the bottom right swing and dash. Places are in the UI
-- canvas's pixels (960 x 540), as input.toml lays the controls out.
local duel = require("duel")

return scenario "player 1 plays under a finger: a tap starts the match, the stick moves, the buttons swing and dash" {
  scene = "arena", seed = 1,
  run = function(t)
    duel.menu(t)
    -- A tap on Two players here, where the layout put it.
    local r = t:call("world.get", { entity = "@menu/panel/here" }).components.UiRect
    local x, y = r.pos[1] + r.size[1] / 2, r.pos[2] + r.size[2] / 2
    t.input:touch(1, "start", x, y)
    t:step(1)
    t.input:touch(1, "end", x, y)
    t:step(2)
    t.expect:field("@arena", "Match.state"):eq("fight")

    -- The stick (centred 140 in from the bottom left): dragged right.
    local start = duel.field(t, "@one", "Transform.pos.x")
    t.input:touch(2, "start", 140, 400)
    t:step(1)
    t.input:touch(2, "move", 230, 400)
    t:step(30)
    t.input:touch(2, "end", 230, 400)
    t:step(1)
    t.expect:field("@one", "Transform.pos.x"):gt(start + 2.5)
    t.expect:field("@one", "Fighter.facing.x"):near(1, 1e-6)

    -- Attack (110 in from the bottom right, 120 up), then Dash (245, 80).
    t.input:touch(3, "start", 960 - 110, 540 - 120)
    t:step(1)
    t.input:touch(3, "end", 960 - 110, 540 - 120)
    t:step(1)
    t.expect:events("swing", { by = "@one" }):count():eq(1)
    t.input:touch(4, "start", 960 - 245, 540 - 80)
    t:step(1)
    t.input:touch(4, "end", 960 - 245, 540 - 80)
    t:step(1)
    t.expect:events("dash", { by = "@one" }):count():eq(1)
    t.expect:events("dash", { by = "@two" }):count():eq(0)
  end,
}
