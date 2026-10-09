-- Healing potions: a monster the hero kills sometimes drops one where it
-- fell, rolled on the run's loot stream; walking onto one picks it up; the
-- drink command (q) drinks one on the hero's turn, healing a set amount
-- (Player.potion_heal) never above the maximum, and takes the turn. With
-- none, it says so and no turn passes. The HUD counts them and the message
-- line says what happened.

local function me(t)
  return t:call("world.get", { entity = "@player" }).components
end

local function shown(t, id)
  return t:call("world.get", { entity = id }).components.UiText.shown
end

local function events(t, name, since)
  return t:call("obs.events", { name = name, since = since, limit = 500 }).rows
end

local function last_seq(t)
  return t:call("obs.events", { limit = 1 }).last_seq
end

local function has(s, part)
  return s ~= nil and string.find(s, part, 1, true) ~= nil
end

-- How many potions lie on the cell (x, y). (A where on a vec2 field with
-- whole numbers matches nothing, so the cells are compared here.)
local function potions_at(t, x, y)
  local r = t:call("world.query", { select = { "Transform.pos" }, where = { has = { "Potion" } }, limit = 100 })
  local n = 0
  for _, row in ipairs(r.rows) do
    if row[1][1] == x and row[1][2] == y then n += 1 end
  end
  return n
end

local function monster_cells(t)
  local r = t:call("world.query", { select = { "id", "Transform.pos" }, where = { has = { "Monster" } }, limit = 100 })
  local cells = {}
  for _, row in ipairs(r.rows) do cells[row[1]] = row[2][1] .. "," .. row[2][2] end
  return cells
end

-- One step of the hunt: to the nearest potion lying there, else at the
-- nearest monster (walking into it attacks). False when nothing is left.
local function hunt_step(t)
  local target = nil
  for _, kind in ipairs({ "Potion", "Monster" }) do
    local r = t:call("world.query", {
      select = { "id", "Transform.pos" }, where = { has = { kind } }, order = "dist(@player)", limit = 1,
    })
    if r.total > 0 then
      target = r.rows[1][2]
      break
    end
  end
  if target == nil then return false end
  local pos = me(t).Transform.pos
  local p = t:call("nav.path", { from = "@player", to = { target[1], target[2] } })
  if not p.found then error("no way to " .. target[1] .. ", " .. target[2] .. ": " .. tostring(p.why), 0) end
  t.input:tap("move", { p.points[1][1] - pos[1], p.points[1][2] - pos[2] })
  t:advance { by = "turn", n = 1 }
  return true
end

local drops_and_pickups = scenario "a monster the hero kills may drop a potion, and walking onto it picks it up" {
  -- Seeds whose six kills drop some potions and not all: a drop is a
  -- chance (Monster.drop in a hundred), and each seed always drops the
  -- same ones, the loot stream being the seed's (the golden pins them).
  -- Seeds 3 and 19 are ones whose kills drop none.
  scene = "delve", seeds = { 1, 2, 4, 5, 6, 7 },
  run = function(t)
    t:advance { by = "turn", n = 1 }
    local from = last_seq(t)
    -- The hero lives through the hunt on any seed (as tests/map.luau).
    t:call("sim.set", { entity = "@player", path = "Health.cur", value = 1000 })

    local seen = from
    local carried = 0
    for _ = 1, 400 do
      if not hunt_step(t) then break end
      local now = me(t)
      if now.Player.dead then error("the hero died", 0) end
      -- A drop: a potion lies where the monster fell, and the line says so.
      local dropped = events(t, "potion.dropped", seen)
      local deaths = events(t, "death", seen)
      for _, d in ipairs(dropped) do
        local f = d.fields
        local matched = false
        for _, k in ipairs(deaths) do
          if k.fields.x == f.x and k.fields.y == f.y and k.fields.turn == f.turn and k.fields.kind == f.kind then matched = true end
        end
        if not matched then error(string.format("a potion dropped at %d, %d with no kill there", f.x, f.y), 0) end
        if potions_at(t, f.x, f.y) < 1 then error(string.format("no potion lies at %d, %d after its drop", f.x, f.y), 0) end
        local line = shown(t, "@messages")
        if not has(line, "drops a potion.") then error("the drop's message: " .. tostring(line), 0) end
      end
      -- A pickup: the count goes up by the potions on the cell, the HUD
      -- and the line say so, and none lies there any more.
      local picked = events(t, "potion.picked", seen)
      if #picked > 0 then
        carried += #picked
        if now.Player.potions ~= carried then error(string.format("carrying %d, not %d", now.Player.potions, carried), 0) end
        local pos = now.Transform.pos
        if potions_at(t, pos[1], pos[2]) ~= 0 then error("a potion still lies where the hero picked it up", 0) end
        local line = shown(t, "@messages")
        if not has(line, "You pick up a potion; you carry " .. carried .. ".") then error("the pickup's message: " .. tostring(line), 0) end
        local hud = shown(t, "@turn")
        if not has(hud, "Potions " .. carried .. ".") then error("the HUD: " .. tostring(hud), 0) end
      end
      seen = last_seq(t)
    end

    -- Every monster dead, every potion that dropped picked up and carried.
    t.expect:count({ has = { "Monster" } }):eq(0)
    t.expect:count({ has = { "Potion" } }):eq(0)
    local deaths = #events(t, "death", from)
    local drops = #events(t, "potion.dropped", from)
    t.expect:events("potion.dropped"):count():gte(1)
    t.expect:events("potion.picked"):count():eq(drops)
    t.expect:field("@player", "Player.potions"):eq(drops)
    -- Sometimes, not always.
    if drops >= deaths then error(string.format("%d drops from %d kills", drops, deaths), 0) end
    t.expect:snapshot("after_the_hunt", { select = { "Player.kills", "Player.potions" }, where = { has = { "Player" } } })
    t:log(string.format("%d kills dropped %d potions, all picked up", deaths, drops))

    -- The drops are the run's: back to the hunt's first turn and forward
    -- again, the replay rolls the same ones (its events are checked
    -- against the log).
    local stop = t:status().tick
    local s = t:call("sim.seek", { frame = 2 })
    if s.diverged ~= nil then error("the replay to the start diverged", 0) end
    t.expect:field("@player", "Player.potions"):eq(0)
    s = t:call("sim.seek", { frame = stop })
    if s.diverged ~= nil then error("the replay of the hunt diverged: " .. tostring(s.diverged), 0) end
    t.expect:field("@player", "Player.potions"):eq(drops)

    -- And one of them drunk, as the player would: wounded, it heals.
    t:call("sim.set", { entity = "@player", path = "Health.cur", value = 2 })
    t.input:tap("drink")
    t:advance { by = "turn", n = 1 }
    t.expect:field("@player", "Health.cur"):eq(6)
    t.expect:field("@player", "Player.potions"):eq(drops - 1)
  end,
}

local drinking = scenario "drinking heals a set amount, never above the maximum, and takes a turn; with none it only says so" {
  scene = "delve", seeds = range(1, 5),
  run = function(t)
    t:advance { by = "turn", n = 1 }
    local from = last_seq(t)
    t:call("sim.set", { entity = "@player", path = "Player.potions", value = 3 })
    t:call("sim.set", { entity = "@player", path = "Health.cur", value = 3 })
    local heal = me(t).Player.potion_heal
    local max = me(t).Health.max
    if heal ~= 4 or max ~= 10 then error(string.format("a potion heals %d of %d", heal, max), 0) end

    -- One drink, from the key the player presses (Q, as the window hands
    -- it over, held over the turn): 3 + 4 is 7, a potion fewer, and the
    -- turn is taken (the turn counts, and the monsters act).
    local turn = t:call("world.get", { entity = "@turn" }).components.Turn.n
    local ends = #events(t, "turn.end", from)
    t:call("input.device", { events = { { key = "KeyQ", pressed = true } } })
    t:advance { by = "turn", n = 1 }
    t:call("input.device", { events = { { key = "KeyQ", pressed = false } } })
    t.expect:field("@player", "Health.cur"):eq(7)
    t.expect:field("@player", "Player.potions"):eq(2)
    t.expect:field("@turn", "Turn.n"):eq(turn + 1)
    if #events(t, "turn.end", from) ~= ends + 1 then error("the monsters did not act after the drink", 0) end
    t.expect:field("@messages", "UiText.shown"):eq("You drink a potion and heal 4, to 7/10.")
    if not has(shown(t, "@turn"), "HP 7/10. Potions 2.") then error("the HUD: " .. tostring(shown(t, "@turn")), 0) end

    -- Never above the maximum: 7 + 4 stops at 10.
    t.input:tap("drink")
    t:advance { by = "turn", n = 1 }
    t.expect:field("@player", "Health.cur"):eq(10)
    t.expect:field("@player", "Player.potions"):eq(1)
    t.expect:field("@messages", "UiText.shown"):eq("You drink a potion and heal 3, to 10/10.")

    -- At full health it is drunk for nothing: no overheal, and the turn is
    -- still taken.
    t.input:tap("drink")
    t:advance { by = "turn", n = 1 }
    t.expect:field("@player", "Health.cur"):eq(10)
    t.expect:field("@player", "Player.potions"):eq(0)
    t.expect:field("@turn", "Turn.n"):eq(turn + 3)
    t.expect:field("@messages", "UiText.shown"):eq("You drink a potion, but you are already at full health.")
    local drunk = events(t, "potion.drunk", from)
    if #drunk ~= 3 then error(#drunk .. " drinks", 0) end
    local healed = {}
    for _, d in ipairs(drunk) do table.insert(healed, d.fields.healed) end
    if table.concat(healed, " ") ~= "4 3 0" then error("healed " .. table.concat(healed, " "), 0) end
    -- No monster reached the hero meanwhile, so the hit points are the drinks'.
    t.expect:events("damage", { target = "@player" }):count():eq(0)

    -- With none: nothing but the words. No turn passes, no monster moves,
    -- nothing heals.
    local at = monster_cells(t)
    ends = #events(t, "turn.end", from)
    t:call("sim.set", { entity = "@player", path = "Health.cur", value = 5 })
    t.input:tap("drink")
    t:advance { by = "turn", n = 1 }
    t.expect:events("potion.none"):count():eq(1)
    t.expect:field("@messages", "UiText.shown"):eq("You have no potions to drink.")
    t.expect:field("@player", "Health.cur"):eq(5)
    t.expect:field("@player", "Player.potions"):eq(0)
    t.expect:field("@turn", "Turn.n"):eq(turn + 3)
    if #events(t, "turn.end", from) ~= ends then error("a turn passed on a drink with no potion", 0) end
    for id, cell in pairs(monster_cells(t)) do
      if at[id] ~= cell then error(id .. " moved on a drink with no potion: " .. tostring(at[id]) .. " to " .. cell, 0) end
    end
    -- The game goes on: the next turn passes as ever.
    t:advance { by = "turn", n = 1 }
    t.expect:field("@turn", "Turn.n"):eq(turn + 4)
  end,
}

return { drops_and_pickups, drinking }
