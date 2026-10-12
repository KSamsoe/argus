-- The meadow's things, one sprite of 33 frames: the berry bushes, the
-- trees, the goods carried home, the store, its woodpile and baskets, the
-- pond, and the stockpile's yard. systems/looks.luau picks them by number,
-- counted from 0 in the order the list at the bottom gives them. The art
-- is 9 pixels to a world unit, and each frame's pivot is where the entity
-- stands (a tree's trunk foot, a bush's base), as a fraction of the frame,
-- y up from the bottom.
--
-- Most of it is drawn by rule; the leaves, the pond's edge and ripples,
-- the store's knots, and the yard take seeded random numbers, so SEED
-- picks other ones (the same every run). Change a colour or a number and
-- the game shows it at once, while it runs.

local SEED = 7

-- The colours other scripts draw with too (the outline, the shadow, grass,
-- flowers, stone, wood, steel, baskets, berries) are in
-- assets/shared.lib.luau, with the helpers this draws by.
local shared = require("shared")
local INK, GRASS, FLOWERS, STONE = shared.INK, shared.GRASS, shared.FLOWERS, shared.STONE
local WOOD, STEEL, BASKET, BERRY = shared.WOOD, shared.STEEL, shared.BASKET, shared.BERRY

-- The meadow's own palette. Each list runs dark to light, numbered from 0.
local LEAF = { [0] = "#223C1D", "#30522A", "#426E34", "#578A42", "#74A852" }
local PINE = { [0] = "#1A3424", "#23472F", "#2F5C3C", "#3E744C", "#528E5C" }
local BARK = { [0] = "#3A281C", "#553B28", "#704E34", "#8E6A48" }
local CUT = { [0] = "#A88454", "#CFAE78", "#E6CC98" } -- a cut log's face
local UNRIPE = { [0] = "#80A040", "#BCD46C" }
local BLOSSOM = { [0] = "#F6F2E4", "#EDCB5A" }
local WATER = { [0] = "#1C3C62", "#265086", "#336AA2", "#4E88C0", "#98C8E6" }
local BANK = { [0] = "#4E4030", "#6E5C40", "#8E7A56", "#AE9A70" }
local REED = { [0] = "#3E5E28", "#628A3C", "#6A4428" } -- a reed's tip, its stem, a cattail
local DIRT = { [0] = "#5E4A34", "#7A6346", "#907858", "#A88E6C" }
local ROOF = { [0] = "#4A2416", "#6E321E", "#8E4428", "#AE5C36", "#C8784A" }
local GLASS = "#27323E" -- a window by day
local LAMPLIGHT = { [0] = "#F2A040", "#FFD27A", "#FFF2BE" } -- the window and lantern at night

-- Drawing ----------------------------------------------------------------------

local hypot, round, dither = shared.hypot, shared.round, shared.dither
local shadow, value_noise = shared.shadow, shared.value_noise

-- A pixel's place in a set of pixels (every frame is under 256 wide).
local function at(x, y)
  return y * 256 + x
end

-- Whether a pixel of a set is on its edge: a neighbour (of four) is not in it.
local function on_edge(cells, x, y)
  return not (cells[at(x + 1, y)] and cells[at(x - 1, y)] and cells[at(x, y + 1)] and cells[at(x, y - 1)])
end

-- Leaves: the union of circles { cx, cy, r }, lit from the top left, in
-- pal's shades with a darker rim; dx shifts them (a tree shaken by the
-- axe). Neighbouring leaves come in clumps a shade lighter or darker.
local function foliage(c, circles, pal, rng, dx)
  local lx, ly = -0.55, -0.8 -- the way the light comes from
  local cells, order = {}, {}
  for y = 0, c.h - 1 do
    for x = 0, c.w - 1 do
      local best = nil
      for _, o in circles do
        local ox, oy = x + 0.5 - o[1] - dx, y + 0.5 - o[2]
        local d = hypot(ox, oy)
        if d <= o[3] then
          local k = (o[3] - d) / o[3]
          if best == nil or k > best.k then
            best = { k = k, nx = ox / o[3], ny = oy / o[3] }
          end
        end
      end
      if best ~= nil then
        cells[at(x, y)] = best
        table.insert(order, { x, y })
      end
    end
  end
  local clumps = {}
  for _, p in order do
    local x, y = p[1], p[2]
    local cell = cells[at(x, y)]
    local clump = at((x + 1) // 3, (y + 2) // 3)
    if clumps[clump] == nil then
      clumps[clump] = rng:float(-0.55, 0.55)
    end
    local lit = -(cell.nx * lx + cell.ny * ly) -- 1 facing the light, -1 away
    local v = 1.9 + 1.25 * lit + clumps[clump] + 0.6 * dither(x, y)
    if (x + 1) % 3 == 0 and (y + 2) % 3 == 0 and lit > -0.2 then
      v += 0.8 -- a leaf catching the light
    end
    c:pixel(x, y, pal[math.max(1, math.min(#pal, round(v)))])
  end
  for _, p in order do
    if on_edge(cells, p[1], p[2]) then
      c:pixel(p[1], p[2], pal[0])
    end
  end
end

-- The bushes ---------------------------------------------------------------------

local BUSH_W, BUSH_H = 13, 12
local BUSH_LEAVES = { { 6.5, 5.8, 5.0 }, { 3.4, 7.6, 3.2 }, { 9.6, 7.6, 3.2 } }
local BUSH_SPOTS = { { 3, 6 }, { 6, 3 }, { 9, 5 }, { 5, 8 }, { 8, 8 }, { 10, 8 }, { 2, 9 } } -- twigs, flowers, berries

-- A berry bush: stage 0 just picked, 1 in flower, 2 green berries, 3 ripe.
-- The same leaves at every stage (the same seed); only what is on them changes.
local function bush(stage)
  local rng = art.rng(SEED + 10)
  local c = art.canvas(BUSH_W, BUSH_H)
  local pal = LEAF
  if stage == 0 then
    pal = { [0] = LEAF[0], LEAF[1], LEAF[1], LEAF[2], LEAF[3] } -- duller, picked bare
  end
  foliage(c, BUSH_LEAVES, pal, rng, 0)
  for n, spot in BUSH_SPOTS do
    local i, x, y = n - 1, spot[1], spot[2]
    if stage == 0 and i % 3 == 0 then
      c:pixel(x, y, BARK[2]) -- a bare twig
    elseif stage == 1 then
      c:pixel(x, y, BLOSSOM[0])
      if i % 2 == 1 then
        c:pixel(x + 1, y, BLOSSOM[1])
      end
    elseif stage == 2 then
      c:pixel(x, y, UNRIPE[1])
      c:pixel(x, y + 1, UNRIPE[0])
    elseif stage == 3 then
      c:pixel(x, y, BERRY[2])
      c:pixel(x + 1, y, BERRY[1])
      c:pixel(x, y + 1, BERRY[1])
      c:pixel(x + 1, y + 1, BERRY[0])
    end
  end
  c:outline(INK)
  shadow(c, 6.5, 10.9, 5.6, 1.4)
  return c
end

-- The trees -----------------------------------------------------------------------

local TREE_W, TREE_H = 19, 27
local FOOT = 24 -- the row the trunk stands on

-- A trunk over columns x0 to x1, from top down to bottom, lit on its left,
-- with a knot here and there; roots at its foot when it is wide enough.
local function trunk(c, x0, x1, top, bottom)
  bottom = bottom or FOOT
  for y = top, bottom do
    for x = x0, x1 do
      local col = if x == x0 then BARK[3] elseif x == x1 then BARK[1] else BARK[2]
      if (y * 3 + x) % 5 == 0 then
        col = BARK[1]
      end
      c:pixel(x, y, col)
    end
  end
  if x1 - x0 >= 2 then
    c:pixel(x0 - 1, bottom, BARK[1])
    c:pixel(x1 + 1, bottom, BARK[1])
  end
end

-- A felled tree's stump: its cut face, and chips round it.
local function stump(c)
  trunk(c, 7, 11, 21)
  for x = 7, 11 do
    c:pixel(x, 21, CUT[1])
  end
  c:pixel(9, 21, CUT[2])
  c:pixel(8, 21, CUT[0])
  c:pixel(10, 21, CUT[0])
  for _, p in { { 5, 24 }, { 13, 23 }, { 14, 24 } } do
    c:pixel(p[1], p[2], CUT[1]) -- chips
  end
end

-- An oak's leaves: six circles, smaller for a younger tree (scale) and
-- lower on its shorter trunk (drop).
local function oak_leaves(scale, drop)
  local circles = {}
  for _, o in { { 9.5, 8.5, 7.2 }, { 5.0, 11.5, 4.6 }, { 14.0, 11.5, 4.6 }, { 9.5, 13.0, 5.0 }, { 6.6, 5.4, 4.0 }, { 12.4, 5.4, 4.0 } } do
    table.insert(circles, { 9.5 + (o[1] - 9.5) * scale, 17 + (o[2] - 17) * scale + drop, o[3] * scale })
  end
  return circles
end

-- A fir's leaves: three tiers, each a triangle, lit from the left, a light
-- bough every third row.
local function fir_leaves(c, scale, drop, dx, rng)
  local cells, order = {}, {}
  for i = 0, 2 do
    local apex = 1 + i * 5.2
    local base = apex + 8.5
    local half = 4.0 + i * 1.9
    apex, base, half = 19 + (apex - 19) * scale + drop, 19 + (base - 19) * scale + drop, half * scale
    for y = 0, TREE_H - 1 do
      if apex <= y + 0.5 and y + 0.5 <= base then
        local w = half * (y + 0.5 - apex) / (base - apex)
        for x = 0, TREE_W - 1 do
          if math.abs(x + 0.5 - 9.5 - dx) <= w and not cells[at(x, y)] then
            cells[at(x, y)] = true
            table.insert(order, { x, y })
          end
        end
      end
    end
  end
  for _, p in order do
    local x, y = p[1], p[2]
    local side = (x + 0.5 - 9.5 - dx) / 6.0
    local v = 2.4 - 1.6 * side + 0.7 * dither(x, y) + rng:float(-0.3, 0.3)
    if y % 3 == 0 and side < 0.3 then
      v += 0.7
    end
    c:pixel(x, y, PINE[math.max(1, math.min(4, round(v)))])
  end
  for _, p in order do
    if on_edge(cells, p[1], p[2]) then
      c:pixel(p[1], p[2], PINE[0])
    end
  end
end

-- A tree: kind 0 an oak, 1 a fir. Stage 0 grown, 1 shaken by the axe, 2
-- young, 3 a sapling, 4 a shoot from the stump, 5 the stump.
local function tree(kind, stage)
  local rng = art.rng(SEED + 20 + kind)
  local c = art.canvas(TREE_W, TREE_H)
  if stage <= 3 then
    local scale = ({ [0] = 1.0, 1.0, 0.74, 0.44 })[stage]
    local top = if stage < 2 then 15 elseif stage == 2 then 17 else 19
    if stage == 3 then
      trunk(c, 9, 9, top)
    else
      trunk(c, 8, 10, top)
    end
    local dx = if stage == 1 then 1 else 0
    local drop = if stage < 2 then 0 elseif stage == 2 then 2 else 4
    if kind == 0 then
      foliage(c, oak_leaves(scale, drop), LEAF, rng, dx)
    else
      fir_leaves(c, scale, drop, dx, rng)
    end
    if stage == 1 then -- leaves shaken loose
      for _, p in { { 2, 14 }, { 16, 9 }, { 15, 17 } } do
        c:pixel(p[1], p[2], if kind == 0 then LEAF[3] else PINE[3])
      end
    end
  else
    stump(c)
    if stage == 4 then -- a shoot: { x, y, shade }
      local pal = if kind == 0 then LEAF else PINE
      for _, p in { { 9, 20, 2 }, { 9, 19, 2 }, { 8, 18, 3 }, { 10, 18, 3 }, { 8, 19, 1 }, { 10, 19, 1 } } do
        c:pixel(p[1], p[2], pal[p[3]])
      end
    end
  end
  c:outline(INK)
  shadow(c, 9.5, 24.6, if stage <= 2 then 6.4 else 4.0, 1.8)
  return c
end

-- The goods ----------------------------------------------------------------------
-- A basket of berries and a pair of logs, on the ground or carried. Both
-- are 8 x 19 frames with their pivot at the bottom: on the ground the goods
-- are in the bottom rows; carried, in the top rows, so they ride over the
-- carrier's head, in their raised hands (the job system keeps carried goods
-- where the carrier is).

local GOODS_W, GOODS_H = 8, 19
local BERRIES = { ".RrRRr.", "RRRrRRR", "bBBBBBb", "bBbBbBb", ".bBBBb." }
local LOGS = { "CccccD.", "cccccDd", "CccccD.", ".CccccD", ".cccccD" }
-- R a berry, r its shine, b and B the basket, C and c bark, D and d the cut ends.
local GOODS_KEY = {
  R = BERRY[1], r = BERRY[2], b = BASKET[0], B = BASKET[2],
  C = BARK[3], c = BARK[2], D = CUT[1], d = CUT[0],
}

local function goods(picture, carried)
  local c = art.canvas(GOODS_W, GOODS_H)
  local top = if carried then 1 else GOODS_H - 1 - #picture
  c:rows(0, top, picture, GOODS_KEY)
  c:outline(INK)
  if not carried then
    shadow(c, 4, GOODS_H - 0.6, 3.6, 0.9)
  end
  return c
end

-- The store ----------------------------------------------------------------------
-- The stockpile's store, behind its yard: a log cabin with a shingled roof,
-- a door, a window, and a lantern by the door; by night the window and the
-- lantern are lit.

local STORE_W, STORE_H = 36, 34
local STORE_BASE = 31 -- the row the store stands on

local function store(lit)
  local rng = art.rng(SEED + 30)
  local c = art.canvas(STORE_W, STORE_H)
  -- The wall: logs laid flat, a knot here and there, their ends at the corners.
  for y = 15, STORE_BASE - 1 do
    for x = 2, 33 do
      local band = (y - 15) % 3
      c:pixel(x, y, ({ [0] = WOOD[3], WOOD[2], WOOD[1] })[band])
      if band == 1 and rng:float() < 0.08 then
        c:pixel(x, y, WOOD[1])
      end
    end
    for _, x in { 2, 33 } do
      c:pixel(x, y, if (y - 15) % 3 == 1 then CUT[1] else CUT[0])
    end
  end
  -- The roof: shingles in rows, overhanging the wall, its ridge at the top.
  for y = 1, 13 do
    local inset = math.max(0, 3 - y // 4)
    for x = inset, STORE_W - inset - 1 do
      local row = (y - 1) // 3
      local col = ROOF[2]
      if (y - 1) % 3 == 2 then
        col = ROOF[1]
      elseif (x + row * 2) % 5 == 0 then
        col = ROOF[1]
      elseif (y - 1) % 3 == 0 and x < STORE_W * 0.55 then
        col = ROOF[3] -- lit on the left
      end
      c:pixel(x, y, col)
    end
  end
  for x = 3, STORE_W - 4 do
    c:pixel(x, 1, if x % 2 == 1 then ROOF[4] else ROOF[3])
  end
  for x = 0, STORE_W - 1 do
    c:pixel(x, 13, WOOD[1]) -- the eave's board
    c:pixel(x, 14, WOOD[0]) -- its shadow on the wall
  end
  -- The door: planks in a frame, a latch; at night, light through its crack.
  for y = 19, STORE_BASE - 1 do
    for x = 15, 21 do
      local edge = x == 15 or x == 21 or y == 19
      c:pixel(x, y, if edge then WOOD[0] elseif x % 2 == 1 then WOOD[1] else WOOD[2])
    end
  end
  c:pixel(20, 25, STEEL[1])
  if lit then
    for y = 20, STORE_BASE - 1 do
      c:pixel(18, y, if y % 3 ~= 0 then LAMPLIGHT[0] else WOOD[1])
    end
  end
  -- The window: shutters open, glass dark by day and warm by night.
  for y = 18, 23 do
    for x = 25, 30 do
      local col
      if x == 25 or x == 30 or y == 18 or y == 23 then
        col = WOOD[0]
      elseif lit then
        col = if (x == 27 or x == 28) and (y == 20 or y == 21) then LAMPLIGHT[2] else LAMPLIGHT[1]
      else
        col = if (x + y) % 5 ~= 0 then GLASS else STONE[1]
      end
      c:pixel(x, y, col)
    end
  end
  for y = 18, 23 do
    c:pixel(24, y, WOOD[2])
    c:pixel(31, y, WOOD[2])
  end
  -- The lantern by the door, on a hook.
  c:pixel(12, 17, INK)
  c:pixel(13, 17, INK)
  for y = 18, 21 do
    for x = 11, 13 do
      if x == 12 and (y == 19 or y == 20) then
        c:pixel(x, y, if lit then LAMPLIGHT[2] else STEEL[1])
      elseif x == 12 and y == 21 and lit then
        c:pixel(x, y, LAMPLIGHT[1])
      else
        c:pixel(x, y, if y == 18 or y == 21 or x ~= 12 then INK else STEEL[0])
      end
    end
  end
  -- The foundation stones, the door's step among them.
  for x = 1, STORE_W - 2 do
    c:pixel(x, STORE_BASE, if (x // 3) % 2 == 1 then STONE[1] else STONE[0])
  end
  c:outline(INK)
  shadow(c, 18, STORE_BASE + 1.2, 17.5, 2.2)
  return c
end

-- The woodpile and the baskets ---------------------------------------------------
-- Before the store, showing what is in it.

local PILE_W, PILE_H = 16, 14
local LOG_END = { ".bb.", "bDdb", "bdDb", ".bb." }
local LOG_KEY = { b = BARK[1], D = CUT[2], d = CUT[0] }

-- Level 0 a chopping block with its axe; 1 to 3 more and more logs, ends out.
local function woodpile(level)
  local c = art.canvas(PILE_W, PILE_H)
  if level == 0 then
    trunk(c, 6, 9, 8, 12)
    for x = 6, 9 do
      c:pixel(x, 8, CUT[1])
    end
    for i = 0, 4 do -- the axe's handle, its head in the block
      c:pixel(9 + i // 2 + i % 2, 7 - i, WOOD[3])
    end
    c:pixel(7, 7, STEEL[1])
    c:pixel(8, 7, STEEL[0])
    c:pixel(8, 6, STEEL[1])
  else
    local tiers = ({ { 3 }, { 3, 2 }, { 4, 3, 2, 1 } })[level] -- logs in each tier, bottom up
    for t, n in tiers do
      local tier = t - 1
      local x0 = (PILE_W - n * 4 + 1) // 2 - (if tier % 2 == 1 then 1 else 0) + tier
      for k = 0, n - 1 do
        c:rows(x0 + k * 4 - tier, PILE_H - 5 - tier * 3, LOG_END, LOG_KEY)
      end
    end
  end
  c:outline(INK)
  shadow(c, 8, PILE_H - 0.8, 7.2, 1.2)
  return c
end

local BASKET_PIC = { ".RrRRr.", "RRRrRRR", "bBBBBBb", "bBbBbBb", "bBBBBBb", ".bbbbb." }
local EMPTY_PIC = { ".......", ".......", "bBBBBBb", "bkkkkkb", "bBbBbBb", ".bbbbb." }
local BASKET_KEY = table.clone(GOODS_KEY)
BASKET_KEY.k = BASKET[0] -- the inside of an empty basket

-- Level 0 an empty basket; 1 to 3 baskets of berries, a few berries spilled.
local function baskets(level)
  local c = art.canvas(PILE_W, PILE_H)
  if level == 0 then
    c:rows(4, PILE_H - 7, EMPTY_PIC, BASKET_KEY)
  else
    local spots = { { 4, PILE_H - 7 }, { 0, PILE_H - 7 }, { 8, PILE_H - 7 } }
    if level == 3 then
      spots = { { 0, PILE_H - 7 }, { 8, PILE_H - 7 }, { 4, PILE_H - 11 } } -- two, and one on top
    end
    for i = 1, level do
      c:rows(spots[i][1] + 1, spots[i][2], BASKET_PIC, BASKET_KEY)
    end
    if level >= 2 then
      c:pixel(13, PILE_H - 2, BERRY[1])
      c:pixel(2, PILE_H - 1, BERRY[0])
    end
  end
  c:outline(INK)
  shadow(c, 8, PILE_H - 0.8, 7.4, 1.2)
  return c
end

-- The pond -----------------------------------------------------------------------

local POND_W, POND_H = 54, 45 -- a unit and a half wider and taller than the water's 5 x 4

-- The water within its bank: deeper toward the middle, the far bank's
-- shadow on it, lily pads, reeds at two corners. Frame 1 moves the ripples
-- (systems/looks.luau swaps the two); everything else is the same in both.
local function pond(frame)
  local rng = art.rng(SEED + 40)
  local c = art.canvas(POND_W, POND_H)
  local wob = value_noise(rng, 6, 5) -- the bank's edge, wavering
  local cx, cy, a, b = (POND_W - 1) / 2, (POND_H - 1) / 2, 26.0, 21.6
  local bed = value_noise(rng, 4, 3) -- the pond's floor: deeper in some places than others
  local depth = {}
  for y = 0, POND_H - 1 do
    for x = 0, POND_W - 1 do
      local u, v = (x - cx) / a, (y - cy) / b
      local r = (math.abs(u) ^ 2.4 + math.abs(v) ^ 2.4) ^ (1 / 2.4) + (wob(x / POND_W, y / POND_H) - 0.5) * 0.1
      local d = (1 - r) * 20.0
      if d >= 0 then
        depth[at(x, y)] = d
      end
    end
  end
  for y = 0, POND_H - 1 do
    for x = 0, POND_W - 1 do
      local d = depth[at(x, y)]
      if d ~= nil then
        local t = dither(x, y)
        local col
        if d < 2.6 then -- the bank, darker toward the water
          col = if d < 1.0 then BANK[3] elseif d + t < 1.9 then BANK[2] else BANK[1]
        elseif d < 3.4 then
          col = if y < cy then BANK[0] else WATER[3] -- the far bank's lip, the near bank's shallows
        else
          local k = (d - 3.4) * (0.6 + 0.9 * bed(x / POND_W, y / POND_H)) + t * 1.2
          col = if k < 1.2 then WATER[3] elseif k < 4.5 then WATER[2] elseif k < 9.0 then WATER[1] else WATER[0]
          if y < cy - 8 and k < 3.0 then
            col = WATER[1] -- the far bank shades the water under it
          end
        end
        c:pixel(x, y, col)
      end
    end
  end
  -- Ripples: short light lines in deep water that move a little between the frames.
  local rr = art.rng(SEED + 41)
  for _ = 1, 9 do
    local x, y
    repeat
      x, y = rr:int(10, POND_W - 14), rr:int(10, POND_H - 10)
    until (depth[at(x, y)] or 0) > 6
    x += if rr:float() < 0.5 then 2 * frame else -frame
    local n = rr:int(3, 5)
    for i = 0, n - 1 do
      c:pixel(x + i, y, if i > 0 and i < n - 1 then WATER[4] else WATER[3])
    end
  end
  -- Lily pads, one in flower: { x, y, in flower }.
  for _, pad in { { 15, 27, true }, { 19, 31, false }, { 37, 15, false } } do
    local px, py = pad[1], pad[2]
    for y = py - 2, py + 2 do
      for x = px - 3, px + 2 do
        if hypot(x + 0.5 - px, (y + 0.5 - py) * 1.3) <= 2.6 and not (x == px and y < py) then
          c:pixel(x, y, if x < px then LEAF[3] else LEAF[2])
        end
      end
    end
    if pad[3] then
      c:pixel(px - 1, py - 1, FLOWERS[2])
      c:pixel(px, py - 1, FLOWERS[0])
    end
  end
  -- Reeds at the near left and the far right, cattails on some: { x, y, how many }.
  for _, reeds in { { 6, 36, 6 }, { 44, 9, 5 } } do
    for i = 0, reeds[3] - 1 do
      local x = reeds[1] + i + i // 2
      local h = 3 + (i * 7) % 4
      local base = reeds[2] + i % 2
      for k = 0, h - 1 do
        c:pixel(x, base - k, if k < h - 1 then REED[1] else REED[0])
      end
      if i % 2 == 0 then
        c:pixel(x, base - h, REED[2])
        c:pixel(x, base - h - 1, REED[2])
      end
    end
  end
  return c
end

-- The yard -----------------------------------------------------------------------

local YARD_W, YARD_H = 40, 48
local YARD_MIDDLE = 32 -- the row of the yard's middle, where the stockpile stands

-- The stockpile's ground: trodden earth, ragged at the edge, a path worn
-- from it up to the store's door, tufts the feet have missed, a few stones.
local function yard()
  local rng = art.rng(SEED + 50)
  local c = art.canvas(YARD_W, YARD_H)
  local wob = value_noise(rng, 6, 6) -- the ragged edge
  local tone = value_noise(rng, 4, 4) -- lighter and darker earth
  local cx, cy = (YARD_W - 1) / 2, YARD_MIDDLE
  local inside, cells = {}, {}
  for y = 0, YARD_H - 1 do
    for x = 0, YARD_W - 1 do
      local r = hypot((x - cx) / 18.5, (y - cy) / 13.5) + (wob(x / YARD_W, y / YARD_H) - 0.5) * 0.3
      local path = y < cy
        and math.abs(x - cx + 0.8 * math.sin(y / 5.0))
          < 2.6 + (cx - 2.6) * (y / YARD_MIDDLE) ^ 3 * 0.25 + (wob(x / YARD_W, y / YARD_H) - 0.5) * 2
      if r <= 1 or path then
        inside[at(x, y)] = true
        table.insert(cells, { x, y })
        local v = 1.6 + (tone(x / YARD_W, y / YARD_H) - 0.5) * 1.6 + dither(x, y) * 0.3
        c:pixel(x, y, DIRT[math.max(1, math.min(3, math.floor(v)))])
      end
    end
  end
  for _, p in cells do -- a darker rim where the grass begins
    if on_edge(inside, p[1], p[2]) then
      c:pixel(p[1], p[2], DIRT[0])
    end
  end
  for _ = 1, 12 do -- tufts the feet have missed, away from the middle
    local p = rng:pick(cells)
    local x, y = p[1], p[2]
    if hypot((x - cx) / 18.5, (y - cy) / 13.5) > 0.7 then
      for _, d in { { 0, 0 }, { 2, 0 }, { 1, 1 } } do
        c:pixel(x + d[1], y + d[2], GRASS[3])
      end
    end
  end
  for _ = 1, 7 do -- stones
    local x, y = rng:int(8, YARD_W - 9), rng:int(YARD_MIDDLE - 8, YARD_H - 6)
    c:pixel(x, y, STONE[2])
    c:pixel(x + 1, y, STONE[1])
    c:pixel(x, y + 1, STONE[0])
  end
  return c
end

-- The frames ---------------------------------------------------------------------
-- In the order systems/looks.luau counts them; the pivot is how far up the
-- frame the entity stands.

local frames = {}
local function add(canvas, pivot_y)
  table.insert(frames, { canvas, pivot = { 0.5, pivot_y } })
end

for stage = 0, 3 do -- 0-3: a bush picked, in flower, in green berries, ripe
  add(bush(stage), 0.1333) -- 1.6 pixels up from the bottom
end
for kind = 0, 1 do -- 4-9 an oak, 10-15 a fir: grown, shaken, young, sapling, shoot, stump
  for stage = 0, 5 do
    add(tree(kind, stage), 0.0926) -- the trunk's foot
  end
end
add(goods(BERRIES, false), 0.0) -- 16 berries
add(goods(LOGS, false), 0.0) -- 17 logs
add(goods(BERRIES, true), 0.0) -- 18 berries carried
add(goods(LOGS, true), 0.0) -- 19 logs carried
add(store(false), 0.0735) -- 20 the store
add(store(true), 0.0735) -- 21 the store at night
for level = 0, 3 do -- 22-25 the woodpile, from none to full
  add(woodpile(level), 0.1071)
end
for level = 0, 3 do -- 26-29 the baskets, from none to full
  add(baskets(level), 0.1071)
end
add(pond(0), 0.5) -- 30, 31 the pond and its ripples
add(pond(1), 0.5)
add(yard(), 0.3229) -- 32 the yard, standing at its middle

return art.sprite{ frames = frames, pixels_per_unit = 9 }
