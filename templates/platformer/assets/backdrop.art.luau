-- Behind the level, four frames at 15 pixels a unit. Frame 0 is the sky's
-- colours, top to bottom, one column that scenes/level/sky.toml stretches
-- across the level. 1 clouds, 2 far hills, 3 near hills with trees: strips
-- 24 units wide that repeat end to end (scenes/level/clouds.toml,
-- hills.far.toml, hills.near.toml). Each strip's hills and clouds are made
-- from waves that fit the width a whole number of times, so its right end
-- meets its left. Change a colour below and the game shows it at once.

-- The scale and the helpers this script shares with sprites.art.luau
-- (assets/shared.lib.luau).
local shared = require("shared")
local PPU = shared.PPU -- pixels a world unit
local round, trunc, mix = shared.round, shared.trunc, shared.mix

-- The palette ----------------------------------------------------------------

-- The sky, top (0) to bottom (1): a colour at each point, blended between.
local SKY = {
  { 0.0, "#2E64B8" },
  { 0.35, "#4F8FD8" },
  { 0.62, "#8CC4EC" },
  { 0.72, "#C6E8F2" },
  { 1.0, "#E6F4E4" },
}
local CLOUD = "#FFFFFF"
local CLOUD_SHADE = "#D4E6F6"
local FAR_HILL = "#A9CFDC"
local FAR_HILL_SHINE = "#C4E1EA"
local FAR_HILL_SHADE = "#8DB9CC"
local NEAR_HILL = "#79BE8C"
local NEAR_HILL_SHINE = "#9CD7A4"
local NEAR_HILL_SHADE = "#4F8F72"
local TREE = "#4E9E6A"
local TREE_SHADE = "#3A7D58"

-- Sizes, in pixels.
local STRIP_W = 24 * PPU
local SKY_H = 16 * PPU -- the scene's bounds: 16 units top to bottom
local CLOUDS_H, FAR_H, NEAR_H = 4 * PPU, 5 * PPU, 8 * PPU

-- How far (dx, dy) is from (0, 0).
local function dist(dx, dy)
  return math.sqrt(dx * dx + dy * dy)
end

-- The sky -------------------------------------------------------------------------

local function sky()
  local c = art.canvas(1, SKY_H)
  for y = 0, SKY_H - 1 do
    local t = y / (SKY_H - 1)
    for i = 1, #SKY - 1 do
      local t0, c0, t1, c1 = SKY[i][1], SKY[i][2], SKY[i + 1][1], SKY[i + 1][2]
      if t0 <= t and t <= t1 then
        c:pixel(0, y, mix(c0, c1, (t - t0) / (t1 - t0)))
        break
      end
    end
  end
  return c
end

-- The clouds ------------------------------------------------------------------------

-- Each cloud: its middle across, its middle down, its size, in pixels of the
-- strip.
local CLOUDS = { { 40, 34, 1.0 }, { 170, 22, 0.75 }, { 275, 40, 1.15 } }
-- A cloud's five puffs (circles): across and up from its middle, and how
-- big, at size 1.
local PUFFS = { { -16, 0, 8 }, { -6, 5, 11 }, { 7, 3, 10 }, { 17, 0, 7 }, { 0, -1, 9 } }

-- Each cloud is its puffs cut flat along the bottom, white with a pale blue
-- shade underneath.
local function clouds()
  local c = art.canvas(STRIP_W, CLOUDS_H)
  for _, cloud in CLOUDS do
    local cx, cy, size = cloud[1], cloud[2], cloud[3]
    local puffs = {}
    for _, p in PUFFS do
      table.insert(puffs, { cx + p[1] * size, cy - p[2] * size, p[3] * size })
    end
    local bottom = cy + 6 * size
    for y = 0, trunc(bottom) do
      for x = trunc(cx - 40), trunc(cx + 40) - 1 do
        -- How far inside the nearest puff's edge (less than 0: inside).
        local d = math.huge
        for _, p in puffs do
          d = math.min(d, dist(x - p[1], y - p[2]) - p[3])
        end
        if d <= 0 then
          local under = y > bottom - 3 or (d > -1.2 and y > cy)
          c:pixel(x % STRIP_W, y, under and CLOUD_SHADE or CLOUD)
        end
      end
    end
  end
  return c
end

-- The hills -------------------------------------------------------------------------

-- The ridge's height at x, from waves: each {cycles across the strip,
-- amplitude, phase}.
local function waves(x, parts)
  local h = 0
  for _, w in parts do
    h += w[2] * math.sin(2 * math.pi * w[1] * x / STRIP_W + w[3])
  end
  return h
end

-- A strip of hills h pixels tall: the ridge `base` pixels down, lifted and
-- dropped by its waves, a lit line along the ridge, darkening toward the
-- bottom (to `shade`, if given). With trees, that many clumps of them are
-- put along the ridge at random, from `seed`.
local function hills(o)
  local h = o.h
  local c = art.canvas(STRIP_W, h)
  local tops = {}
  for x = 0, STRIP_W - 1 do
    tops[x] = round(o.base + waves(x, o.waves))
  end
  for x = 0, STRIP_W - 1 do
    for y = math.max(tops[x], 0), h - 1 do
      local col = o.fill
      if o.shade then
        col = mix(col, o.shade, math.min(1, math.max(0, (y - tops[x] - 8) / (h - 20))))
      end
      c:pixel(x, y, col)
    end
    c:pixel(x, tops[x], o.shine)
    c:pixel(x, tops[x] + 1, tops[x] < tops[(x + 1) % STRIP_W] and o.shine or o.fill)
  end
  -- Trees: round crowns on short trunks along the ridge, in clumps of one
  -- to three, lit from the top left.
  local r = art.rng(o.seed or 1)
  for _ = 1, o.trees or 0 do
    local tx = r:int(0, STRIP_W - 1)
    for k = 0, r:int(1, 3) - 1 do
      local x0 = tx + k * r:int(6, 9)
      local rad = r:float(3.4, 5.2)
      local ground = tops[trunc(x0) % STRIP_W]
      local cy = ground - rad - 1
      for y = trunc(cy), ground + 1 do
        c:pixel(trunc(x0) % STRIP_W, y, TREE_SHADE)
      end
      for y = trunc(cy - rad - 1), trunc(cy + rad + 2) - 1 do
        for x = trunc(x0 - rad - 1), trunc(x0 + rad + 2) - 1 do
          if dist(x - x0, y - cy) <= rad then
            local col = TREE_SHADE
            if dist(x - (x0 - rad * 0.35), y - (cy - rad * 0.35)) < rad * 0.55 then
              col = NEAR_HILL_SHINE
            elseif dist(x - (x0 - rad * 0.2), y - (cy - rad * 0.2)) < rad * 0.85 then
              col = TREE
            end
            c:pixel(x % STRIP_W, y, col)
          end
        end
      end
    end
  end
  return c
end

return art.sprite{
  pixels_per_unit = PPU,
  frames = {
    sky(),
    clouds(),
    hills{
      h = FAR_H, base = 26,
      waves = { { 1, 7, 0.3 }, { 3, 5, 1.7 }, { 5, 3, 4.0 }, { 8, 1.5, 2.2 } },
      fill = FAR_HILL, shine = FAR_HILL_SHINE, shade = FAR_HILL_SHADE,
    },
    hills{
      h = NEAR_H, base = 30,
      waves = { { 2, 9, 1.1 }, { 3, 6, 0.2 }, { 7, 3, 2.9 } },
      fill = NEAR_HILL, shine = NEAR_HILL_SHINE, shade = NEAR_HILL_SHADE,
      trees = 14, seed = 3,
    },
  },
}
