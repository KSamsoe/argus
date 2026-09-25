-- The valley, laid out from the seed when the colony is founded: the
-- ground as a navigation grid with trees on it, the stockpile (the colony
-- itself), a stove, fields, a bed each, the colonists with their skills,
-- the job kinds from defs/jobs.toml, a camera, and the HUD.
--
-- A colony is laid out once (Colony.laid_out). A save from before the
-- valley was (the first week's, tests/saves/week1.save) has colonists and
-- a stockpile and nothing else; the next tick lays the valley out around
-- them and gives them what a colonist has now.

local PILE = vector.create(20, 12, 0)

local function sprite(e, kind, w, h, color, z)
  e:insert("Sprite", { kind = kind, size = vector.create(w, h, 0), color = color, z = z })
end

-- Somewhere in the valley away from the colony's middle, for a tree.
local function wild(rng, W, H)
  while true do
    local x, y = rng:range(0, W - 1), rng:range(0, H - 1)
    if x < 3 or x > 27 or y < 3 or y > 19 then return x, y end
  end
end

return system "sim.colony.found" {
  stage = "sim",
  on_load = true, -- the colony is there before the first tick
  reads = { "Colony", "Colonist", "Camera", "UiRoot", "JobKind" },
  writes = { "Colony", "Stockpile", "Colonist", "Needs", "Worker", "NavAgent", "NavGrid", "NavCost", "Transform", "Sprite", "Describe", "Field", "Station", "Bed", "JobKind", "Camera", "UiRoot", "UiNode", "UiPanel", "UiText", "PlayerSlot", "Actions", "Item" },
  rng = "valley",
  doc = "Lays the valley out once (Colony.laid_out): a navigation grid of Colony.size with trees from the seed, the stockpile at the colony, a stove, ten fields, a bed each, the founders from defs/founders.toml (or Colony.colonists of them, their skills in turn), Colony.stones stones lying about, a JobKind per row of defs/jobs.toml, a camera, and the HUD. An older save's colonists get a place, a Worker, a NavAgent, and a bed.",
  run = function(q)
    local colony = q:entity("@colony")
    local rules = colony:get("Colony")
    if rules.laid_out then return end
    rules.laid_out = true
    -- An older save's calendar was the sim's.
    rules.clock = q.time
    local rng = q:rng()
    local W, H = math.floor(rules.size.x), math.floor(rules.size.y)

    -- The ground: a grid to walk, and something to see.
    local g = q:spawn({ "NavGrid" })
    local grid = g:get("NavGrid")
    grid.origin = vector.create(-0.5, -0.5, 0)
    grid.width = W
    grid.height = H
    grid.diagonal = true
    local ground = q:spawn({ "Transform" })
    ground:get("Transform").pos = vector.create((W - 1) / 2, (H - 1) / 2, 0)
    sprite(ground, "rect", W, H, "#2F4A2A", -10)
    for _ = 1, W * H // 20 do
      local x, y = wild(rng, W, H)
      local t = q:spawn({ "Transform", "NavCost" })
      t:get("Transform").pos = vector.create(x, y, 0)
      sprite(t, "circle", 0.9, 0.9, "#1E3B1E", 1)
    end

    -- The player's hands are the colony's (the speed and pause keys).
    if not colony:has("Actions") then
      colony:insert("PlayerSlot", { slot = 1 })
      colony:insert("Actions")
    end

    -- The stockpile is the colony itself.
    if not colony:has("Transform") then colony:insert("Transform", { pos = PILE }) end
    if not colony:has("Sprite") then sprite(colony, "rect", 3, 3, "#6B5B3E", -5) end
    if not colony:has("Describe") then
      colony:insert("Describe", { name = "stockpile", text = "day {Colony.day}: food {Stockpile.food}, meals {Stockpile.meal}, crops {Stockpile.crop}" })
    end

    local stove = q:spawn({ "Station", "Transform", "NavCost" })
    stove:get("Transform").pos = vector.create(24, 12, 0)
    sprite(stove, "rect", 0.9, 0.9, "#B04A2A", 2)

    -- Sown at different times: the first harvests come over the first
    -- days, not all at once.
    for i = 0, 9 do
      local f = q:spawn({ "Field", "Transform" })
      f:get("Transform").pos = vector.create(5 + i % 5, 6 + i // 5, 0)
      f:get("Field").growth = 0.9 * rng:next()
      sprite(f, "rect", 0.9, 0.9, "#6E5A2E", -4)
    end

    -- The kinds of job, as the colony weighs them (tune fields).
    if q:count("JobKind") == 0 then
      for _, row in ipairs(q:defs("jobs")) do
        local k = q:spawn({ "JobKind" })
        local kind = k:get("JobKind")
        kind.name = row.key
        kind.priority = row.priority
        kind.skill = row.skill
        kind.work = row.work
      end
    end

    -- The colonists: the founders, or an older save's, fitted out.
    local skills = {}
    for _, row in ipairs(q:defs("founders")) do skills[row.key] = row.skills end
    local ids = {}
    for e in q:each("Colonist") do table.insert(ids, e:id()) end
    if #ids == 0 then
      local founders = q:defs("founders")
      local n = if rules.colonists > 0 then rules.colonists else #founders
      for i = 1, n do
        local row = founders[(i - 1) % #founders + 1]
        local c = q:spawn({ "Colonist", "Needs" })
        c:get("Colonist").name = if i <= #founders then row.key else row.key .. " " .. i
        table.insert(ids, c:id())
      end
    end
    for i, id in ipairs(ids) do
      local c = q:entity(id)
      local who = c:get("Colonist")
      local at = vector.create(10 + (i - 1) % 20, 14 + (i - 1) // 20, 0)
      if not c:has("Transform") then c:insert("Transform", { pos = at }) end
      if not c:has("Worker") then
        local base = string.match(who.name, "^(%a+)") or who.name
        c:insert("Worker", { skills = skills[base] or "" })
      end
      if not c:has("NavAgent") then c:insert("NavAgent", { goal = c:get("Transform").pos, speed = 4 }) end
      if not c:has("Sprite") then sprite(c, "circle", 0.7, 0.7, "#F2E6C8", 5) end
      if not c:has("Describe") then c:insert("Describe", { name = who.name, text = "{Worker.idle}" }) end
      if who.bed == nil then
        local b = q:spawn({ "Bed", "Transform" })
        b:get("Transform").pos = vector.create(15 + (i - 1) % 30, 18 + (i - 1) // 30, 0)
        b:get("Bed").owner = c
        sprite(b, "rect", 0.8, 0.5, "#5A6E8C", -3)
        who.bed = b
      end
    end

    -- Stones lying about, for haulers (the stress scene's twenty thousand).
    if rules.stones > 0 then
      q:spawn_many(rules.stones, { "Item", "Transform" }, function(e)
        e:get("Item").kind = "item:stone"
        e:get("Transform").pos = vector.create(rng:range(0, W - 1), rng:range(0, H - 1), 0)
      end)
    end

    if q:count("Camera") == 0 then
      local cam = q:spawn({ "Camera", "Transform" })
      cam:get("Transform").pos = vector.create((W - 1) / 2, (H - 1) / 2, 0)
      local cc = cam:get("Camera")
      cc.height = 26
      cc.pan_action = "pan"
      cc.pan_speed = 0.8
      cc.zoom_action = "zoom"
      cc.zoom_min = 8
      cc.zoom_max = 40
    end
    if q:count("UiRoot") == 0 then
      local hud = q:spawn({ "UiRoot", "UiNode", "UiPanel", "UiText" })
      hud:get("UiRoot").interactive = false
      local n = hud:get("UiNode")
      n.absolute = true
      n.offset = vector.create(12, 10, 0)
      n.pad = vector.create(8, 4, 0)
      hud:get("UiPanel").color = "#05070A99"
      hud:get("UiText").text = "Outpost"
    end
    q:emit("colony.laid_out", { day = rules.day, colonists = #ids })
  end,
}
