-- Woodcutting. The valley's trees are felled for wood: a standing tree a
-- woodcutter can get to asks for one (a chop job, JobKind chop: skill
-- chop), Colony.chop_open at a time, the nearest the stockpile first. The
-- woodcutter walks to it and chops; sim.colony.work fells it (a stump),
-- and its Colony.tree_wood logs fall at their feet for a hauler to take to
-- the stockpile (Stockpile.wood). Trees regrow: a stump grows back by
-- daylight over Colony.tree_days days, a sapling half way, and then stands
-- again to be felled once more. So the valley never runs out of wood, but
-- it gives no more than its trees regrow.
--
-- A tree blocks the way standing or felled (its NavCost stays), so the
-- grid never changes under the colonists' paths.
--
-- All of it is done once a second of the colony's time, not every tick: a
-- tree is slow, and the stress scene has three hundred of them.

local SAPLING = { size = 0.55, color = "#3E6B35" }
local TREE = { size = 0.9, color = "#1E3B1E" }

local function show(e, look)
  local s = e:get("Sprite")
  s.color = look.color
  s.size = vector.create(look.size, look.size, 0)
end

return system "sim.colony.woods" {
  stage = "sim",
  after = { "sim.colony.haul" },
  reads = { "Colony", "Transform" },
  writes = { "Tree", "Sprite", "Job" },
  doc = "Once a second of the colony's time (its clock stands still while paused): grows each felled tree's stump back if the second was daylight (1 / Colony.tree_days a day), drawn a sapling half way and a tree again when grown (tree.grown); and, while there are fewer than Colony.chop_open chop jobs, posts one (JobKind chop: skill chop) for each standing tree nearest the stockpile that a woodcutter can get to (Tree.reach) and that has none. sim.colony.work fells it on job.done.",
  run = function(q)
    local colony = q:entity("@colony")
    local rules = colony:get("Colony")
    local now = rules.clock
    if math.floor(now) == math.floor(now - q.dt) then return end
    local dl = q.day_length
    -- The colony's second just gone, by its middle: daylight, or night.
    local daylight = ((math.floor(now) - 0.5) / dl) % 1 < rules.dusk
    local grow = if daylight then 1 / (rules.tree_days * dl * rules.dusk) else 0

    local jobs = 0
    for _, j in q:each("Job") do
      if j.kind == "chop" then jobs += 1 end
    end
    local want = rules.chop_open - jobs
    local home = if colony:has("Transform") then colony:get("Transform").pos else nil
    -- The standing trees within reach without a job, by distance from the
    -- stockpile, in the order they were laid out among equals.
    local free = {}
    for e, tree, t in q:each("Tree", "Transform") do
      if tree.growth < 1 then
        if grow > 0 then
          local was = tree.growth
          tree.growth = math.min(1, was + grow)
          if tree.growth >= 1 then
            show(e, TREE)
            q:emit("tree.grown", { tree = e })
          elseif was < 0.5 and tree.growth >= 0.5 then
            show(e, SAPLING)
          end
        end
      elseif want > 0 and home ~= nil and tree.reach and (tree.job == nil or not tree.job:alive()) then
        table.insert(free, { id = e:id(), d = vector.magnitude(t.pos - home), n = #free })
      end
    end
    if #free == 0 then return end
    table.sort(free, function(a, b)
      if a.d ~= b.d then return a.d < b.d end
      return a.n < b.n
    end)
    for i = 1, math.min(want, #free) do
      local site = q:entity(free[i].id)
      local j = q:spawn({ "Job" })
      local job = j:get("Job")
      job.kind = "chop"
      job.site = site
      site:get("Tree").job = j
    end
  end,
}
