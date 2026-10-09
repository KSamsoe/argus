-- Woodcutting: the valley's trees are felled for wood. A chop job is
-- taken like any other, by skill (chop: Bram, Cleo, and Dov) and priority
-- (below the food's jobs); the woodcutter walks to the tree, chops it a
-- while, and its logs are hauled to the stockpile, which counts them
-- (Stockpile.wood) and the HUD shows. Trees regrow: a stump grows back
-- into a tree over Colony.tree_days days of daylight and is felled again.
-- The 30-day food scenario (tests/outpost_food.luau) is the proof that
-- nobody starves for it.

local STUMP, TREE = "#7A5A38", "#1E3B1E"

local function one(t, select, where)
  local q = t:call("world.query", { select = select, where = where })
  if #q.rows ~= 1 then error("one row for " .. select[1] .. ", not " .. #q.rows, 0) end
  return q.rows[1]
end

local function who(t, name)
  return one(t, { "id" }, { ["Colonist.name"] = name })[1]
end

-- The HUD's line, as the player reads it.
local function hud_line(t)
  return one(t, { "UiText.text" }, { ["Hud.kind"] = "line" })[1]
end

-- The first event of a name whose fields match, or nil.
local function first(t, name, match)
  local ev = t:call("obs.events", { name = name, limit = 1000 })
  for _, r in ipairs(ev.rows) do
    local ok = true
    for k, v in pairs(match or {}) do
      if r.fields[k] ~= v then ok = false end
    end
    if ok then return r end
  end
  return nil
end

scenario "woodcutters fell trees and the wood reaches the stockpile in the first two days" {
  scene = "valley", seeds = range(1, 10),
  run = function(t)
    t.expect:count({ has = { "Tree", "NavCost" } }):gte(40)
    t.expect:count({ has = { "JobKind" }, ["JobKind.name"] = "chop" }):eq(1)
    t:run_until { sim_time = days(2) }
    -- Chopped, by woodcutters only.
    t.expect:events("job.claimed", { kind = "chop" }):exists()
    t.expect:events("job.done", { kind = "chop" }):exists()
    t.expect:events("tree.felled"):exists()
    for _, name in ipairs({ "Ada", "Esme", "Finn" }) do
      t.expect:events("job.claimed", { kind = "chop", worker = who(t, name) }):count():eq(0)
    end
    local claims = t:call("obs.events", { name = "job.claimed", limit = 1000 })
    local choppers = {}
    for _, r in ipairs(claims.rows) do
      if r.fields.kind == "chop" then
        local c = t:call("world.get", { entity = r.fields.worker }).components
        if not string.find(" " .. c.Worker.skills .. " ", " chop ", 1, true) then
          error(c.Colonist.name .. " chopped without the skill", 0)
        end
        choppers[c.Colonist.name] = true
      end
    end
    local names = {}
    for name in pairs(choppers) do table.insert(names, name) end
    table.sort(names)
    t:log("chopped by " .. table.concat(names, ", "))
    -- Hauled in, and counted.
    t.expect:events("colony.stored", { kind = "item:wood" }):exists()
    t.expect:sum("Stockpile.wood"):gte(3)
    local felled = first(t, "tree.felled")
    local stored = first(t, "colony.stored", { kind = "item:wood" })
    local wood = t.expect:field("@colony", "Stockpile.wood"):get()
    t:log(string.format("first tree felled at %.2f days, first wood stored at %.2f days; %d wood by day 2",
      felled.frame / 3600, stored.frame / 3600, wood))
    -- The HUD shows it (written twice a second, so at most what is stored).
    local line = hud_line(t)
    local shown = tonumber(string.match(line, "wood (%d+)"))
    if shown == nil or shown < 3 or shown > wood then error("the HUD shows the wood: " .. line, 0) end
    -- Nobody went hungry for it.
    t.expect:events("colonist.died"):count():eq(0)
    t.expect:count({ has = { "Colonist" } }):eq(6)
  end,
}

scenario "a felled tree is a stump that grows back and is felled again" {
  scene = "valley", seeds = range(1, 3),
  run = function(t)
    t:run_until { event = "tree.felled", max_ticks = days(2) * 60 }
    local ev = first(t, "tree.felled")
    local tree = ev.fields.tree
    t.expect:field(tree, "Tree.growth"):lt(0.01)
    t.expect:field(tree, "Sprite.color"):eq(STUMP)
    -- It still stands in the way, so no path changes under the colony.
    t.expect:count({ has = { "Tree", "NavCost" } }):gte(40)
    -- Its logs lie beside it until a hauler takes them.
    t.expect:count({ has = { "Item" }, ["Item.kind"] = "item:wood" }):gte(1)
    -- Three days of daylight later it is a tree again.
    local days_to_grow = t.expect:field("@colony", "Colony.tree_days"):get()
    t:run_until { event = "tree.grown", max_ticks = math.ceil((days_to_grow + 1) * 3600) }
    local grown = first(t, "tree.grown", { tree = tree })
    if grown == nil then error("the first tree felled is the first to grow back", 0) end
    t:log(string.format("felled at %.2f days, stood again at %.2f days", ev.frame / 3600, grown.frame / 3600))
    t.expect:field(tree, "Tree.growth"):eq(1)
    t.expect:field(tree, "Sprite.color"):eq(TREE)
    -- And it is felled again: the valley's wood does not run out.
    t:run_until { sim_time = days(9) }
    local again = t:call("obs.events", { name = "tree.felled", limit = 1000 })
    local n = 0
    for _, r in ipairs(again.rows) do
      if r.fields.tree == tree then n += 1 end
    end
    if n < 2 then error("the tree is felled again once it has grown back: " .. n .. " times", 0) end
    t:log(string.format("by day 9: %d trees felled, this one %d times; %d wood stored",
      again.total, n, t.expect:field("@colony", "Stockpile.wood"):get()))
    t.expect:events("colonist.died"):count():eq(0)
  end,
}

scenario "the inspector counts the trees and the wood" {
  scene = "valley", seed = 1,
  run = function(t)
    t:run_until { sim_time = days(1.3) }
    t.input:tap("inspect")
    t:step(2)
    local function lines(page)
      local q = t:call("world.query", {
        select = { "UiText.shown", "UiRect.shown", "InspectLine.n" },
        where = { has = { "InspectLine" }, ["InspectLine.page"] = page },
      })
      local out = {}
      for _, r in ipairs(q.rows) do
        if r[2] and r[1] ~= "" then out[r[3]] = r[1] end
      end
      return out
    end
    local colony = lines("colony")
    local trees = colony[8]
    t:log("colony: " .. tostring(trees))
    if trees == nil or not string.find(trees, "^%d+ trees standing, %d+ felled and growing back, %d+ being felled$") then
      error("the colony page counts the trees: " .. tostring(trees), 0)
    end
    local felled = tonumber(string.match(trees, "(%d+) felled"))
    if felled < 1 then error("some are felled by now: " .. trees, 0) end
    -- The Stores tab: the wood in the stockpile.
    local tabs = t:call("world.query", { select = { "id", "UiButton.event" }, where = { has = { "UiButton" } } })
    local stores
    for _, r in ipairs(tabs.rows) do
      if r[2] == "inspect.tab.stores" then stores = r[1] end
    end
    t.input:tap("ui_move", { 1, 0 })
    t:step(2)
    t.input:tap("ui_move", { 1, 0 })
    t:step(2)
    t.expect:field(stores, "UiButton.focused"):eq(true)
    t.input:tap("ui_confirm")
    t:step(2)
    local pile = lines("stores")[1]
    t:log("stores: " .. tostring(pile))
    if pile == nil then error("the stores page is shown", 0) end
    local wood = math.floor(t.expect:field("@colony", "Stockpile.wood"):get())
    if wood < 1 or not string.find(pile, ", " .. wood .. " wood, ", 1, true) then
      error("the stores page shows the stockpile's wood (" .. wood .. "): " .. pile, 0)
    end
  end,
}

-- A valley laid out before trees could be felled (a save from then): its
-- trees were no Trees, it had no chop JobKind, and its founders had their
-- old skills. The next tick makes its trees Trees, gives it the chop job,
-- and teaches Bram, Cleo, and Dov to fell trees; then they go to work.
return scenario "a valley laid out before woodcutting gets its trees and woodcutters" {
  scene = "valley", seed = 2,
  run = function(t)
    t:step(2)
    local trees = t:call("world.query", { select = { "id" }, where = { has = { "Tree" } }, limit = 1000 })
    local n = #trees.rows
    local tx = t:call("edit.begin", { author = "test:outpost", note = "a valley from before woodcutting" }).tx
    for _, r in ipairs(trees.rows) do
      t:call("edit.remove", { tx = tx, entity = r[1], component = "Tree" })
    end
    t:call("edit.despawn", { tx = tx, entity = one(t, { "id" }, { ["JobKind.name"] = "chop" })[1] })
    local old = { Bram = "farm", Cleo = "cook", Dov = "farm" }
    for name, skills in pairs(old) do
      t:call("edit.set", { tx = tx, entity = who(t, name), path = "Worker.skills", value = skills })
    end
    t:call("edit.commit", { tx = tx })
    t.expect:count({ has = { "Tree" } }):eq(0)
    t.expect:count({ has = { "JobKind" } }):eq(5)
    t:step(1)
    t.expect:count({ has = { "Tree", "NavCost" } }):eq(n)
    t.expect:count({ has = { "Tree", "Station" } }):eq(0)
    t.expect:count({ has = { "Tree" }, ["Tree.reach"] = true }):gte(1)
    t.expect:count({ has = { "JobKind" }, ["JobKind.name"] = "chop" }):eq(1)
    t.expect:field(who(t, "Bram"), "Worker.skills"):eq("farm chop")
    t.expect:field(who(t, "Cleo"), "Worker.skills"):eq("cook chop")
    t.expect:field(who(t, "Finn"), "Worker.skills"):eq("")
    t:run_until { sim_time = days(1.5) }
    t.expect:events("tree.felled"):exists()
    t.expect:events("job.claimed", { kind = "chop", worker = who(t, "Finn") }):count():eq(0)
  end,
}
