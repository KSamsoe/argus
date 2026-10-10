-- How the floor is drawn (the art pass). Presentation only: sim.floor
-- paints the floor's walls and open cells (maps/floor.tilemap.toml's walls
-- and floor layers) and this never touches them; it paints three layers
-- of its own that only draw, from tiles/dungeon (tools/art.py lays its
-- frames out; tile n is frame n - 1):
--
-- - stone: every cell. Open floor is one of eight flagstone tiles; a wall
--   with open floor below it is a brick face, one beside open floor is
--   rock with a lit rim on each side that meets the room, and the rock
--   with no room beside it is dark.
-- - shade: the shadow a wall casts on the floor beside it, above and left.
-- - decor: what lies on the floor (cracks, pebbles, moss, puddles, bones,
--   a web in a corner), and what each fight leaves where a monster fell.
--
-- Which tile a cell gets is a hash of the cell and the floor's number, not
-- a random stream, so no system's numbers change and a floor looks the
-- same every time its seed makes it.

local FACE, TOP, DEEP = 17, 9, 21 -- face + sides (0-3), top + mask (0-7), deep (2)
local FACE_MOSS, FACE_CRACK = 23, 24
local SHADE_N, SHADE_W, SHADE_NW, SHADE_C = 25, 26, 27, 28
local CRACK, PEBBLES, MOSS, PUDDLE, BONES, WEB = 29, 30, 31, 32, 33, 34
local REMAINS = { rat = 35, bat = 36, goblin = 37 }
local BLOOD, SKULL = 38, 39
-- The floor's stones by a cell's number: half whole slabs, the rest two to
-- four stones, now and then cracked or worn (frames 0 to 7).
local SLABS = { 0, 0, 0, 0, 0, 0, 0, 0, 7, 7, 3, 1, 2, 4, 5, 6 }

-- A cell's number, the same every run: x, row, and the floor's count mixed.
local function hash(x, y, n)
  local h = (x * 7919 + y * 104729 + n * 15485863 + 1013) % 1048573
  h = (h * 31337 + 7) % 1048573
  h = (h * 2654435) % 1048573
  return h
end

-- map:paint's legend, a character a tile from 0 to 40, and each tile's
-- character.
local CHARS = "0123456789abcdefghijklmnopqrstuvwxyzABCDE"
local LEGEND, CH = {}, {}
for i = 1, #CHARS do
  local c = string.sub(CHARS, i, i)
  LEGEND[c] = i - 1
  CH[i - 1] = c
end

-- Every cell of the three layers, for floor number n. Rows go down: (x, y)
-- here is the map's cell, world (x, h - 1 - y).
local function paint(q, fe, map, n)
  local w, h = map:size()
  -- The walls, read once with a ring of rock round them (off the map is
  -- rock), as the navigation grid has them: @floor's NavGrid is its walls
  -- layer cell for cell, and asking it is some twenty times cheaper than
  -- a map:get a cell, which would cost a new floor's turn 15 ms.
  local rock = {}
  local W = w + 2
  for y = -1, h do
    for x = -1, w do
      local inside = x >= 0 and y >= 0 and x < w and y < h
      rock[(y + 1) * W + x + 2] = not inside or not q:walkable(vector.create(x, h - 1 - y, 0), fe)
    end
  end
  local function solid(x, y)
    return rock[(y + 1) * W + x + 2]
  end
  local stone, shade, decor = {}, {}, {}
  for y = 0, h - 1 do
    local s, d, e = {}, {}, {}
    for x = 0, w - 1 do
      local r = hash(x, y, n)
      local st, sh, de = 0, 0, 0
      if solid(x, y) then
        local open_l, open_r = not solid(x - 1, y), not solid(x + 1, y)
        if not solid(x, y + 1) then
          local sides = (open_l and 1 or 0) + (open_r and 2 or 0)
          st = FACE + sides
          if sides == 0 and r % 9 == 0 then
            st = (r // 9) % 2 == 0 and FACE_MOSS or FACE_CRACK
          end
        else
          local mask = (not solid(x, y - 1) and 1 or 0) + (open_r and 2 or 0) + (open_l and 4 or 0)
          st = TOP + mask
          if mask == 0 then
            local near = false
            for dy = -1, 1 do
              for dx = -1, 1 do
                if not solid(x + dx, y + dy) then near = true end
              end
            end
            if not near then st = DEEP + r % 2 end
          end
        end
      else
        -- Mostly whole slabs and pairs; now and then cracked or worn.
        st = 1 + SLABS[r % 16 + 1]
        local wn, ww, wnw = solid(x, y - 1), solid(x - 1, y), solid(x - 1, y - 1)
        if wn and ww then
          sh = SHADE_NW
        elseif wn then
          sh = SHADE_N
        elseif ww then
          sh = SHADE_W
        elseif wnw then
          sh = SHADE_C
        end
        local k = (r // 16) % 64
        local by_wall = wn or ww or solid(x + 1, y) or solid(x, y + 1)
        if wn and ww and k < 20 then
          de = WEB
        elseif k < 3 then
          de = CRACK
        elseif k < 6 then
          de = PEBBLES
        elseif k < 10 and by_wall then
          de = MOSS
        elseif k == 10 then
          de = PUDDLE
        elseif k == 11 then
          de = BONES
        elseif k == 12 and by_wall then
          de = SKULL
        end
      end
      s[x + 1], d[x + 1], e[x + 1] = CH[st], CH[sh], CH[de]
    end
    stone[y + 1] = table.concat(s)
    shade[y + 1] = table.concat(d)
    decor[y + 1] = table.concat(e)
  end
  map:paint("stone", 0, 0, stone, LEGEND)
  map:paint("shade", 0, 0, shade, LEGEND)
  map:paint("decor", 0, 0, decor, LEGEND)
end

return system "sim.floor_art" {
  stage = "sim",
  -- After the floor is made and the hero's blow has landed.
  after = { "sim.floor", "sim.player_turn", "sim.monsters" },
  on_load = true,
  -- A new run from the open menu is drawn on the turn it is asked for.
  unfrozen = true,
  reads = { "Tilemap", "NavGrid", "Floor", "Transform" },
  writes = { "Tilemap" },
  doc = "Presentation only: when a floor is made, paints @floor's stone, shade, and decor layers (drawn, never collided with) from its walls (as its NavGrid has them): flagstones, brick faces, rock with lit rims, wall shadows, and what lies about; where the hero kills a monster, its remains; where the hero dies, blood. Never writes the walls or floor layers, and draws on no random stream.",
  run = function(q)
    local fe = q:entity("@floor")
    local made = #q:events("floor.generated") > 0
    local deaths = q:events("death")
    local died = q:events("player.died")
    if not made and #deaths == 0 and #died == 0 then return end
    local map = q:tiles(fe)
    local _, h = map:size()
    if made then
      paint(q, fe, map, fe:get("Floor").n)
    end
    for _, ev in ipairs(deaths) do
      if ev.cause == "hero" then
        map:set("decor", ev.x, h - 1 - ev.y, REMAINS[ev.kind] or BLOOD)
      end
    end
    if #died > 0 then
      local p = q:entity("@player"):get("Transform").pos
      map:set("decor", p.x, h - 1 - p.y, BLOOD)
    end
  end,
}
