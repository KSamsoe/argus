-- Walking into a monster attacks it: each hit takes Player.atk from its
-- hit points, the hero stays put, and at zero the monster is gone and
-- counted as a kill. Then the cell is free to walk onto.
return scenario "walking into a monster hurts it until it dies" {
  scene = "dungeon", seeds = range(1, 3),
  run = function(t)
    local m = t:call("world.query", {
      select = { "id", "Health.cur", "Monster.kind" }, where = { has = { "Monster" } },
      order = "dist(@player)", limit = 1,
    }).rows[1]
    local id, hp, kind = m[1], m[2], m[3]
    local atk = t:call("world.get", { entity = "@player" }).components.Player.atk
    -- Put it right next to the hero, at (4, 6).
    t:call("sim.set", { entity = id, path = "Transform.pos", value = { 4, 6 } })

    -- The first hit.
    t.input:tap("move", { 1, 0 })
    t:advance { by = "turn", n = 1 }
    t.expect:field(id, "Health.cur"):eq(hp - atk)
    t.expect:events("damage", { by = "@player" }):count():eq(1)
    t.expect:field("@player", "Transform.pos"):eq({ 3, 6 })

    -- Hit it until it dies.
    local hits = math.ceil(hp / atk)
    for _ = 2, hits do
      t.input:tap("move", { 1, 0 })
      t:advance { by = "turn", n = 1 }
    end
    t.expect:events("damage", { by = "@player" }):count():eq(hits)
    t.expect:events("death", { cause = "hero" }):count():eq(1)
    t.expect:count({ has = { "Monster" } }):eq(3)
    t.expect:field("@player", "Player.kills"):eq(1)
    t.expect:field("@messages", "UiText.shown"):eq("You kill the " .. kind .. ".")

    -- Its cell is open floor now.
    t.input:tap("move", { 1, 0 })
    t:advance { by = "turn", n = 1 }
    t.expect:field("@player", "Transform.pos"):eq({ 4, 6 })
  end,
}
