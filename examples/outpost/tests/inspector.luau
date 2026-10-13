-- The inspector (the phase 3 gate: "a menu-driven in-game inspector" for
-- Outpost's core). Tab opens it; the menu actions move between its tabs
-- and press; a colonist's page follows them; Esc backs out; the mouse
-- clicks a tab; it answers while paused and keeps up at 10x; and on the
-- stress scene the people page lists two hundred colonists ten at a time.

-- Every shown UI text of a kind, top to bottom.
local function shown(t, comp)
  local q = t:call("world.query", {
    select = { "UiRect.pos", "UiText.shown", "UiRect.shown" },
    where = { has = { comp, "UiText" } },
  })
  local rows = {}
  for _, r in ipairs(q.rows) do
    if r[3] and r[2] ~= "" then table.insert(rows, r) end
  end
  table.sort(rows, function(a, b)
    if a[1][2] ~= b[1][2] then return a[1][2] < b[1][2] end
    return a[1][1] < b[1][1]
  end)
  local out = {}
  for _, r in ipairs(rows) do table.insert(out, r[2]) end
  return out
end

local function open(t)
  local q = t:call("world.query", { select = { "UiRect.shown", "Inspector.page" }, where = { has = { "Inspector" } } })
  return q.rows[1][1], q.rows[1][2]
end

local press = require("helpers").press

local function has(list, words)
  for _, l in ipairs(list) do
    if string.find(l, words, 1, true) then return true end
  end
  return false
end

scenario "the inspector: the colony, its people one by one, its stores and jobs" {
  scene = "valley", seed = 1,
  run = function(t)
    t:step(2)
    local on = open(t)
    if on then error("the inspector starts closed", 0) end
    press(t, "inspect")
    local on, page = open(t)
    if not on or page ~= "colony" then error("Tab opens it on the colony: " .. tostring(page), 0) end
    local lines = shown(t, "InspectLine")
    t:log("colony: " .. table.concat(lines, " | "))
    if not string.find(lines[1], "^Day 0, 0%d:00, day%.") then error("the day: " .. tostring(lines[1]), 0) end
    if not has(lines, "6 colonists") then error("six colonists", 0) end

    -- The People tab, by the arrows and Enter.
    press(t, "ui_move", { 1, 0 })
    press(t, "ui_confirm")
    local _, page = open(t)
    if page ~= "people" then error("the arrows and Enter reach People: " .. tostring(page), 0) end
    local rows = shown(t, "InspectRow")
    t:log("people: " .. table.concat(rows, " | "))
    if #rows ~= 6 or not string.find(rows[1], "^Ada") or not string.find(rows[6], "^Finn") then
      error("the six, by name: " .. table.concat(rows, " | "), 0)
    end
    if not has(shown(t, "InspectLine"), "1 of 1") then error("one page of them", 0) end

    -- Down to Ada ({0, -1} is down), and her page; Follow, and the view
    -- follows her.
    press(t, "ui_move", { 0, -1 })
    press(t, "ui_confirm")
    local _, page = open(t)
    if page ~= "person" then error("Ada's page: " .. tostring(page), 0) end
    lines = shown(t, "InspectLine")
    t:log("Ada: " .. table.concat(lines, " | "))
    if lines[1] ~= "Ada" or lines[2] ~= "Skills: farm cook" or not string.find(lines[3], "^Fed %d+%%, rest %d+%%$") then
      error("Ada's page: " .. table.concat(lines, " | "), 0)
    end
    press(t, "ui_move", { -1, 0 })
    press(t, "ui_confirm")
    local cam = t:call("world.query", { select = { "Camera.target" }, where = { has = { "Camera" }, ["Camera.pan_action"] = "" } })
    if #cam.rows ~= 1 or cam.rows[1][1] == nil then error("the view follows her", 0) end
    -- Back, and out.
    press(t, "ui_cancel")
    local _, page = open(t)
    if page ~= "people" then error("Esc backs out to the people: " .. tostring(page), 0) end
    press(t, "ui_cancel")
    local on = open(t)
    if on then error("Esc again closes it", 0) end
    local pan = t:call("world.query", { select = { "Camera.pan_action", "Camera.target" }, where = { has = { "Camera" }, ["not"] = { "Vignette" } } })
    if pan.rows[1][1] ~= "pan" or pan.rows[1][2] ~= nil then error("closed, the arrows move the view again and it stops following", 0) end

    -- Paused, it still answers; the clock stands still.
    press(t, "pause")
    press(t, "inspect")
    local on, page = open(t)
    if not on then error("it opens while paused", 0) end
    -- It opens where it was left: People, whose tab has the focus.
    if page ~= "people" then error("it opens where it was left: " .. tostring(page), 0) end
    press(t, "ui_move", { -1, 0 })
    press(t, "ui_confirm")
    local before = shown(t, "InspectLine")[1]
    if not string.find(before, "^Day 0, ") then error("the colony page, paused: " .. tostring(before), 0) end
    t:step(120)
    if shown(t, "InspectLine")[1] ~= before then error("paused, the day stands still", 0) end
    if not has(shown(t, "InspectLine"), "paused") then error("it says paused", 0) end
    press(t, "pause")

    -- The mouse: the Stores tab, clicked.
    local tabs = t:call("world.query", { select = { "UiRect.pos", "UiRect.size", "UiButton.event" }, where = { has = { "UiButton" } } })
    local at
    for _, r in ipairs(tabs.rows) do
      if r[3] == "inspect.tab.stores" then at = { r[1][1] + r[2][1] / 2, r[1][2] + r[2][2] / 2 } end
    end
    t.input:hold("pointer", at)
    t:step(1)
    press(t, "ui_click")
    local _, page = open(t)
    if page ~= "stores" then error("a click on Stores: " .. tostring(page), 0) end
    lines = shown(t, "InspectLine")
    t:log("stores: " .. table.concat(lines, " | "))
    if not string.find(lines[1], "^Stockpile: %d+ meals, %d+ crops") then error("the stockpile: " .. tostring(lines[1]), 0) end

    -- The jobs, and at 10x a day on it keeps up.
    press(t, "ui_move", { 1, 0 })
    press(t, "ui_confirm")
    press(t, "speed_10")
    t:step(3600)
    local _, page = open(t)
    if page ~= "jobs" then error("the Jobs tab: " .. tostring(page), 0) end
    lines = shown(t, "InspectLine")
    t:log("jobs a day on: " .. table.concat(lines, " | "))
    for _, kind in ipairs({ "eat", "sleep", "cook", "harvest", "haul" }) do
      if not has(lines, kind .. ":") then error("the " .. kind .. " jobs", 0) end
    end
    press(t, "ui_move", { -1, 0 })
    press(t, "ui_move", { -1, 0 })
    press(t, "ui_move", { -1, 0 })
    press(t, "ui_confirm")
    lines = shown(t, "InspectLine")
    if not string.find(lines[1], "^Day 1, ") then error("a day on: " .. tostring(lines[1]), 0) end
  end,
}

scenario "the stress scene's two hundred colonists, ten to a page" {
  scene = "stress", seed = 1,
  run = function(t)
    t:step(2)
    press(t, "inspect")
    press(t, "ui_move", { 1, 0 })
    press(t, "ui_confirm")
    if not has(shown(t, "InspectLine"), "1 of 20") then error("twenty pages", 0) end
    local first = shown(t, "InspectRow")
    if #first ~= 10 then error("ten to a page", 0) end
    -- The Next arrow, clicked.
    local b = t:call("world.query", { select = { "UiRect.pos", "UiRect.size", "UiButton.event" }, where = { has = { "UiButton" } } })
    local at
    for _, r in ipairs(b.rows) do
      if r[3] == "inspect.next" then at = { r[1][1] + r[2][1] / 2, r[1][2] + r[2][2] / 2 } end
    end
    t.input:hold("pointer", at)
    t:step(1)
    press(t, "ui_click")
    if not has(shown(t, "InspectLine"), "2 of 20") then error("the second page", 0) end
    local second = shown(t, "InspectRow")
    if second[1] == first[1] then error("the next ten", 0) end
    t:log("page 2 starts: " .. second[1])
    local p = t:call("prof.systems", { last = 60 })
    for _, r in ipairs(p.rows or {}) do
      if r.name == "ui.layout" or r.name == "sim.colony.inspector" then
        t:log(string.format("%s: mean %.3f ms", r.name, r.mean_ms or -1))
      end
    end
  end,
}
