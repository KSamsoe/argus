return scenario "walking at the nearest monster attacks it" {
  scene = "delve", seed = 4,
  run = function(t)
    t:advance { by = "turn", n = 1 }
    for turn = 1, 40 do
      -- Where is the nearest monster? A query, like any client would ask.
      local r = t:call("world.query", {
        select = { "id", "Transform.pos" },
        where = { has = { "Monster" } },
        order = "dist(@player)", limit = 1,
      })
      if r.total == 0 then break end
      local me = t:call("world.get", { entity = "@player" }).components.Transform.pos
      local mp = r.rows[1][2]
      local dx, dy = mp[1] - me[1], mp[2] - me[2]
      if math.abs(dx) >= math.abs(dy) then
        t.input:tap("move", { dx > 0 and 1 or -1, 0 })
      else
        t.input:tap("move", { 0, dy > 0 and 1 or -1 })
      end
      t:advance { by = "turn", n = 1 }
      if t:call("world.get", { entity = "@player" }).components.Player.dead then break end
    end
    t.expect:events("damage", { by = "@player" }):count():gte(1)
    t.expect:events("death", { cause = "hero" }):count():gte(1)
    t.expect:field("@player", "Player.kills"):gte(1)
    t.expect:events("turn.end"):count():gte(5)
    t.expect:snapshot("after_the_fight", { select = {"Transform.pos", "Health.cur"}, where = { has = {"Health"} } })
  end,
}
