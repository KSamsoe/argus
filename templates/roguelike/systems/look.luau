-- How the dungeon looks: the art pass. Presentation only. The other systems
-- decide everything; this one dresses what they decided, in the late stage
-- after the turn's rules have run. It never changes a cell the game reads
-- (the tilemap's floor and walls layers) and draws on no random stream.
-- The pictures are tools/art.py's, in assets/.
--
-- - The map. When a run begins (sim.dungeon's run.new event) it paints the
--   two look layers of @dungeon's tilemap from its walls: "stone" gets
--   every cell's tile (a flagstone, a wall's brick face over the floor,
--   now and then with a torch, the rock beside a room with a lit rim, or
--   the dark far from any room) and "shade" the shadow a wall casts on the
--   floor beside it (or a torch's light, under one). Where the hero kills
--   a monster, the stone shows what the fight left.
-- - The creatures. Their frames from sprites/creatures: two idle frames
--   that swap each turn (out of step with each other), a flash the turn a
--   blow lands on them, a jump the turn a monster bites, monsters facing
--   the hero, the hero facing the way it last stepped, and fallen when
--   dead. A kind with no row in ROWS
--   keeps the coloured circle sim.dungeon gave it.
--
-- What to change first: ROWS when you draw a new monster, SLABS for more or
-- fewer cracked and mossy stones, TORCHES for more or fewer torches.

-- Tiles of tiles/dungeon (tile n is frame n - 1; the frames are listed in
-- assets/tiles/dungeon.toml).
local TOP, FACE = 9, 17 -- TOP + mask (0-7), FACE + sides (0-3)
local FACE_MOSS, TORCH_LIGHT, DEEP, FACE_TORCH = 21, 22, 23, 24
local SHADE_N, SHADE_W, SHADE_NW, SHADE_C = 25, 26, 27, 28
local REMAINS = { rat = 29, goblin = 30, bat = 31 }
local BLOOD = 32
-- The floor's stones, picked by a cell's number: mostly one slab, then two
-- to four stones, now and then cracked (6), mossy (7), or with bones (8).
local SLABS = { 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 2, 2, 3, 3, 3, 4, 4, 5, 5, 6, 7, 8 }
-- A torch on a plain stretch of wall every TORCHES columns.
local TORCHES = 5

-- Frames of sprites/creatures: each creature's row starts at its first
-- frame; then idle, idle a pixel lower, struck, and the monsters' bite
-- (the hero's fourth is fallen). The bat's row is drawn, ready for a bat.
local ATLAS = "sprites/creatures"
local ROWS = { rat = 4, goblin = 8, bat = 12 }
local STRUCK, BITE, FALLEN = 2, 3, 3
local ONE = vector.create(1, 1, 0)

-- A cell's number, the same every run: a hash of its column and row.
local function hash(x, y)
  local h = (x * 7919 + y * 104729 + 1013) % 1048573
  h = (h * 31337 + 7) % 1048573
  return (h * 2654435) % 1048573
end

-- A field written only when it changes, so a still turn changes nothing.
local function set(c, field, v)
  if c[field] ~= v then c[field] = v end
end

-- An entity's number (e:27 is 27), to put idle frames out of step.
local function num(id)
  return tonumber(string.match(id, "^e:(%d+)")) or 0
end

-- The place one step from pos the way the move action points, read as
-- sim.hero_turn reads it.
local function ahead(pos, m)
  local dx, dy = 0, 0
  if m.x > 0.5 then dx = 1
  elseif m.x < -0.5 then dx = -1
  elseif m.y > 0.5 then dy = 1
  elseif m.y < -0.5 then dy = -1 end
  return vector.create(pos.x + dx, pos.y + dy, 0)
end

-- map:paint's legend: one character a tile, from 0 ("0") to 32 ("w").
-- paint() writes each look layer in one map:paint call, as sim.dungeon
-- paints the map, rather than a map:set for every cell.
local CHARS = "0123456789abcdefghijklmnopqrstuvw"
local LEGEND, CH = {}, {}
for i = 1, #CHARS do
  LEGEND[string.sub(CHARS, i, i)] = i - 1
  CH[i - 1] = string.sub(CHARS, i, i)
end

-- The look layers, from the walls layer. Rows go down: y + 1 is the cell
-- below on screen.
local function paint(map)
  local w, h = map:size()
  local solid = {}
  for y = 0, h - 1 do
    for x = 0, w - 1 do
      solid[y * w + x] = map:get("walls", x, y) ~= 0
    end
  end
  -- Off the map is rock too.
  local function rock(x, y)
    return x < 0 or y < 0 or x >= w or y >= h or solid[y * w + x]
  end
  local function near_floor(x, y)
    for dy = -1, 1 do
      for dx = -1, 1 do
        if not rock(x + dx, y + dy) then return true end
      end
    end
    return false
  end
  -- A torch: on a wall's face with wall either side, every TORCHES columns.
  local function torch(x, y)
    return x % TORCHES == 2 and rock(x, y) and not rock(x, y + 1) and rock(x - 1, y) and rock(x + 1, y)
  end

  local stones, shades = {}, {}
  for y = 0, h - 1 do
    local srow, drow = {}, {}
    for x = 0, w - 1 do
      local r = hash(x, y)
      local stone, shade = 0, 0
      if rock(x, y) then
        local open_l, open_r = not rock(x - 1, y), not rock(x + 1, y)
        if not rock(x, y + 1) then
          -- Floor below: the wall shows its face, a lit edge where its
          -- left or right is open too.
          local sides = (open_l and 1 or 0) + (open_r and 2 or 0)
          stone = FACE + sides
          if torch(x, y) then
            stone = FACE_TORCH
          elseif sides == 0 and r % 6 == 0 then
            stone = FACE_MOSS
          end
        else
          -- Seen from above: a lit rim on each side that meets the floor,
          -- and the dark where no floor is near.
          local mask = (not rock(x, y - 1) and 1 or 0) + (open_r and 2 or 0) + (open_l and 4 or 0)
          stone = TOP + mask
          if mask == 0 and not near_floor(x, y) then stone = DEEP end
        end
      else
        stone = SLABS[r % #SLABS + 1]
        local above, left = rock(x, y - 1), rock(x - 1, y)
        if torch(x, y - 1) then
          shade = TORCH_LIGHT
        elseif above and left then
          shade = SHADE_NW
        elseif above then
          shade = SHADE_N
        elseif left then
          shade = SHADE_W
        elseif rock(x - 1, y - 1) then
          shade = SHADE_C
        end
      end
      srow[x + 1], drow[x + 1] = CH[stone], CH[shade]
    end
    stones[y + 1], shades[y + 1] = table.concat(srow), table.concat(drow)
  end
  map:paint("stone", 0, 0, stones, LEGEND)
  map:paint("shade", 0, 0, shades, LEGEND)
end

return system "late.look" {
  stage = "late",
  -- The first run's dungeon is dressed before the first key, and a
  -- restart (which freezes its turn) is dressed on the turn it happens.
  on_load = true,
  unfrozen = true,
  reads = { "Monster", "Player", "Transform", "Turn", "Actions", "UiNode", "Tilemap", "Sprite" },
  writes = { "Sprite", "Tilemap" },
  doc = "Presentation only: paints @dungeon's stone and shade layers from its walls when a run begins (and what a fight left where the hero kills a monster), and gives the hero and the monsters their frames of sprites/creatures: idle frames by turn, a flash when struck, a jump when biting, facing, the hero fallen when dead. Never writes the floor or walls layers.",
  run = function(q)
    local de = q:entity("@dungeon")
    local map = q:tiles(de)
    if #q:events("run.new") > 0 then
      paint(map)
    end

    local pe = q:entity("@player")
    local hero = pe:get("Transform").pos
    local turn = q:entity("@turn"):get("Turn").n
    local paused = q:entity("@pause"):get("UiNode").visible

    -- What this turn's blows landed on, and who bit.
    local struck, bit, hurt = {}, {}, false
    for _, ev in ipairs(q:events("damage")) do
      if tostring(ev.target) == "@player" then
        hurt = true
        bit[tostring(ev.by)] = true
      else
        struck[tostring(ev.target)] = true
      end
    end

    -- A kill leaves its remains where the monster stood: the cell the hero
    -- struck at (an attack does not move the hero).
    local m = pe:get("Actions").move
    for _, ev in ipairs(q:events("death")) do
      if ev.cause == "hero" then
        local col, row = map:cell(ahead(hero, m))
        map:set("stone", col, row, REMAINS[ev.kind] or BLOOD)
      end
    end
    if #q:events("player.died") > 0 then
      local col, row = map:cell(hero)
      map:set("stone", col, row, BLOOD)
    end

    -- The monsters.
    for e, mo, sp, tr in q:each("Monster", "Sprite", "Transform") do
      local row = ROWS[mo.kind]
      if row ~= nil then
        local id = e:id()
        set(sp, "atlas", ATLAS)
        set(sp, "size", ONE)
        set(sp, "color", "")
        local frame = (turn + num(id)) % 2
        if struck[id] then
          frame = STRUCK
        elseif bit[id] then
          frame = BITE
        end
        set(sp, "frame", row + frame)
        if hero.x ~= tr.pos.x then set(sp, "flip_x", hero.x < tr.pos.x) end
      end
    end

    -- The hero.
    local hs = pe:get("Sprite")
    if pe:get("Player").dead then
      set(hs, "frame", FALLEN)
    else
      set(hs, "frame", hurt and STRUCK or turn % 2)
      if not paused then
        if m.x > 0.5 then
          set(hs, "flip_x", false)
        elseif m.x < -0.5 then
          set(hs, "flip_x", true)
        end
      end
    end
  end,
}
