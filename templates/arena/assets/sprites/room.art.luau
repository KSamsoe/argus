-- The room, two frames at 12 pixels a world unit. Frame 0 is the floor,
-- 32 x 18 units: @arena draws it (scenes/main/arena.toml). Frame 1 is the
-- walls round it, 33.5 x 20.25 units and clear in the middle, where the
-- floor shows: @wall draws it (wall.toml). The floor's stones, chips,
-- scorches, and grit, and the walls' blocks and bricks, are laid at
-- random from the seeds below, so it draws the same every time. Change a
-- size, a colour, or a seed and the game shows it at once.

-- The sizes ------------------------------------------------------------------

local PPU = 12 -- pixels a world unit
local ARENA_W, ARENA_H = 32, 18 -- the arena in units: @arena's Sprite size in arena.toml
local STONE = 2 -- a floor stone's side, in units
local SIDE, SOUTH = 9, 9 -- the walls' thickness at the sides and bottom, in pixels
local CAP, FACE = 6, 12 -- the top wall: its cap, and the face below it, in pixels
local FLOOR_SEED, WALL_SEED = 11, 5 -- another number, another floor or wall

-- The colours ----------------------------------------------------------------

local FLOOR = "#2E3C45" -- the stones
local FLOOR_SEAM = "#131B20" -- the gaps between them
local FLOOR_PAINT = "#4F7C80" -- the worn rings painted on the floor
local IRON = "#3A3F47" -- the drain grate in the middle
local VIGNETTE = 0.25 -- how much darker the floor is at the walls than in the middle

local WALL_TOP = "#617986" -- the walls' tops
local WALL_LIGHT = "#86A0AD" -- their lit edge
local WALL_FACE = "#3C4E5A" -- the top wall's face
local WALL_DARK = "#25323B" -- mortar, and the face's foot
local INK = "#0B1013" -- outlines round the walls
local PENNANTS = { "#C4872A", "#A8402C" } -- the pennants on the top wall, in turn

-- Colours worked out ---------------------------------------------------------

-- Rounding (a half to the even number), a level of red, green, or blue,
-- and a colour's three levels are in assets/shared.lib.luau, which the
-- actors' sheet takes too.
local shared = require("shared")
local round, level, channels = shared.round, shared.level, shared.channels

-- The colour of three levels. The floor works out a few hundred colours
-- tens of thousands of times, so each is made once and remembered.
local joined = {}

local function colour(r, g, b)
  local key = (r * 256 + g) * 256 + b
  local col = joined[key]
  if not col then
    col = art.rgb(r, g, b)
    joined[key] = col
  end
  return col
end

-- A colour made lighter (d > 0) or darker (d < 0) by d levels of 255
-- (remembered too: the stones ask for the same few shades of FLOOR).
local shaded = {}

local function shade(col, d)
  local by = shaded[col]
  if not by then
    by = {}
    shaded[col] = by
  end
  local out = by[d]
  if not out then
    local r, g, b = channels(col)
    out = colour(level(r + d), level(g + d), level(b + d))
    by[d] = out
  end
  return out
end

-- A colour times k: darker for k under 1.
local function scale(col, k)
  local r, g, b = channels(col)
  return colour(level(r * k), level(g * k), level(b * k))
end

-- The colour t of the way from a to b.
local function mix(a, b, t)
  local r, g, bl = channels(a)
  local r2, g2, b2 = channels(b)
  return colour(level(r + (r2 - r) * t), level(g + (g2 - g) * t), level(bl + (b2 - bl) * t))
end

-- The floor ------------------------------------------------------------------
--
-- Worn stones with bevelled edges, chips, scorch marks, and grit; two rings
-- painted round the middle and a drain grate in it; the top wall's shadow,
-- and darker toward the walls.

local function draw_floor()
  local r = art.rng(FLOOR_SEED)
  local W, H = ARENA_W * PPU, ARENA_H * PPU
  local c = art.canvas(W, H, FLOOR)
  local cx, cy = W / 2 - 0.5, H / 2 - 0.5 -- the middle
  -- The seams' colour as c:get answers it (#RRGGBB in capitals), so the
  -- chips and the rings below know a seam when they come to one.
  local seam = shade(FLOOR_SEAM, 0)

  -- A stone over sx..sx + w - 1, sy..sy + h - 1, in its own shade (one of
  -- TONES): a seam along its top and left, a lit bevel inside that, a dark
  -- one at its bottom and right, and a little grain (one of GRAIN a pixel).
  local TONES = { -7, -4, -2, 0, 0, 2, 3, 6 }
  local GRAIN = { -2, -1, 0, 0, 0, 1, 2 }
  local function stone(sx, sy, w, h)
    local tone = r:pick(TONES)
    for ly = 0, h - 1 do
      for lx = 0, w - 1 do
        local x, y = sx + lx, sy + ly
        if x >= 0 and x < W and y >= 0 and y < H then
          -- A stone is a little lighter at its top left, where the light
          -- comes from, than at its bottom right.
          local lean = round(4 - 8 * (lx / w + ly / h) / 2)
          local col
          if lx == 0 or ly == 0 then
            col = FLOOR_SEAM
          elseif lx == 1 or ly == 1 then
            col = shade(FLOOR, tone + 10)
          elseif lx == w - 1 or ly == h - 1 then
            col = shade(FLOOR, tone - 10)
          else
            col = shade(FLOOR, tone + lean + r:pick(GRAIN))
          end
          c:pixel(x, y, col)
        end
      end
    end
  end

  -- The stones, laid in courses (rows) a stone tall. Each course starts up
  -- to a stone left of the floor's edge, and its stones are 1 to 2 stones
  -- long, so the joints do not line up.
  local tall = STONE * PPU
  for sy = 0, H - 1, tall do
    local sx = -r:int(0, tall - 1)
    while sx < W do
      local w = math.floor(r:pick({ 1, 1.25, 1.5, 1.5, 2 }) * tall)
      stone(sx, sy, w, tall)
      sx += w
    end
  end

  -- A few chips and cracks: short dark runs, never on a seam.
  for _ = 1, 26 do
    local x, y = r:int(4, W - 5), r:int(4, H - 5)
    local dx, dy = r:pick({ -1, 1 }), r:pick({ -1, 0, 1 })
    for s = 0, r:int(3, 8) - 1 do
      local px, py = x + s * dx, y + math.floor(s * dy / 2)
      local at = c:get(px, py)
      if at and at ~= seam then
        c:pixel(px, py, shade(at, -12))
      end
    end
  end

  -- Scorch marks from old fights: dark, ragged, softest at their edge.
  for _ = 1, 7 do
    local x0, y0, r0 = r:int(20, W - 21), r:int(20, H - 21), r:float(7, 15)
    for y = math.floor(y0 - r0) - 1, math.floor(y0 + r0) + 1 do
      for x = math.floor(x0 - r0) - 1, math.floor(x0 + r0) + 1 do
        local at = c:get(x, y)
        if at then
          local dx, dy = x - x0, y - y0
          local d = math.sqrt(dx * dx + dy * dy) / r0 + r:float(-0.18, 0.18)
          if d < 1 then
            c:pixel(x, y, scale(at, 0.62 + 0.38 * d))
          end
        end
      end
    end
  end

  -- Grit: small stones, lit above and shadowed below.
  for _ = 1, 70 do
    local x, y = r:int(2, W - 3), r:int(2, H - 4)
    c:pixel(x, y, shade(c:get(x, y), 16))
    c:pixel(x, y + 1, shade(c:get(x, y + 1), -10))
  end

  -- The rings round the middle, 4 and 6.25 units out, painted over the
  -- stones (not the seams) and worn: 3 in 10 of their pixels are gone.
  for y = 0, H - 1 do
    for x = 0, W - 1 do
      local dx, dy = x - cx, y - cy
      local d = math.sqrt(dx * dx + dy * dy) / PPU
      local on = (d > 3.85 and d < 4.15) or (d > 6.0 and d < 6.5)
      if on and c:get(x, y) ~= seam and r:chance(0.7) then
        c:pixel(x, y, mix(c:get(x, y), FLOOR_PAINT, 0.35))
      end
    end
  end

  -- A drain grate in the middle, 41 pixels across: an iron ring lit on
  -- top, with a line of ink each side of it, and inside it bars over the
  -- dark (two rows of iron, lit then shaded, then two of dark).
  for y = 0, H - 1 do
    for x = 0, W - 1 do
      local dx, dy = x - cx, y - cy
      local d = math.sqrt(dx * dx + dy * dy)
      if d <= 20.5 then
        local bar = math.floor(y - cy) % 4
        local col
        if d > 19.5 then
          col = INK
        elseif d > 16.5 then
          col = shade(IRON, if y < cy then 14 else -8)
        elseif d > 15.5 then
          col = INK
        elseif bar < 2 then
          col = shade(IRON, if bar == 0 then 6 else -6)
        else
          col = shade(INK, 3)
        end
        c:pixel(x, y, col)
      end
    end
  end

  -- The top wall's shadow over the first 10 rows, and the dark toward
  -- every wall, starting 55% of the way out from the middle.
  for y = 0, H - 1 do
    for x = 0, W - 1 do
      local ex = math.abs(x - cx) / (W / 2)
      local ey = math.abs(y - cy) / (H / 2)
      local e = math.max(0, math.max(ex, ey) - 0.55) / 0.45
      local k = 1 - VIGNETTE * e * e
      if y < 10 then
        k *= 0.62 + 0.038 * y
      end
      if k < 1 then -- the middle (k = 1) stays as it is
        c:pixel(x, y, scale(c:get(x, y), k))
      end
    end
  end
  return c
end

-- The walls ------------------------------------------------------------------
--
-- Their tops all round, and the top wall's face of bricks below its cap (we
-- look down on the room a little from the south, so the top wall shows its
-- face). The middle is clear: the floor is drawn there.

local function draw_walls()
  local r = art.rng(WALL_SEED)
  local W = ARENA_W * PPU + 2 * SIDE
  local H = ARENA_H * PPU + CAP + FACE + SOUTH
  local c = art.canvas(W, H)
  local top = CAP + FACE -- the floor's first row

  -- A block of the caps over x0..x1 - 1, y0..y1 - 1, in its own shade: lit
  -- along its top and left, dark along its bottom and right.
  local function cap_block(x0, y0, x1, y1)
    local t = r:pick({ -5, -2, 0, 2, 4 })
    for y = y0, y1 - 1 do
      for x = x0, x1 - 1 do
        local col
        if x == x0 or y == y0 then
          col = shade(WALL_LIGHT, t)
        elseif x == x1 - 1 or y == y1 - 1 then
          col = shade(WALL_DARK, t)
        else
          col = shade(WALL_TOP, t + r:pick({ -2, 0, 0, 1, 2 }))
        end
        c:pixel(x, y, col)
      end
    end
  end

  -- The caps: blocks of stone two units long along each wall, the bottom
  -- wall's set over by half a block.
  local block = 2 * PPU
  for x0 = 0, W - 1, block do
    cap_block(x0, 0, math.min(x0 + block, W), CAP)
  end
  local off = block // 2
  for x0 = -off, W - 1, block do
    cap_block(math.max(x0, 0), H - SOUTH, math.min(x0 + block, W), H)
  end
  for y0 = CAP, H - SOUTH - 1, block do
    cap_block(0, y0, SIDE, math.min(y0 + block, H - SOUTH))
    cap_block(W - SIDE, y0, W, math.min(y0 + block, H - SOUTH))
  end

  -- The top wall's face: two courses of bricks 16 pixels long, the second
  -- set over by half a brick and darker, toward the floor. Each brick has
  -- mortar along its top and left, and a lit row under that.
  local course = FACE // 2
  for k = 0, 1 do
    local y0 = CAP + k * course
    for x0 = SIDE - (k % 2) * 8, W - SIDE - 1, 16 do
      local t = r:pick({ -4, -2, 0, 1, 3 })
      for y = y0, y0 + course - 1 do
        for x = math.max(x0, SIDE), math.min(x0 + 16, W - SIDE) - 1 do
          local col = WALL_DARK
          if y ~= y0 and x ~= x0 then
            col = shade(WALL_FACE, t + r:pick({ -1, 0, 0, 1 }) - 4 * k)
            if y == y0 + 1 then
              col = shade(col, 8)
            end
          end
          c:pixel(x, y, col)
        end
      end
    end
  end
  -- The face's foot, where it meets the floor.
  c:line(SIDE, top - 1, W - SIDE - 1, top - 1, INK)
  c:line(SIDE, top - 2, W - SIDE - 1, top - 2, shade(WALL_DARK, -6))

  -- Five pennants hung along the face, in turn of colour: a rod, the cloth
  -- lit at its left and dark at its right, a pale band, and a swallowtail.
  for i = 0, 4 do
    local col = PENNANTS[i % #PENNANTS + 1]
    local x0 = (W * (i + 1)) // 6 - 3
    c:line(x0 - 1, CAP, x0 + 6, CAP, INK) -- the rod
    for d = 0, FACE - 3 do
      local y = CAP + 1 + d
      for x = x0, x0 + 5 do
        local tail = d >= FACE - 4 and x >= x0 + 2 and x <= x0 + 3 -- the tail's cut
        if not tail then
          local cloth = col
          if x == x0 then
            cloth = shade(col, 22)
          elseif x == x0 + 5 then
            cloth = shade(col, -30)
          end
          if d == 3 or d == 4 then
            cloth = shade(cloth, 45)
          end
          c:pixel(x, y, cloth)
        end
      end
    end
  end

  -- Pillars at the corners: a bigger block, 15 x 15, lit at its top and
  -- left, in a line of ink.
  for _, p in { { 0, 0 }, { W - 15, 0 }, { 0, H - 15 }, { W - 15, H - 15 } } do
    local px, py = p[1], p[2]
    c:rect(px, py, 15, 15, shade(WALL_TOP, 6))
    c:line(px + 13, py + 1, px + 13, py + 13, WALL_DARK)
    c:line(px + 1, py + 13, px + 13, py + 13, WALL_DARK)
    c:line(px + 1, py + 1, px + 1, py + 13, WALL_LIGHT)
    c:line(px + 1, py + 1, px + 13, py + 1, WALL_LIGHT)
    c:box(px, py, 15, 15, INK)
  end

  -- The inner rim of the caps (the drop to the floor), and an outline
  -- round it all.
  c:line(SIDE, H - SOUTH, W - SIDE - 1, H - SOUTH, INK)
  c:line(SIDE - 1, top, SIDE - 1, H - SOUTH - 1, INK)
  c:line(W - SIDE, top, W - SIDE, H - SOUTH - 1, INK)
  c:box(0, 0, W, H, INK)
  return c
end

return art.sprite{
  pixels_per_unit = PPU,
  frames = { draw_floor(), draw_walls() },
}
