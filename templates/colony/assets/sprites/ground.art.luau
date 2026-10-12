-- The meadow's grass, one picture of 40 x 24 world units (360 x 216 pixels,
-- 9 a unit) under everything (scenes/main/ground.toml shows it): the 32 x 18
-- the colonists walk on, grass in light and dark patches with tufts,
-- flowers, pebbles, and a few rocks; and round it, outside where the
-- colonists walk, longer and darker grass, so the meadow reads as a
-- clearing.
--
-- SEED picks other grass (the same every run). Change a colour or a number
-- and the game shows it at once, while it runs.

local SEED = 7 -- another number, another meadow
local PX = 9 -- pixels a world unit
local MEADOW_W, MEADOW_H = 32, 18 -- the walkable meadow, in units (scenes/main/ground.toml's NavGrid)
local GROUND_W, GROUND_H = 40, 24 -- the picture: the meadow and a border of long grass

-- The grass, the flowers, the stones, and the rocks' outline are colours
-- the meadow's things draw with too, so they are in assets/shared.lib.luau
-- (each list there runs dark to light, numbered from 0), with the helpers
-- this draws by. The brightest tufts are the ground's own.
local shared = require("shared")
local INK, GRASS, FLOWERS, STONE = shared.INK, shared.GRASS, shared.FLOWERS, shared.STONE
local BRIGHT_GRASS = "#78A256" -- the brightest tufts
local hypot, dither, value_noise = shared.hypot, shared.dither, shared.value_noise

-- A tuft's pixels, { dx, dy } each: a v, a w, a lone blade, a peak.
local TUFTS = {
  { { 0, 0 }, { 2, 0 }, { 1, 1 } },
  { { 0, 0 }, { 1, 1 }, { 2, 0 }, { 3, 1 }, { 4, 0 } },
  { { 1, 0 }, { 1, 1 } },
  { { 1, 0 }, { 0, 1 }, { 2, 1 } },
}

local rng = art.rng(SEED)
local W, H = GROUND_W * PX, GROUND_H * PX
local MX, MY = (GROUND_W - MEADOW_W) / 2 * PX, (GROUND_H - MEADOW_H) / 2 * PX -- the border, in pixels
local c = art.canvas(W, H)
local big = value_noise(rng, 7, 4) -- broad light and dark patches
local small = value_noise(rng, 22, 13) -- smaller ones over them

-- How far a pixel is beyond the meadow's edge, in units (0 on the meadow).
local function outside(x, y)
  local ox = math.max(MX - x, x + 1 - (W - MX), 0)
  local oy = math.max(MY - y, y + 1 - (H - MY), 0)
  return hypot(ox, oy) / PX
end

-- The grass: five shades by the noise, darker beyond the meadow's edge.
local level = {} -- each pixel's shade, 0 to 4
for y = 0, H - 1 do
  for x = 0, W - 1 do
    local n = 0.7 * big(x / W, y / H) + 0.3 * small(x / W, y / H)
    local v = 0.5 + n * 3.4 - math.min(outside(x, y), 2.5) * 0.75 + dither(x, y) * 0.45
    local l = math.max(0, math.min(4, math.floor(v)))
    level[y * W + x] = l
    c:pixel(x, y, GRASS[l])
  end
end

-- Tufts of grass, a shade lighter than the ground under them (a few a
-- shade darker), each with a dark pixel under it; beyond the meadow, long
-- blades instead.
for _ = 1, W * H // 40 do
  local x, y = rng:int(0, W - 1), rng:int(1, H - 4)
  local l = level[y * W + x]
  if outside(x, y) > 0.3 then
    for k = 0, rng:int(3, 5) - 1 do
      c:pixel(x, y + k, GRASS[math.min(4, l + (if k == 0 then 2 else 1))])
    end
  else
    local light = rng:float() < 0.8
    local col = if light then GRASS[math.min(4, l + 1)] else GRASS[math.max(0, l - 1)]
    if light and l >= 2 and rng:float() < 0.25 then
      col = BRIGHT_GRASS
    end
    for _, d in rng:pick(TUFTS) do
      c:pixel(x + d[1], y + d[2], col)
    end
    if light then
      c:pixel(x + 1, y + 2, GRASS[math.max(0, l - 1)])
    end
  end
end

-- Flowers, only on the meadow: a bright pixel on a dark one, in little
-- clusters, and the first few patches thick with one colour.
for k = 0, 45 do
  local cx, cy = rng:int(MX + 6, W - MX - 7), rng:int(MY + 6, H - MY - 7)
  local col = FLOWERS[rng:pick({ 0, 0, 1, 1, 2, 3 })] -- white and yellow the most
  local patch = k < 6
  local n = if patch then rng:int(14, 22) else rng:int(3, 6)
  for _ = 1, n do
    local x = if patch then cx + rng:int(-9, 9) else cx + rng:int(-5, 5)
    local y = if patch then cy + rng:int(-5, 5) else cy + rng:int(-3, 3)
    c:pixel(x, y + 1, GRASS[0])
    c:pixel(x, y, col)
  end
end

-- Pebbles, lit at the top left.
for _ = 1, 16 do
  local x, y = rng:int(8, W - 9), rng:int(8, H - 9)
  c:pixel(x, y, STONE[2])
  c:pixel(x + 1, y, STONE[1])
  c:pixel(x, y + 1, STONE[1])
  c:pixel(x + 1, y + 1, STONE[0])
end

-- A few rocks on the meadow, outlined.
local rock = art.canvas(5, 4)
rock:rows(0, 0, { ".hHh.", "hHHMm", "MMMmm", ".mmm." }, { H = STONE[2], h = STONE[1], M = STONE[1], m = STONE[0] })
rock:outline(INK)
for _ = 1, 7 do
  c:draw(rock, rng:int(MX, W - MX - 7), rng:int(MY, H - MY - 7))
end

return art.sprite{ frames = { c }, pixels_per_unit = PX }
