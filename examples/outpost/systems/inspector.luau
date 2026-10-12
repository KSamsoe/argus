-- The inspector (the phase 3 gate: "a menu-driven in-game inspector"; see
-- components/inspector.luau). Tab opens a panel on the right with four
-- tabs: the colony at a glance, its people (ten to a page; press one to
-- see them, and follow them with the view), its stores, and its jobs. The
-- menu actions move the focus and press, the mouse clicks, and Esc backs
-- out. It answers while the game is paused, and its words are written
-- again six times a second of the player's time at any speed. One panel
-- at a time with the game's menu (systems/menu.luau): while the inspector
-- is open Esc is its own, and while the menu is open Tab does nothing.
--
-- The panel is made of UI entities like any other tree, built here the
-- first tick there is none: a save from before it (the first week's) gets
-- one too.

local TABS = { "colony", "people", "stores", "jobs" }
local LINES = { colony = 8, people = 1, person = 5, stores = 4, jobs = 14 }
local ROWS = 10

local function pct(x)
  return math.floor(x * 100 + 0.5)
end

-- The colony's hour at t, a share of its day, as the HUD shows it
-- (systems/hud.luau): dawn 05:00, night falling at 19:00.
local function hour(t, dusk)
  local h = if t < dusk then 5 + 14 * t / dusk else 19 + 10 * (t - dusk) / (1 - dusk)
  return math.floor(h) % 24
end

local function spawn(q, parent, names)
  local all = { "UiNode", "Parent" }
  for _, n in ipairs(names or {}) do table.insert(all, n) end
  local e = q:spawn(all)
  e:get("Parent").entity = parent
  return e
end

local function button(q, parent, event, key)
  local b = spawn(q, parent, { "UiPanel", "UiButton", "UiText" })
  b:get("UiNode").pad = vector.create(10, 5, 0)
  b:get("UiPanel").color = "button"
  b:get("UiButton").event = event
  local t = b:get("UiText")
  if key ~= nil then t.key = key end
  t.align = "center"
  return b
end

local function lines(q, page, name, n)
  for i = 1, n do
    local l = spawn(q, page, { "UiText", "InspectLine" })
    local t = l:get("UiText")
    t.size = 14
    t.wrap = true
    local il = l:get("InspectLine")
    il.page = name
    il.n = i
  end
end

system "sim.colony.inspector.build" {
  stage = "sim",
  on_load = true,
  reads = { "Inspector" },
  writes = { "UiRoot", "UiNode", "UiPanel", "UiText", "UiButton", "Parent", "Inspector", "InspectPage", "InspectLine", "InspectRow" },
  doc = "Builds the inspector's panel once (while there is no Inspector): a title, a row of tabs (colony, people, stores, jobs), a page each with its lines, ten colonist buttons and a page's worth of arrows on the people page, a person page with Follow and Back, and a line of hints. Hidden until Tab.",
  run = function(q)
    if q:count("Inspector") > 0 then return end
    local root = q:spawn({ "UiRoot", "UiNode", "UiPanel", "Inspector" })
    root:get("UiRoot").layer = 20
    local n = root:get("UiNode")
    n.absolute = true
    n.anchor = vector.create(1, 0, 0)
    n.pivot = vector.create(1, 0, 0)
    n.offset = vector.create(-12, 66, 0)
    n.size = vector.create(420, 0, 0)
    n.pad = vector.create(14, 12, 0)
    n.gap = 8
    n.visible = false
    local p = root:get("UiPanel")
    p.color = "panel"
    p.border = 1

    local title = spawn(q, root, { "UiText" }):get("UiText")
    title.key = "inspect.title"
    title.size = 18

    local tabs = spawn(q, root)
    tabs:get("UiNode").direction = "row"
    tabs:get("UiNode").gap = 6
    for _, name in ipairs(TABS) do
      button(q, tabs, "inspect.tab." .. name, "inspect.tab." .. name)
    end

    for _, name in ipairs({ "colony", "people", "person", "stores", "jobs" }) do
      local page = spawn(q, root, { "InspectPage" })
      page:get("InspectPage").name = name
      local pn = page:get("UiNode")
      pn.gap = 4
      pn.visible = name == "colony"
      if name == "people" then
        for i = 1, ROWS do
          local r = button(q, page, "inspect.row", nil)
          r:get("UiNode").pad = vector.create(8, 3, 0)
          local t = r:get("UiText")
          t.align = "left"
          t.size = 14
          r:insert("InspectRow", { n = i })
        end
        local nav = spawn(q, page)
        local nn = nav:get("UiNode")
        nn.direction = "row"
        nn.gap = 10
        nn.align = "center"
        button(q, nav, "inspect.prev", "inspect.people.prev")
        lines(q, nav, name, LINES.people)
        button(q, nav, "inspect.next", "inspect.people.next")
      else
        lines(q, page, name, LINES[name])
      end
      if name == "person" then
        local row = spawn(q, page)
        row:get("UiNode").direction = "row"
        row:get("UiNode").gap = 8
        button(q, row, "inspect.follow", "inspect.person.follow")
        button(q, row, "inspect.back", "inspect.person.back")
      end
    end

    local hint = spawn(q, root, { "UiText" }):get("UiText")
    hint.key = "inspect.hint"
    hint.size = 12
    hint.color = "muted"
  end,
}

-- The colony at a glance.
local function colony_page(q)
  local rules = q:entity("@colony"):get("Colony")
  local t = (rules.clock / q.day_length) % 1
  local meal, crop, food = 0, 0, 0
  for _, s in q:each("Stockpile") do
    meal += s.meal
    crop += s.crop
    food += s.food
  end
  local alive, working, idle = 0, 0, 0
  local why = {}
  for _, _, w in q:each("Colonist", "Worker") do
    alive += 1
    if w.job ~= nil then
      working += 1
    else
      idle += 1
      if w.idle ~= "" then why[w.idle] = (why[w.idle] or 0) + 1 end
    end
  end
  local most, most_n = nil, 0
  for k, v in pairs(why) do
    if v > most_n or (v == most_n and most ~= nil and k < most) then
      most, most_n = k, v
    end
  end
  local fields, ripe = 0, 0
  for _, f in q:each("Field") do
    fields += 1
    if f.growth >= 1 then ripe += 1 end
  end
  local open, taken = 0, 0
  for _, j in q:each("Job") do
    if j.worker ~= nil then taken += 1 else open += 1 end
  end
  local trees, felled, chopping = 0, 0, 0
  for _, tr in q:each("Tree") do
    if tr.growth < 1 then
      felled += 1
    else
      trees += 1
      if tr.job ~= nil and tr.job:alive() and tr.job:get("Job").worker ~= nil then chopping += 1 end
    end
  end
  local speed = if rules.paused then q:text("hud.paused") else q:text("hud.speed", { x = q:speed() })
  return {
    q:text("inspect.colony.day", {
      day = rules.day,
      hour = string.format("%02d", hour(t, rules.dusk)),
      when = q:text(if t >= rules.dusk then "inspect.night" else "inspect.day"),
      speed = speed,
    }),
    q:text("inspect.colony.food", { food = string.format("%.1f", food), meals = math.floor(meal), crops = math.floor(crop) }),
    q:text("inspect.colony.people", { alive = alive, working = working, idle = idle }),
    q:text("inspect.colony.lost", { lost = q:count("Grave") }),
    q:text("inspect.colony.fields", { fields = fields, ripe = ripe, growth = pct(rules.growth) }),
    q:text("inspect.colony.jobs", { open = open, taken = taken }),
    if most ~= nil then q:text("inspect.colony.idle", { why = most, n = most_n }) else "",
    -- Last, so a panel built before there were trees to fell (a save's,
    -- with a line fewer) loses only this one.
    q:text("inspect.colony.trees", { trees = trees, felled = felled, chopping = chopping }),
  }
end

-- What a colonist is doing, in a few words.
local function doing(q, w)
  if w.job == nil then return q:text("inspect.people.idle") end
  local j = w.job:get("Job")
  return q:text("inspect.people.doing", { kind = j.kind, stage = j.stage })
end

-- The colonists, by name, a number in it counted as one ("Ada 2" before
-- "Ada 10").
local function people(q)
  local list = {}
  for e, c in q:each("Colonist") do
    local base, n = string.match(c.name, "^(.-)%s*(%d*)$")
    table.insert(list, { id = e:id(), name = c.name, base = base, n = tonumber(n) or 0 })
  end
  table.sort(list, function(a, b)
    if a.base ~= b.base then return a.base < b.base end
    return a.n < b.n
  end)
  return list
end

-- Who has a worker's name.
local function name_of(e)
  if e ~= nil and e:has("Colonist") then return e:get("Colonist").name end
  return "?"
end

local function person_page(q, e)
  local c = e:get("Colonist")
  local w = e:get("Worker")
  local nd = e:get("Needs")
  local at = e:get("Transform").pos
  local out = {
    q:text("inspect.person.name", { name = c.name }),
    q:text("inspect.person.skills", { skills = if w.skills ~= "" then w.skills else q:text("inspect.none") }),
    q:text("inspect.person.needs", { fed = pct(nd.fed), rest = pct(nd.rest) }),
  }
  if w.job ~= nil then
    local j = w.job:get("Job")
    local work = if j.work > 0 then j.work else 1
    table.insert(out, q:text("inspect.person.doing", { kind = j.kind, stage = j.stage, progress = pct(math.min(1, j.progress / work)) }))
  else
    table.insert(out, q:text("inspect.person.idle", { why = if w.idle ~= "" then w.idle else q:text("inspect.none") }))
  end
  table.insert(out, q:text("inspect.person.where", { x = math.floor(at.x + 0.5), y = math.floor(at.y + 0.5) }))
  return out
end

local function stores_page(q)
  local meal, crop, wood, goods, food = 0, 0, 0, 0, 0
  for _, s in q:each("Stockpile") do
    meal += s.meal
    crop += s.crop
    wood += s.wood
    goods += s.goods
    food += s.food
  end
  local n, kinds = 0, {}
  for _, it in q:each("Item") do
    n += 1
    local k = string.gsub(it.kind, "^item:", "")
    kinds[k] = (kinds[k] or 0) + math.floor(it.count + 0.5)
  end
  local names = {}
  for k, v in pairs(kinds) do table.insert(names, k .. " " .. v) end
  table.sort(names)
  local cook = nil
  for _, s in q:each("Station") do
    if s.job ~= nil and s.job:get("Job").worker ~= nil then cook = name_of(s.job:get("Job").worker) end
  end
  return {
    q:text("inspect.stores.pile", { meals = math.floor(meal), crops = math.floor(crop), wood = math.floor(wood), goods = math.floor(goods) }),
    q:text("inspect.stores.food", { food = string.format("%.1f", food) }),
    q:text("inspect.stores.lying", { n = n, kinds = if #names > 0 then table.concat(names, ", ") else q:text("inspect.none") }),
    q:text("inspect.stores.stove", { state = if cook ~= nil then q:text("inspect.stores.cooking", { who = cook }) else q:text("inspect.stores.cold") }),
  }
end

local function jobs_page(q)
  local kinds, order = {}, {}
  for _, k in q:each("JobKind") do
    kinds[k.name] = { open = 0, taken = 0, priority = k.priority }
    table.insert(order, k.name)
  end
  table.sort(order, function(a, b) return kinds[a].priority > kinds[b].priority end)
  local taken = {}
  for _, j in q:each("Job") do
    local k = kinds[j.kind]
    if k == nil then
      k = { open = 0, taken = 0, priority = 0 }
      kinds[j.kind] = k
      table.insert(order, j.kind)
    end
    if j.worker ~= nil then
      k.taken += 1
      table.insert(taken, { who = name_of(j.worker), kind = j.kind, stage = j.stage })
    else
      k.open += 1
    end
  end
  table.sort(taken, function(a, b) return a.who < b.who end)
  local out = {}
  for _, name in ipairs(order) do
    local k = kinds[name]
    table.insert(out, q:text("inspect.jobs.kind", { kind = name, open = k.open, taken = k.taken, priority = k.priority }))
  end
  if #taken == 0 then table.insert(out, q:text("inspect.jobs.none")) end
  for _, t in ipairs(taken) do
    if #out >= LINES.jobs then break end
    table.insert(out, q:text("inspect.jobs.job", t))
  end
  return out
end

return system "sim.colony.inspector" {
  stage = "sim",
  after = { "ui.input" },
  -- It answers while the game is paused.
  unfrozen = true,
  reads = { "Actions", "Colony", "Stockpile", "Colonist", "Needs", "Worker", "Job", "JobKind", "Field", "Tree", "Item", "Station", "Grave", "Transform", "InspectPage", "InspectLine", "Vignette", "Menu" },
  writes = { "Inspector", "InspectRow", "UiRoot", "UiNode", "UiText", "UiButton", "UiPanel", "Camera" },
  doc = "Runs the inspector: Tab (inspect) opens and closes it, but not while the menu is open; its tabs, a colonist's button, Follow, Back, the page arrows, and cancel move between its pages; while it is open the arrows move its focus, not the view, and Follow has the view follow the person shown. Six times a second of the player's time (every ten ticks at 1x, a hundred at 10x) the page shown is written again from the world: the colony (day, food, people, the lost, fields, trees, jobs, the most common reason for idling), the people ten at a time (fed, rest, what they are doing), one person (skills, needs, their job and how far along, or why they are idle), the stores (the stockpile, what is lying about, the stove), and the jobs (by kind, open and taken, and who has what).",
  run = function(q)
    local ie = nil
    for e in q:each("Inspector") do
      ie = q:entity(e:id())
    end
    if ie == nil then return end
    local ins = ie:get("Inspector")
    local was = { open = ins.open, page = ins.page }
    local focus = nil

    -- Closed, and staying closed: nothing to do (the tick it closed put
    -- the view back). The colony's own ticks pay nothing for it.
    local toggled = q:entity("@colony"):get("Actions").inspect_pressed
    -- Tab does nothing while the menu is open (systems/menu.luau).
    if toggled then
      for _, _, n in q:each("Menu", "UiNode") do
        if n.visible then toggled = false end
      end
    end
    if not ins.open and not toggled then return end
    if toggled then
      ins.open = not ins.open
    end
    for _, name in ipairs(TABS) do
      if #q:events("inspect.tab." .. name) > 0 then ins.page = name end
    end
    for _, ev in ipairs(q:events("ui.pressed")) do
      if ev.entity ~= nil and ev.entity:has("InspectRow") then
        local who = ev.entity:get("InspectRow").who
        if who ~= nil then
          ins.person = who
          ins.page = "person"
          ins.follow = false
        end
      end
    end
    -- The colonists by name, sorted only when a page of them is needed.
    local list = nil
    local function listed()
      if list == nil then list = people(q) end
      return list
    end
    local function pages()
      return math.max(1, math.ceil(q:count("Colonist") / ROWS))
    end
    if #q:events("inspect.prev") > 0 and ins.list > 0 then ins.list -= 1 end
    if #q:events("inspect.next") > 0 and ins.list + 1 < pages() then ins.list += 1 end
    ins.list = math.min(ins.list, pages() - 1)
    if #q:events("inspect.follow") > 0 then ins.follow = not ins.follow end
    if #q:events("inspect.back") > 0 or (ins.open and #q:events("ui.cancel") > 0) then
      if ins.page == "person" then
        ins.page = "people"
      elseif #q:events("ui.cancel") > 0 then
        ins.open = false
      end
    end
    -- Someone shown who is gone (lost to hunger): back to the list.
    if ins.page == "person" and (ins.person == nil or not ins.person:alive()) then
      ins.page = "people"
    end
    local moved = ins.open ~= was.open or ins.page ~= was.page

    -- Shown, and its pages; the view's arrows are the menu's while open.
    ie:get("UiNode").visible = ins.open
    for e, pg in q:each("InspectPage") do
      e:get("UiNode").visible = pg.name == ins.page
    end
    -- The player's view (not a vignette's camera).
    for e, cam in q:each("Camera") do
      if not e:has("Vignette") then
        cam.pan_action = if ins.open then "" else "pan"
        cam.target = if ins.open and ins.follow and ins.page == "person" then ins.person else nil
      end
    end
    if not ins.open then return end

    -- Where the focus goes when the page changes.
    local root = ie:get("UiRoot")
    if moved then
      local want = if ins.page == "person" then "inspect.back" else "inspect.tab." .. (if ins.page == "people" then "people" else ins.page)
      for e, b in q:each("UiButton") do
        if b.event == want then focus = e:id() end
      end
      if focus ~= nil then root.focus = q:entity(focus) end
      ins.refresh = 0
    end

    -- The words, six times a second of the player's time (every ten ticks
    -- at 1x, every hundred at 10x), and at once on a change.
    if ins.refresh > 0 and #q:events("ui.pressed") == 0 then
      ins.refresh -= 1
      return
    end
    -- The buttons' state with the words: a page, a list page, and Follow
    -- change only by a press or a page change, which refresh at once. (Every
    -- tick, this walk of every button in the game was most of what the open
    -- inspector cost: 0.05 ms a tick in the stress scene.)
    for e, b in q:each("UiButton") do
      if string.sub(b.event, 1, 12) == "inspect.tab." then
        local tab = string.sub(b.event, 13)
        local on = tab == ins.page or (tab == "people" and ins.page == "person")
        e:get("UiPanel").color = if on then "chosen" else "button"
      elseif b.event == "inspect.prev" then
        b.disabled = ins.list == 0
      elseif b.event == "inspect.next" then
        b.disabled = ins.list + 1 >= pages()
      elseif b.event == "inspect.follow" then
        e:get("UiText").key = if ins.follow then "inspect.person.unfollow" else "inspect.person.follow"
      end
    end
    local rules = q:entity("@colony"):get("Colony")
    ins.refresh = 10 * math.max(1, math.floor(q:speed()))
    local words
    if ins.page == "colony" then
      words = colony_page(q)
    elseif ins.page == "person" then
      words = person_page(q, ins.person)
    elseif ins.page == "stores" then
      words = stores_page(q)
    elseif ins.page == "jobs" then
      words = jobs_page(q)
    else
      words = { q:text("inspect.people.page", { page = ins.list + 1, pages = pages() }) }
      local all = listed()
      for e, r in q:each("InspectRow") do
        local at = all[ins.list * ROWS + r.n]
        local shown = e:get("UiNode")
        if at == nil then
          r.who = nil
          shown.visible = false
        else
          local c = q:entity(at.id)
          r.who = c
          shown.visible = true
          local nd = c:get("Needs")
          e:get("UiText").text = q:text("inspect.people.row", {
            name = at.name,
            fed = pct(nd.fed),
            rest = pct(nd.rest),
            doing = doing(q, c:get("Worker")),
          })
        end
      end
    end
    for e, l in q:each("InspectLine") do
      if l.page == ins.page then
        local w = words[l.n] or ""
        e:get("UiText").text = w
        e:get("UiNode").visible = w ~= ""
      end
    end
  end,
}
