-- How Outpost's valley is drawn (the art pass). Presentation only: the
-- colony's systems lay the valley out and run it with described shapes
-- (sim.colony.found, work, woods, vignette); each tick this gives them the
-- art tools/art.py draws, and it writes nothing a gameplay system reads.
-- It spawns no entity and despawns none (the job system orders jobs by
-- their entity ids, so the colony's own spawns get the ids they always
-- did), and it puts components only on entities gameplay never iterates
-- in an order that matters: the ground, the stove, the campfire, and the
-- cameras.
--
-- - Every sprite it dresses is drawn with materials/art, its frame without
--   the tint, so the colony's colours stay as the systems write them (a
--   tree's colour says standing or stump; tests/woodcutting.luau reads it).
-- - Depth: things lower on the screen are in front (z falls with y).
-- - The ground: ground/valley for the 40 x 24 valley, else grass repeated
--   by materials/ground.
-- - Colonists: their row of colonist.png by name, walking the way they go,
--   working at their job (chopping, harvesting, cooking, eating, picking
--   up), carrying their load overhead, and asleep in their cot (the cot
--   shows them; they are hidden).
-- - Trees by Tree.growth: standing, swaying under the axe, young, a
--   sapling, a shoot, a stump. Fields by Field.growth, half cut while a
--   farmer harvests. The stockpile shows its wood, crops, and meals.
-- - The stove's fire when it is lit or cooking, the campfire's when it is
--   lit, and smoke from both; chips and chaff where the work is.
-- - The HUD, the menu, and the inspector on wooden panels; a soft vignette
--   on the cameras.

local MAT = "materials/art"
local POSES = 21
local ROW = { Ada = 0, Bram = 1, Cleo = 2, Dov = 3, Esme = 4, Finn = 5 }
-- colonist.png's poses.
local STAND, WALK_DOWN, WALK_UP, WALK_SIDE, CARRY = 0, 1, 3, 5, 9
local WORK = { chop = 11, harvest = 13, cook = 15, eat = 17 }
local PICK = 19
local ITEM = { ["item:crop"] = 0, ["item:wood"] = 1, ["item:stone"] = 2 }
local CARRIED = 3
-- bed.png: a cot with its owner's quilt folded (by row), with them asleep
-- in it, and nobody's.
local ASLEEP, NOBODY = 6, 12

local function depth(y)
  return 3 - 0.02 * y
end

local function set(c, field, v)
  if c[field] ~= v then c[field] = v end
end

local function size(s, w, h)
  local z = s.size
  if z.x ~= w or z.y ~= h then s.size = vector.create(w, h, 0) end
end

local function setz(s, z)
  if math.abs(s.z - z) > 1e-4 then s.z = z end
end

-- Dress a sprite in an atlas of Outpost's art.
local function dress(s, atlas, w, h, z)
  set(s, "atlas", atlas)
  set(s, "material", MAT)
  size(s, w, h)
  setz(s, z)
end

-- A tree's kind from where it stands: oak, pine, or birch (frames of 7).
local function tree_kind(p)
  local k = (math.floor(p.x + 0.5) * 7 + math.floor(p.y + 0.5) * 13) % 10
  if k < 5 then return 0 elseif k < 8 then return 7 end
  return 14
end

local function tree_stage(g)
  if g >= 1 then return 0 elseif g >= 0.75 then return 3 elseif g >= 0.5 then return 4 elseif g >= 0.2 then return 5 end
  return 6
end

local function field_stage(g)
  if g >= 1 then return 5 elseif g >= 0.75 then return 4 elseif g >= 0.5 then return 3 elseif g >= 0.25 then return 2 elseif g >= 0.06 then return 1 end
  return 0
end

local function level(v, a, b)
  if v <= 0.5 then return 0 elseif v <= a then return 1 elseif v <= b then return 2 end
  return 3
end

local function row_of(name)
  local base = string.match(name, "^(%a+)") or name
  local r = ROW[base]
  if r == nil then r = #base % 6 end
  return r
end

local function burst(q, bits, n, p)
  if bits ~= nil then
    q:emit("particles.burst", { emitter = bits, count = n, pos = vector.create(p.x, p.y + 0.3, 0) })
  end
end

return system "late.colony.art" {
  stage = "late",
  -- After the light, so it sees the lamps lit this tick, and after the
  -- vignette (before the light), so a grave is dressed as it is dug.
  after = { "late.colony.light" },
  on_load = true,
  -- The menu stands the colony still but not its panels; the colony's own
  -- clock (which stands still too) drives every animation.
  unfrozen = true,
  reads = { "Colony", "Stockpile", "Colonist", "Worker", "Job", "NavAgent", "Transform", "Tree", "Field", "Bed", "Item", "Station", "Lamp", "Light", "Grave", "Camera", "Hud", "MenuPage", "Inspector", "UiButton", "UiPanel", "UiNode", "UiText", "Sprite", "Emitter", "PostVignette" },
  writes = { "Sprite", "Emitter", "PostVignette", "UiPanel", "UiNode" },
  doc = "Presentation only: dresses the colony's described shapes in Outpost's art (tools/art.py) by what they are doing: the ground, colonists walking, working, carrying, and asleep in their cots, trees by growth (swaying under the axe), fields by growth, the stockpile by what it holds, the stove and campfire burning with smoke, items on the ground and carried, graves; chips and chaff bursts; the HUD, menu, and inspector panels; a vignette on the cameras. Spawns nothing and writes nothing gameplay reads.",
  run = function(q)
    local colony = q:entity("@colony")
    local rules = colony:get("Colony")
    local clock = rules.clock
    local turned = math.floor(clock) ~= math.floor(clock - q.dt)
    local every = function(n) return q.tick % n == 0 end

    -- The first look at a valley (a new one, or one from an older save):
    -- everything is dressed at once.
    local first = false
    for _, _, s in q:each("Stockpile", "Sprite") do
      if s.atlas ~= "stockpile" then first = true end
    end

    -- The ground, and the emitter whose bursts are chips and chaff.
    local bits = nil
    for e, em in q:each("Emitter") do
      if em.effect == "fx/bits" then bits = e:id() end
    end
    bits = if bits ~= nil then q:entity(bits) else nil
    if first or bits == nil then
      local W, H = math.floor(rules.size.x), math.floor(rules.size.y)
      local ground = nil
      for e, s in q:each("Sprite", "Transform") do
        if s.kind == "rect" and s.z <= -9.5 and (s.atlas == "" or s.atlas == "ground/valley" or s.material == "materials/ground") then
          ground = e:id()
        end
      end
      if ground ~= nil then
        local g = q:entity(ground)
        local s = g:get("Sprite")
        if W == 40 and H == 24 then
          set(s, "atlas", "ground/valley")
          set(s, "material", MAT)
          size(s, W + 16, H + 16)
        else
          set(s, "material", "materials/ground")
        end
        if not g:has("Emitter") then
          g:insert("Emitter", { effect = "fx/bits", playing = false })
        end
        bits = g
      end
    end

    -- Trees: when the colony's second turns (sim.colony.woods grows them
    -- then, and draws its saplings and trees), when one is felled, and at
    -- the first look.
    local felled = q:events("tree.felled")
    if first or turned then
      for _, tree, s, t in q:each("Tree", "Sprite", "Transform") do
        local p = t.pos
        local want = tree_kind(p) + tree_stage(tree.growth)
        -- (The colony's systems size a tree's shape as its stage changes.)
        if first or s.frame ~= want or s.atlas ~= "trees" then
          dress(s, "trees", 2, 3, depth(p.y))
          s.frame = want
        end
      end
    end
    for _, ev in ipairs(felled) do
      local site = ev.tree
      if site ~= nil and site:alive() and site:has("Sprite") then
        local p = site:get("Transform").pos
        local s = site:get("Sprite")
        dress(s, "trees", 2, 3, depth(p.y))
        set(s, "frame", tree_kind(p) + tree_stage(site:get("Tree").growth))
        burst(q, bits, 14, p)
      end
    end

    -- Fields: a few times a second, and at once when one is harvested.
    local harvested = {}
    -- A harvest or a felling leaves goods on the ground this tick.
    local dropped = #felled > 0
    for _, ev in ipairs(q:events("job.done")) do
      if ev.kind == "harvest" and ev.site ~= nil and ev.site:alive() then
        harvested[ev.site:id()] = true
        dropped = true
        burst(q, bits, 10, ev.site:get("Transform").pos)
      end
    end
    -- A colonist lost: a grave dug, and the vignette's camera made.
    local lost = #q:events("colonist.died") > 0
    if first or every(15) or next(harvested) ~= nil then
      for e, f, s, t in q:each("Field", "Sprite", "Transform") do
        dress(s, "field", 1, 1.5, depth(t.pos.y) - 0.01)
        if harvested[e:id()] or f.job == nil or not f.job:alive() or f.job:get("Job").stage ~= "working" then
          set(s, "frame", field_stage(f.growth))
        end
      end
    end

    -- Cots, in their owners' colours: their owner asleep in them is the
    -- colonists' pass below; here a cot whose owner is gone is nobody's.
    if first or q.tick % 60 == 30 then
      for _, b, s in q:each("Bed", "Sprite") do
        local owner = b.owner
        if owner == nil or not owner:alive() or not owner:has("Colonist") then
          dress(s, "bed", 1, 1.5, 0.5)
          set(s, "frame", NOBODY)
        elseif first or s.atlas ~= "bed" then
          dress(s, "bed", 1, 1.5, 0.5)
          local r = row_of(owner:get("Colonist").name)
          local asleep = owner:has("Sprite") and not owner:get("Sprite").visible
          set(s, "frame", if asleep then ASLEEP + r else r)
        end
      end
    end

    -- Colonists, and what they work on and carry: every one each tick in
    -- a small colony, a quarter of them in turn in a big one (the stress
    -- scene's two hundred), so the pass stays a small share of the tick.
    local walk = math.floor(clock * 8) % 4
    local n = q:count("Colonist")
    local slices = if n > 100 then 8 elseif n > 24 then 4 else 1
    local slot = q.tick % slices
    local i = 0
    for e, _, s in q:each("Colonist", "Sprite") do
      i += 1
      if i % slices == slot and e:has("Worker") and e:has("NavAgent") then
        local who, w, nav, t = e:get("Colonist"), e:get("Worker"), e:get("NavAgent"), e:get("Transform")
        local was = s.frame
        local row
        if s.atlas == "colonist" then
          row = was - was % POSES
        else
          row = row_of(who.name) * POSES
          dress(s, "colonist", 1.75, 2.25, 0)
          was = -1
        end
        local p = t.pos
        local pose, flip, shown = STAND, s.flip_x, true
        local job = w.job
        local j = if job ~= nil and job:alive() then job:get("Job") else nil
        local moving = nav.state == "moving"
        local stage = if j ~= nil then j.stage else ""
        if moving and (j == nil or stage == "going" or stage == "") then
          local d = nav.next - p
          if math.abs(d.x) >= math.abs(d.y) * 0.7 then
            flip = d.x < 0
            pose = WALK_SIDE + walk
          elseif d.y > 0 then
            pose = WALK_UP + walk % 2
          else
            pose = WALK_DOWN + walk % 2
          end
        elseif j == nil then
          pose = STAND
        elseif stage == "carrying" then
          pose = CARRY
          if moving then
            pose += math.floor(clock * 6) % 2
            local dx = nav.next.x - p.x
            if math.abs(dx) > 0.05 then flip = dx < 0 end
          end
          local load = j.carry
          if load ~= nil and load:alive() and load:has("Sprite") then
            local ls = load:get("Sprite")
            if ls.frame < CARRIED or ls.atlas ~= "items" then
              dress(ls, "items", 1, 2.5, 0)
              set(ls, "frame", CARRIED + (ITEM[load:get("Item").kind] or 2))
            end
            setz(ls, depth(p.y) + 0.002)
          end
        elseif stage == "working" then
          local kind = j.kind
          if kind == "sleep" then
            shown = false
          else
            local site = j.site
            local live = site ~= nil and site:alive()
            local at = if live and site:has("Transform") then site:get("Transform").pos else p
            if math.abs(at.x - p.x) > 0.05 then flip = at.x < p.x end
            local base = WORK[kind]
            if base ~= nil then
              local rate = if kind == "chop" then 2.5 elseif kind == "eat" then 2 else 3
              local phase = math.floor(clock * rate) % 2
              pose = base + phase
              local struck = phase == 1 and was == row + base
              if kind == "chop" and live and site:has("Tree") then
                if site:get("Tree").growth >= 1 then
                  local sway = if phase == 1 then 1 + math.floor(clock * rate / 2) % 2 else 0
                  set(site:get("Sprite"), "frame", tree_kind(at) + sway)
                end
                if struck then burst(q, bits, 4, at) end
              elseif kind == "harvest" and live and site:has("Field") then
                set(site:get("Sprite"), "frame", 6)
                if struck then burst(q, bits, 3, at) end
              end
            elseif kind == "haul" then
              pose = PICK
            end
          end
        end
        -- Asleep, the cot shows them; awake, it is empty.
        if shown ~= s.visible then
          s.visible = shown
          local bed = who.bed
          if bed ~= nil and bed:alive() and bed:has("Sprite") then
            local bs = bed:get("Sprite")
            dress(bs, "bed", 1, 1.5, 0.5)
            set(bs, "frame", (if shown then 0 else ASLEEP) + row // POSES)
          end
        end
        if was ~= row + pose then s.frame = row + pose end
        set(s, "flip_x", flip)
        local z = depth(p.y) + 0.001
        if math.abs(s.z - z) > 0.002 then s.z = z end
      end
    end

    -- Goods on the ground (as they appear), and a load put down before it
    -- got there (a haul given up) back on the ground.
    if first or dropped or every(30) then
      for _, item, s, t in q:each("Item", "Sprite", "Transform") do
        if s.atlas ~= "items" then
          dress(s, "items", 1, 1, depth(t.pos.y) - 0.004)
          set(s, "frame", ITEM[item.kind] or 2)
        elseif s.frame >= CARRIED and every(30) then
          local j = item.job
          if j == nil or not j:alive() or j:get("Job").stage ~= "carrying" then
            dress(s, "items", 1, 1, depth(t.pos.y) - 0.004)
            set(s, "frame", ITEM[item.kind] or 2)
          end
        end
      end
    end

    -- The stockpile, by what it holds.
    if first or every(15) then
      for _, pile, s, t in q:each("Stockpile", "Sprite", "Transform") do
        dress(s, "stockpile", 3, 3.5, 0.4)
        set(s, "frame", level(pile.wood, 6, 18) * 16 + level(pile.crop, 5, 15) * 4 + level(pile.meal, 5, 14))
      end
    end

    -- The stove and the campfire: their fire, and their smoke.
    if first or every(5) then
      local flick = math.floor(clock * 7)
      local fresh = {}
      for e, lamp, light, s, t in q:each("Lamp", "Light", "Sprite", "Transform") do
        local lit = light.on
        if e:has("Station") then
          local st = e:get("Station")
          local cooking = st.job ~= nil and st.job:alive() and st.job:get("Job").stage == "working"
          dress(s, "stove", 1.5, 2.5, depth(t.pos.y))
          set(s, "frame", if cooking then 3 + flick % 2 elseif lit then 1 + flick % 2 else 0)
          lit = lit or cooking
        else
          dress(s, "campfire", 1.5, 1.75, depth(t.pos.y))
          set(s, "frame", if lit then 1 + flick % 3 else 0)
        end
        if e:has("Emitter") then
          set(e:get("Emitter"), "playing", lit)
        else
          table.insert(fresh, { id = e:id(), lit = lit })
        end
      end
      for _, f in ipairs(fresh) do
        q:entity(f.id):insert("Emitter", { effect = "fx/smoke", playing = f.lit })
      end
    end

    -- The rest only now and then: graves and the vignette's camera when a
    -- colonist is lost, the notice's plate when there is news.
    local news = lost or #q:events("day.turned") > 0
    -- (The menu and the inspector are built the first tick.)
    local plain = false
    for _, _, p in q:each("MenuPage", "UiPanel") do
      if p.image ~= "ui/panel" then plain = true end
    end
    for _, _, p in q:each("Inspector", "UiPanel") do
      if p.image ~= "ui/panel" then plain = true end
    end
    if not (first or news or plain or every(30)) then return end

    -- Graves, as they are dug.
    for _, _, s, t in q:each("Grave", "Sprite", "Transform") do
      if s.atlas ~= "grave" then dress(s, "grave", 1, 1.5, depth(t.pos.y) - 0.003) end
    end

    -- A soft vignette on every camera.
    local bare = {}
    for e in q:each("Camera") do
      if not e:has("PostVignette") then table.insert(bare, e:id()) end
    end
    for _, id in ipairs(bare) do
      q:entity(id):insert("PostVignette", { amount = 0.32, radius = 0.6, softness = 0.5 })
    end

    -- The HUD, the menu, and the inspector on wooden panels.
    for _, h, p, n, text in q:each("Hud", "UiPanel", "UiNode", "UiText") do
      -- The notice's plate only while it has news.
      if h.kind ~= "line" then set(n, "visible", text.text ~= "") end
      if p.image ~= "ui/hud" then
        p.image = "ui/hud"
        p.slice = 4
        p.scale = 2
        if h.kind == "line" then
          p.color = "#FFFFFF"
          n.pad = vector.create(14, 7, 0)
        else
          p.color = "#FFFFFFC0"
          n.pad = vector.create(12, 5, 0)
          n.offset = vector.create(12, 48, 0)
        end
      end
    end
    if plain then
      for _, _, p in q:each("MenuPage", "UiPanel") do
        p.image = "ui/panel"
        p.slice = 4
        p.scale = 2
        p.color = "#FFFFFF"
        p.border = 0
      end
      for _, _, p in q:each("Inspector", "UiPanel") do
        p.image = "ui/panel"
        p.slice = 4
        p.scale = 2
        p.color = "#FFFFFF"
        p.border = 0
      end
      for _, _, p in q:each("UiButton", "UiPanel") do
        p.image = "ui/button"
        p.slice = 3
        p.scale = 2
      end
    end
  end,
}
