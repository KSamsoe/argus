-- The dungeon's tiles: 32 frames of 16 x 16 pixels, a cell each, drawn by
-- small rules with seeded noise. The floor layer of
-- maps/dungeon.tilemap.toml draws frame 0 under everything;
-- systems/look.luau paints the rest on the look layers by number (tile n
-- is frame n - 1), so keep each frame where it is. Nothing here collides:
-- the walls layer does that.
--
--   0-7    the floor: 0 one slab, 1-4 two to four stones, 5 cracked, 6 mossy, 7 bones
--   8-15   a wall from above beside a room, 8 + mask (1 floor above, 2 right, 4 left)
--   16-19  a wall's face (floor below it), 16 + sides (1 open left, 2 open right)
--   20     a wall's face with moss
--   21     a torch's light on the floor below it
--   22     rock far from any room
--   23     a wall's face with a torch
--   24-27  a wall's shadow on the floor: from above, from the left, both, the corner
--   28-31  what a fight leaves: a rat's, a goblin's, a bat's, blood
--
-- What to change first: the colours just below, and SEED for other stones
-- and specks. Change one and save, and the game shows it while it runs.
-- The camera (scenes/dungeon/camera.toml) shows 32 screen pixels a cell at
-- 960 x 540, so each pixel here is drawn 2 x 2. The colours the creatures
-- and the UI plates use too (shared.X below) are in assets/shared.lib.luau.

-- The colours and the colour mixing the art scripts share.
local shared = require("shared")
local mix = shared.mix

local CELL = 16 -- pixels a cell
local SEED = 7 -- the noise's seed: another number, other specks and stones

-- ----- the palette -------------------------------------------------------

local INK = shared.INK -- the creatures' line, mixed into the rock for its pits
local VOID = "#0C0B10" -- the rock far from any room (the same as the camera's Background)
local VOID_SPECK = "#141219"

-- The floor: flagstones in a cool grey, a little violet, in dark mortar.
local MORTAR = "#1C1922"
local STONE = { "#36313D", "#3D3745", "#45404F" } -- one picked for each stone
local STONE_HI = "#534C60"
local STONE_LO = "#2B2732"

-- A wall seen from above (the rock beside a room) and its lit rim.
local ROCK = "#1F1C25"
local ROCK_SPECK = "#2A2631"
local RIM = "#6A6179"
local RIM_LO = "#3E3849"

-- A wall's face: three courses of bricks under a lit cap, a dark foot.
local CAP = "#756B84"
local CAP_LO = "#5A5167"
local BRICK = { "#4F4660", "#584E6A", "#494058" } -- one picked for each brick
local BRICK_HI = "#6B607C"
local BRICK_LO = "#3B3348"
local BRICK_MORTAR = "#27212E"
local FOOT = "#131017"

-- The torches on the walls, and the glow they put on the bricks.
local FLAME_CORE = "#FFF2B0"
local FLAME = "#FFB43C"
local FLAME_OUT = "#E8582A"
local GLOW = "#F0A050"
local TORCH_WOOD = "#6A4424"
local BRACKET = shared.IRON_LO -- the torch's iron bracket (the UI plates' dark iron)

-- What lies about: moss, bones, and what a fight leaves.
local MOSS = { "#2E482D", "#3C6037", "#567E44" } -- dark to light
local BONE = "#D8CFB2"
local BONE_LO = "#9A9078"
local BLOOD = "#6C1A24"
local BLOOD_HI = "#952632"
-- What each monster leaves, in its own colours (sprites/creatures.art.luau).
local RAT_FUR, RAT_TAIL = shared.RAT_FUR_SHADE, shared.RAT_PINK
local KNIFE, KNIFE_HILT = shared.GOBLIN_BLADE, shared.GOBLIN_HILT -- the goblin's
local BAT_WING = shared.BAT_WING_SHADE

local SHADOW = "#000000" -- a wall's shadow, as dark as SHADE_TOP and SHADE_LEFT say

-- ----- the rules ---------------------------------------------------------

local r = art.rng(SEED)

-- Moss on the floor and on a wall's foot: x, y, and its shade in MOSS.
local FLOOR_MOSS = {
  { 2, 11, 2 }, { 3, 11, 3 }, { 3, 12, 2 }, { 4, 12, 1 }, { 2, 12, 1 }, { 4, 11, 2 },
  { 5, 12, 2 }, { 3, 13, 1 }, { 11, 4, 2 }, { 12, 4, 1 }, { 11, 5, 1 }, { 12, 3, 3 },
}
local WALL_MOSS = {
  { 1, 12, 1 }, { 2, 12, 2 }, { 2, 13, 1 }, { 3, 13, 2 }, { 3, 12, 3 }, { 4, 13, 1 },
  { 10, 13, 1 }, { 11, 12, 3 }, { 11, 13, 2 }, { 12, 13, 1 }, { 12, 12, 2 },
}

local function moss(c, spots)
  for _, p in spots do
    c:pixel(p[1], p[2], MOSS[p[3]])
  end
end

-- A floor tile of flagstones, each stone a rectangle {x0, y0, x1, y1}
-- (its last row and column its own): mortar along the tile's top row and
-- left column (the neighbours close the other two sides), each stone a
-- shade of its own with a lit top-left edge, a dark bottom-right one,
-- chipped corners, and a few specks. extra, if given, lays something on it.
local function stones(rects, extra)
  local c = art.canvas(CELL, CELL, MORTAR)
  for _, s in rects do
    local x0, y0, x1, y1 = s[1], s[2], s[3], s[4]
    local base = r:pick(STONE)
    for y = y0, y1 do
      for x = x0, x1 do
        local colour = base
        if y == y0 or x == x0 then
          colour = mix(base, STONE_HI, 0.55)
        end
        if y == y1 or x == x1 then
          colour = mix(base, STONE_LO, 0.7)
        end
        c:pixel(x, y, colour)
      end
    end
    -- Each corner chipped, more often than not.
    for _, p in { { x0, y0 }, { x1, y0 }, { x0, y1 }, { x1, y1 } } do
      if r:chance(0.6) then
        c:pixel(p[1], p[2], MORTAR)
      end
    end
    -- A speck for every 16 pixels of stone (one at least), darker or lighter.
    for _ = 1, math.max(1, (x1 - x0 + 1) * (y1 - y0 + 1) // 16) do
      local x = r:int(x0 + 1, math.max(x0 + 1, x1 - 1))
      local y = r:int(y0 + 1, math.max(y0 + 1, y1 - 1))
      local dark = r:chance(0.5)
      c:pixel(x, y, mix(base, dark and STONE_LO or STONE_HI, 0.45))
    end
  end
  if extra then
    extra(c)
  end
  return c
end

-- The stones' layouts: one slab, two side by side, two one above the
-- other, three, and four.
local WHOLE = { { 1, 1, 15, 15 } }
local TWO = { { 1, 1, 7, 15 }, { 9, 1, 15, 15 } }
local LEVEL = { { 1, 1, 15, 7 }, { 1, 9, 15, 15 } }
local THREE = { { 1, 1, 8, 7 }, { 10, 1, 15, 7 }, { 1, 9, 15, 15 } }
local FOUR = { { 1, 1, 7, 7 }, { 9, 1, 15, 7 }, { 1, 9, 7, 15 }, { 9, 9, 15, 15 } }

-- What lies on a stone: a crack down it, wandering a pixel left or right
-- now and then; moss; bones.
local function crack(c)
  local x = r:int(5, 9)
  for y = 2, 14 do
    c:pixel(x, y, MORTAR)
    x = math.min(13, math.max(3, x + r:pick({ -1, 0, 0, 1 })))
  end
end

local function floor_moss(c)
  moss(c, FLOOR_MOSS)
end

local function bones(c)
  for _, p in { { 5, 10 }, { 6, 9 }, { 7, 8 } } do
    c:pixel(p[1], p[2], BONE)
  end
  for _, p in { { 4, 10 }, { 5, 11 }, { 7, 7 }, { 8, 8 }, { 11, 12 } } do
    c:pixel(p[1], p[2], BONE_LO)
  end
end

-- Rock from above: a dark mass with specks and a few darker pits.
local function rock(base, speck, specks)
  local c = art.canvas(CELL, CELL, base)
  for _ = 1, specks do
    local x = r:int(0, CELL - 1)
    local y = r:int(0, CELL - 1)
    c:pixel(x, y, speck)
  end
  for _ = 1, specks // 3 do
    local x = r:int(0, CELL - 1)
    local y = r:int(0, CELL - 1)
    c:pixel(x, y, mix(base, INK, 0.6))
  end
  return c
end

-- Frames 8-15: a wall from above beside a room, a lit rim on each side
-- that meets open floor: 8 + mask, mask 1 the cell above, 2 right, 4 left.
local function top(mask)
  local c = rock(ROCK, ROCK_SPECK, 10)
  for i = 0, CELL - 1 do
    if bit32.band(mask, 1) ~= 0 then
      c:pixel(i, 0, RIM)
      c:pixel(i, 1, RIM_LO)
    end
    if bit32.band(mask, 2) ~= 0 then
      c:pixel(CELL - 1, i, RIM)
      c:pixel(CELL - 2, i, RIM_LO)
    end
    if bit32.band(mask, 4) ~= 0 then
      c:pixel(0, i, RIM)
      c:pixel(1, i, RIM_LO)
    end
  end
  return c
end

-- Frames 16-20: a wall's face, the wall cell just above open floor: a lit
-- cap, three courses of bricks (each brick a shade of its own, lit along
-- its top and dark along its bottom), a dark foot. 16 + sides, sides 1
-- when its left is open too and 2 its right (a lit corner edge); 20 mossy.
local function face(sides, mossy)
  local c = art.canvas(CELL, CELL, BRICK_MORTAR)
  c:rect(0, 0, CELL, 1, CAP)
  c:rect(0, 1, CELL, 1, CAP_LO)
  c:rect(0, 14, CELL, 2, FOOT)
  -- Each course: its first row, its last, and how far it is set over.
  for _, course in { { 2, 4, 0 }, { 6, 8, 4 }, { 10, 12, 0 } } do
    local y0, y1, shift = course[1], course[2], course[3]
    for x = -shift, CELL - 1, 8 do
      local base = r:pick(BRICK)
      for y = y0, y1 do
        local colour = base
        if y == y0 then
          colour = mix(base, BRICK_HI, 0.6)
        elseif y == y1 then
          colour = mix(base, BRICK_LO, 0.6)
        end
        -- Seven pixels a brick, wrapping round the tile's sides.
        for xx = x, x + 6 do
          c:pixel(xx % CELL, y, colour)
        end
      end
    end
  end
  if bit32.band(sides, 1) ~= 0 then
    c:rect(0, 0, 1, 14, CAP)
  end
  if bit32.band(sides, 2) ~= 0 then
    c:rect(CELL - 1, 0, 1, 14, CAP)
  end
  if mossy then
    moss(c, WALL_MOSS)
  end
  return c
end

-- Frame 21: a torch's warm light on the floor below it (a layer over the
-- floor, in place of the wall's shadow there).
local function glow()
  local c = art.canvas(CELL, CELL)
  for y = 0, CELL - 1 do
    for x = 0, CELL - 1 do
      -- An oval brightest at the wall, gone before the cell's edges.
      local d = (((x - 7.5) / 7.5) ^ 2 + ((y + 1.0) / 14.0) ^ 2) ^ 0.5
      local a = math.floor(85 * math.max(0, 1 - d) ^ 1.3)
      if a > 0 then
        c:pixel(x, y, art.alpha(GLOW, a / 255))
      end
    end
  end
  return c
end

-- The torch, drawn as text from (4, 1): its flame, its wood, and the iron
-- bracket holding it.
local TORCH = {
  "...o....",
  "...oy...",
  "..oyYo..",
  "..oYYo..",
  "...yy...",
  "...ww...",
  "..iwwi..",
  "..iiii..",
}
local TORCH_KEY = { o = FLAME_OUT, y = FLAME, Y = FLAME_CORE, w = TORCH_WOOD, i = BRACKET }

-- Frame 23: a wall's face with a torch, the bricks round it warmed by its
-- glow (drawn, not lit: the game has no lights).
local function torch()
  local c = face(0)
  local cx, cy, reach = 7.5, 4.0, 6.0
  for y = 0, 13 do
    for x = 0, CELL - 1 do
      local d = ((x - cx) ^ 2 + (y - cy) ^ 2) ^ 0.5
      if d < reach then
        c:pixel(x, y, mix(c:get(x, y), GLOW, 0.6 * (1 - d / reach)))
      end
    end
  end
  c:rows(4, 1, TORCH, TORCH_KEY)
  return c
end

-- How dark a wall's shadow is (0 to 255), row by row below a wall above,
-- and column by column beside a wall to the left.
local SHADE_TOP = { 130, 90, 55, 25 }
local SHADE_LEFT = { 90, 50, 20 }

-- Frames 24-27: the shadow a wall casts on the floor beside it (a layer
-- over the floor): N from the wall above, W from the left, NW both, C a
-- wall only up and to the left.
local function shade(kind)
  local c = art.canvas(CELL, CELL)
  for y = 0, CELL - 1 do
    for x = 0, CELL - 1 do
      local a = 0
      if (kind == "N" or kind == "NW") and y < #SHADE_TOP then
        a = math.max(a, SHADE_TOP[y + 1])
      end
      if (kind == "W" or kind == "NW") and x < #SHADE_LEFT then
        a = math.max(a, SHADE_LEFT[x + 1])
      end
      if kind == "C" and x < 3 and y < 3 then
        a = math.max(a, SHADE_LEFT[math.max(x, y) + 1])
      end
      if a > 0 then
        c:pixel(x, y, art.alpha(SHADOW, a / 255))
      end
    end
  end
  return c
end

-- The blood a fight leaves: a splash (a bat's, smaller, is its first six),
-- more of it for a goblin and where the hero fell, and three drops.
local SPLASH = { { 6, 7 }, { 7, 7 }, { 8, 7 }, { 7, 8 }, { 8, 8 }, { 6, 8 }, { 5, 8 }, { 9, 8 }, { 7, 6 }, { 8, 9 }, { 7, 9 } }
local SPLASH_MORE = { { 4, 7 }, { 5, 7 }, { 9, 7 }, { 10, 8 }, { 6, 9 }, { 9, 9 }, { 5, 9 }, { 8, 10 }, { 6, 6 }, { 11, 9 }, { 4, 9 } }
local DROPS = { { 3, 11 }, { 12, 5 }, { 11, 12 } }

local function dots(c, spots, colour)
  for _, p in spots do
    c:pixel(p[1], p[2], colour)
  end
end

-- Frames 28-31: a floor slab with what a fight left on it: a rat's, a
-- goblin's, a bat's, and plain blood (where the hero fell).
local function remains(kind)
  local c = stones(WHOLE)
  for i = 1, kind == "bat" and 6 or #SPLASH do
    c:pixel(SPLASH[i][1], SPLASH[i][2], BLOOD)
  end
  if kind == "goblin" or kind == "blood" then
    dots(c, SPLASH_MORE, BLOOD)
  end
  dots(c, { { 7, 7 }, { 8, 8 } }, BLOOD_HI)
  dots(c, DROPS, BLOOD)
  if kind == "bat" then
    -- A torn wing.
    dots(c, { { 9, 9 }, { 10, 9 }, { 10, 10 }, { 11, 10 }, { 12, 10 }, { 11, 11 } }, BAT_WING)
  elseif kind == "rat" then
    -- A tuft of fur and the tail.
    dots(c, { { 10, 10 }, { 11, 10 }, { 10, 11 } }, RAT_FUR)
    dots(c, { { 12, 11 }, { 13, 12 }, { 13, 13 } }, RAT_TAIL)
  elseif kind == "goblin" then
    -- The dropped knife.
    dots(c, { { 10, 12 }, { 11, 12 }, { 12, 12 }, { 13, 12 } }, KNIFE)
    dots(c, { { 9, 12 }, { 8, 12 } }, KNIFE_HILT)
  end
  return c
end

-- ----- the frames, in order ----------------------------------------------

local frames = {}
local function add(c)
  table.insert(frames, c)
end

-- 0-7: the floor. 0 is one plain slab (the floor layer's tile, under the
-- look); systems/look.luau picks among all eight.
add(stones(WHOLE))
add(stones(TWO))
add(stones(LEVEL))
add(stones(THREE))
add(stones(FOUR))
add(stones(WHOLE, crack))
add(stones(THREE, floor_moss))
add(stones(FOUR, bones))
for mask = 0, 7 do
  add(top(mask)) -- 8-15
end
for sides = 0, 3 do
  add(face(sides)) -- 16-19
end
add(face(0, true)) -- 20
add(glow()) -- 21
add(rock(VOID, VOID_SPECK, 3)) -- 22
add(torch()) -- 23
for _, kind in { "N", "W", "NW", "C" } do
  add(shade(kind)) -- 24-27
end
for _, kind in { "rat", "goblin", "bat", "blood" } do
  add(remains(kind)) -- 28-31
end

return art.sprite{ frames = frames }
