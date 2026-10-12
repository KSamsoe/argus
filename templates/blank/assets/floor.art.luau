-- The floor, 30 x 30 world units at 9 pixels a unit: flagstones in rows,
-- some cut in two, inside a kerb of stones; each stone with a lit and a
-- shaded edge and a little grain, and moss in some of the joints. Change a
-- colour or the seed and the game shows it at once.

local SEED = 1 -- another number, another floor
local PX = 9 -- pixels a world unit
local FLOOR = 30 * PX -- the floor's side
local SLAB = 3 * PX -- the grid of flagstones

local STONES = { "#2B3A42", "#2E3E47", "#283740", "#314450" } -- one picked per stone
local KERB = { "#3B4D57", "#384953" } -- the border of stones round the floor
local LIGHT, SHADE, JOINT = "#36484F", "#243138", "#182126" -- lit and shaded edges, the gaps
local MOSS = { "#3E5A3C", "#4F7044" }
local RIM = "#11171B" -- the floor's outermost pixels

local r = art.rng(SEED)
local c = art.canvas(FLOOR, FLOOR, JOINT)

-- A stone over x0..x1, y0..y1; its last row and column stay joint.
local function stone(x0, y0, x1, y1, colours)
  local base = r:pick(colours)
  for y = y0, y1 - 2 do
    for x = math.max(x0, 0), math.min(x1 - 2, FLOOR - 1) do
      local col = base
      if y == y0 or x == x0 then
        col = LIGHT
      elseif y == y1 - 2 or x == x1 - 2 then
        col = SHADE
      elseif r:float() < 0.06 then
        col = r:pick(colours)
      end
      c:pixel(x, y, col)
    end
  end
end

-- Rows of stones, each row set over by 0 to 2 units; a stone is whole, or
-- cut in two across or down.
for y0 = 0, FLOOR - 1, SLAB do
  for x0 = -r:pick({ 0, 1, 2 }) * PX, FLOOR - 1, SLAB do
    local x1, y1 = x0 + SLAB, y0 + SLAB
    local cut, roll = r:pick({ 1, 2 }) * PX, r:float()
    if roll < 0.5 then
      stone(x0, y0, x1, y1, STONES)
    elseif roll < 0.75 then
      stone(x0, y0, x0 + cut, y1, STONES)
      stone(x0 + cut, y0, x1, y1, STONES)
    else
      stone(x0, y0, x1, y0 + cut, STONES)
      stone(x0, y0 + cut, x1, y1, STONES)
    end
  end
end

-- The kerb, a unit wide all round.
for a = 0, FLOOR - 1, SLAB do
  stone(a, 0, a + SLAB, PX, KERB)
  stone(a, FLOOR - PX, a + SLAB, FLOOR, KERB)
  stone(0, a, PX, a + SLAB, KERB)
  stone(FLOOR - PX, a, FLOOR, a + SLAB, KERB)
end

-- Moss, in patches along the joints.
local joints = {}
for y = 1, FLOOR - 2 do
  for x = 1, FLOOR - 2 do
    if c:get(x, y) == JOINT then
      table.insert(joints, { x, y })
    end
  end
end
local around = { { 0, 0 }, { 1, 0 }, { 0, -1 }, { -1, 0 }, { 0, 1 } }
for _ = 1, 40 do
  local cx, cy, rad = r:int(0, FLOOR - 1), r:int(0, FLOOR - 1), r:int(5, 17)
  for _, j in joints do
    local dx, dy = j[1] - cx, j[2] - cy
    if dx * dx + dy * dy < rad * rad and r:chance(0.25) then
      for _, d in around do
        if r:chance(0.6) then
          c:pixel(j[1] + d[1], j[2] + d[2], r:pick(MOSS))
        end
      end
    end
  end
end

-- A dark rim all round.
c:box(0, 0, FLOOR, FLOOR, RIM)

return art.sprite{ frames = { c } }
