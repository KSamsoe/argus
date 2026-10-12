-- What the colony's art scripts share: the colours more than one of them
-- draws with, and the helpers they draw by. A script takes it with
-- `local shared = require("shared")`. Change a colour here and every
-- sprite drawn with it changes at once, while the game runs.

local shared = {}

-- The palette. Each list runs dark to light, numbered from 0.
shared.INK = "#2A1E18" -- every outline
shared.SHADOW = "#101A0C50" -- the soft shadow under everything standing
-- The meadow's grass (sprites/ground), and tufts in the yard. The darkest
-- is also what shows past the meadow (scenes/main/camera.toml's Background).
shared.GRASS = { [0] = "#2C4724", "#34532B", "#3C5E31", "#456A37", "#51783E" }
shared.FLOWERS = { [0] = "#EEE8D2", "#F2D25A", "#E79AB4", "#A6BEEA" } -- white, yellow, pink, blue
shared.STONE = { [0] = "#5E5C54", "#85837A", "#ABA99E" }
-- Wood: the store, the axe's handle, the colonists' belts and boots, the
-- panel's frame.
shared.WOOD = { [0] = "#3A2616", "#5A3C24", "#7A5432", "#9C7246", "#BC9060" }
shared.STEEL = { [0] = "#6E747C", "#B4BCC4" } -- the axe's head, the latch, the lantern
shared.BASKET = { [0] = "#5E3E1E", "#8A6234", "#B68E56" } -- the baskets, and the colonists' bowl
shared.BERRY = { [0] = "#6E1428", "#C2405A", "#F49AAC" } -- dark, berry, shine

-- The length of (a, b).
function shared.hypot(a, b)
  return math.sqrt(a * a + b * b)
end

-- To the nearest whole number, a half to the even one.
function shared.round(v)
  local f = math.floor(v)
  local d = v - f
  if d > 0.5 or (d == 0.5 and f % 2 == 1) then
    return f + 1
  end
  return f
end

-- An ordered dither: a pixel's threshold by where it is, so shades meet in
-- a fine checker instead of a hard edge.
local BAYER = { { 0, 8, 2, 10 }, { 12, 4, 14, 6 }, { 3, 11, 1, 9 }, { 15, 7, 13, 5 } }
function shared.dither(x, y)
  return BAYER[y % 4 + 1][x % 4 + 1] / 16.0 - 0.47
end

local function ease(t)
  return t * t * (3 - 2 * t)
end

-- Smooth value noise over [0, 1] x [0, 1]: random values from rng (an
-- art.rng) on a lattice of cx x cy cells, eased between. Answers the
-- function that reads it at (u, v).
function shared.value_noise(rng, cx, cy)
  local v = {}
  for j = 0, cy do
    v[j] = {}
    for i = 0, cx do
      v[j][i] = rng:float()
    end
  end
  return function(u, w)
    local x, y = u * cx, w * cy
    local i, j = math.min(math.floor(x), cx - 1), math.min(math.floor(y), cy - 1)
    local fx, fy = ease(x - i), ease(y - j)
    local a = v[j][i] + (v[j][i + 1] - v[j][i]) * fx
    local b = v[j + 1][i] + (v[j + 1][i + 1] - v[j + 1][i]) * fx
    return a + (b - a) * fy
  end
end

-- A soft oval of SHADOW on the ground under what is drawn on c, centred
-- at (cx, cy) with radii rx and ry: only clear pixels take it.
function shared.shadow(c, cx, cy, rx, ry)
  for y = 0, c.h - 1 do
    for x = 0, c.w - 1 do
      local u, v = (x + 0.5 - cx) / rx, (y + 0.5 - cy) / ry
      if u * u + v * v <= 1 and c:alpha(x, y) == 0 then
        c:pixel(x, y, shared.SHADOW)
      end
    end
  end
end

return shared
