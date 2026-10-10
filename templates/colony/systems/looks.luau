-- How the colony looks. Presentation only: the colony's systems decide
-- everything, and this gives what they decided the art tools/art.py draws,
-- each tick, by what everyone is doing. It writes nothing a gameplay system
-- reads (only Sprite, Light, and the pause screen's UiNode), and it spawns
-- nothing: the job system breaks ties by entity id, so the colony's own
-- spawns keep the ids they always had.
--
-- - Colonists: their row of sprites/colonists by name, walking the way they
--   go (from behind when it is up the screen), carrying goods over their
--   heads, picking, chopping, eating.
-- - Bushes and trees by Source.growth: a bush picked, in flower, in green
--   berries, ripe; a tree a stump, a shoot, a sapling, young, grown, and
--   shaken by each blow of the axe, wood chips flying (@chips).
-- - Goods on the ground, or over their carrier's head.
-- - The stockpile's woodpile and baskets by what is in store, the pond's
--   ripples, and at night the store's window and lantern lit, the
--   lantern's Light as bright as the night is dark.
-- - Depth: what stands lower on the screen is drawn in front.
-- - The pause screen (@paused) while the colony is paused.
--
-- It runs while paused (unfrozen) only to show the pause screen; every
-- animation follows Colony.clock, which stands still then.
--
-- To change first: the timings (STEP, BLOW, RIPPLE), and how much wood and
-- food fill the woodpile and the baskets (WOOD_LEVELS, FOOD_LEVELS).

local MEADOW = "sprites/meadow"

-- sprites/colonists: a row a colonist, 16 poses a row (its sidecar).
local POSES = 16
local ROW = { Ada = 0, Bram = 1, Cleo = 2, Dov = 3 }
local STAND, WALK, CARRY, CARRY_WALK, PICK, CHOP, EAT = 0, 1, 3, 4, 6, 8, 10
local AWAY, CARRY_AWAY = 12, 14 -- walking up the screen, seen from behind

-- Frames of sprites/meadow (its sidecar lists them, in this order).
local BUSH = 0 -- picked, in flower, green berries, ripe
local OAK, FIR = 4, 10 -- grown, shaken, young, sapling, shoot, stump
local GOODS = { food = 16, wood = 17 } -- 2 more when carried
local GOODS_SIZE = vector.create(8 / 9, 19 / 9, 0) -- the frames' pixels, at 9 a unit
local STORE, STORE_LIT = 20, 21
local WOODPILE, BASKETS = 22, 26 -- four each, from none to full
local POND = 30 -- two frames of ripples

local STEP = 8 -- walking frames a second
local BLOW = 2 -- blows of the axe a second (picking and eating keep the same beat)
local RIPPLE = 0.7 -- seconds between the pond's two frames
local WOOD_LEVELS = { 1, 8, 20 } -- wood in store for the woodpile's first, second, and third stack
local FOOD_LEVELS = { 1, 8, 20 } -- food in store for one, two, and three baskets
local LAMP = 6 -- the lantern's brightness in the dark: times @night's darkness, 0 to 1

-- Lower on the screen is nearer, so drawn in front.
local function depth(y)
  return 30 - y
end

local function level(v, steps)
  local n = 0
  for i, s in ipairs(steps) do
    if v >= s then n = i end
  end
  return n
end

local function bush_frame(growth)
  if growth >= 1 then return BUSH + 3 elseif growth >= 0.6 then return BUSH + 2 elseif growth >= 0.3 then return BUSH + 1 end
  return BUSH
end

local function tree_frame(kind, growth, shaken)
  if growth >= 1 then return kind + (if shaken then 1 else 0)
  elseif growth >= 0.75 then return kind + 2
  elseif growth >= 0.5 then return kind + 3
  elseif growth >= 0.2 then return kind + 4 end
  return kind + 5
end

-- An oak or a fir, by where the tree stands: the same every run.
local function tree_kind(p)
  if (math.floor(p.x + 0.5) * 7 + math.floor(p.y + 0.5) * 13) % 3 == 0 then return FIR end
  return OAK
end

local function working(job)
  return job ~= nil and job:alive() and job:get("Job").stage == "working"
end

local function chips(q, p, n)
  q:emit("particles.burst", { emitter = q:entity("@chips"), count = n, pos = vector.create(p.x, p.y + 0.8, 0) })
end

return system "late.colony.looks" {
  stage = "late",
  on_load = true,
  unfrozen = true,
  reads = { "Colony", "Stockpile", "Colonist", "Worker", "NavAgent", "Job", "Source", "Item", "Transform", "Sprite" },
  writes = { "Sprite", "Light", "UiNode" },
  doc = "Presentation only: picks every sprite's frame from what the colony's systems decided (colonists by name and what they are doing, bushes and trees by Source.growth, goods on the ground or carried, the woodpile and baskets by what is in store, the pond's ripples), draws what is lower on the screen in front, lights the store's lantern as dark as @night is, bursts @chips at a tree under the axe, and shows @paused while Colony.paused. Spawns nothing; writes nothing gameplay reads.",
  run = function(q)
    local colony = q:entity("@colony")
    local rules = colony:get("Colony")
    q:entity("@paused"):get("UiNode").visible = rules.paused
    if rules.paused then return end

    local clock = rules.clock
    local step = math.floor(clock * STEP) % 2
    -- Two halves to a blow: the swing, then the strike (when chips fly).
    local half = math.floor(clock * BLOW * 2)
    local strike = half % 2 == 1 and math.floor((clock - q.dt) * BLOW * 2) ~= half

    for _, c, w, agent, t, sp in q:each("Colonist", "Worker", "NavAgent", "Transform", "Sprite") do
      local job = if w.job ~= nil and w.job:alive() then w.job:get("Job") else nil
      local moving = agent.state == "moving"
      local way = agent.next - t.pos
      local away = moving and way.y > math.abs(way.x)
      local pose = STAND
      if job ~= nil and job.stage == "carrying" then
        pose = if away then CARRY_AWAY + step elseif moving then CARRY_WALK + step else CARRY
      elseif job ~= nil and job.stage == "working" then
        pose = (if job.kind == "chop" then CHOP elseif job.kind == "eat" then EAT else PICK) + half % 2
        if job.site ~= nil and job.site:alive() then
          -- Face the work.
          local dx = job.site:get("Transform").pos.x - t.pos.x
          if math.abs(dx) > 0.05 then sp.flip_x = dx < 0 end
        end
      elseif moving then
        pose = if away then AWAY + step else WALK + step
      end
      if moving and math.abs(way.x) > 0.05 then
        sp.flip_x = way.x < 0 -- face the way they go
      end
      sp.frame = (ROW[c.name] or #c.name % 4) * POSES + pose
      sp.z = depth(t.pos.y) + 0.05
    end

    for _, src, t, sp in q:each("Source", "Transform", "Sprite") do
      local p = t.pos
      if src.job_kind == "chop" then
        local axe = working(src.job)
        sp.frame = tree_frame(tree_kind(p), src.growth, axe and half % 2 == 1)
        if axe and strike then chips(q, p, 3) end
      else
        sp.frame = bush_frame(src.growth)
        sp.flip_x = (math.floor(p.x + 0.5) + math.floor(p.y + 0.5)) % 2 == 1
      end
      sp.z = depth(p.y)
    end
    -- A tree felled: a shower of chips.
    for _, ev in ipairs(q:events("job.done")) do
      if ev.kind == "chop" and ev.site ~= nil and ev.site:alive() then
        chips(q, ev.site:get("Transform").pos, 14)
      end
    end

    for _, item, t, sp in q:each("Item", "Transform", "Sprite") do
      local carried = item.job ~= nil and item.job:alive() and item.job:get("Job").stage == "carrying"
      if sp.atlas ~= MEADOW then
        sp.atlas = MEADOW
        sp.size = GOODS_SIZE
      end
      sp.frame = (GOODS[item.gives] or GOODS.food) + (if carried then 2 else 0)
      sp.z = depth(t.pos.y) + (if carried then 0.1 else 0)
    end

    -- The stockpile: its store, lit at night, and what is in store.
    local home = colony:get("Transform").pos
    local stock = colony:get("Stockpile")
    -- How dark it is, 0 to 1: the alpha systems/day.luau gives @night.
    local dark = (tonumber(string.sub(q:entity("@night"):get("Sprite").color, 8, 9), 16) or 0) / 255
    local store = q:entity("@colony/store")
    store:get("Sprite").frame = if dark > 0 then STORE_LIT else STORE
    store:get("Light").intensity = LAMP * dark
    q:entity("@colony/wood"):get("Sprite").frame = WOODPILE + level(stock.wood, WOOD_LEVELS)
    q:entity("@colony/food"):get("Sprite").frame = BASKETS + level(stock.food, FOOD_LEVELS)
    for _, part in ipairs({ "store", "wood", "food" }) do
      local e = q:entity("@colony/" .. part)
      e:get("Sprite").z = depth(home.y + e:get("Transform").pos.y)
    end
    q:entity("@pond"):get("Sprite").frame = POND + math.floor(clock / RIPPLE) % 2
  end,
}
