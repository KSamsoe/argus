-- Everything in the level, one sprite of 36 frames: the hero (scenes/level/
-- player.toml, played by assets/anim/hero.toml), the coin (prefabs/coin.toml,
-- assets/anim/coin.toml), the flag (scenes/level/goal*.toml,
-- assets/anim/flag.toml), the spikes (prefabs/spikes.toml), the ground's
-- tiles, which systems/looks.luau lays over every solid block, and the
-- sparkle assets/fx/sparkle.toml bursts into.
--
-- The game picks these frames by number, so keep them in this order (put a
-- new one at the end). The frames, from 0:
--   0-8    the hero: idle (2), run (4), jump, fall, land
--   9-16   the coin turning
--   17-20  the flag's cloth waving
--   21     the flag's pole, 22 the spikes
--   23-25  grass tiles, 26-30 earth tiles, 31 a side, 32 a grass corner,
--   33     an underside, 34 a plank (systems/looks.luau lays 23 to 34)
--   35     a sparkle (assets/fx/sparkle.toml)
--
-- Sizes are in pixels, 15 to a world unit: the camera shows 12 units top to
-- bottom, so at 960 x 540 each pixel of the art is 3 on the screen. Change
-- a colour below and the game shows it at once, while it runs.

-- The scale and the helpers this script shares with backdrop.art.luau
-- (assets/shared.lib.luau).
local shared = require("shared")
local PPU = shared.PPU -- pixels a world unit
local round, trunc, mix = shared.round, shared.trunc, shared.mix

-- The palette ----------------------------------------------------------------

local OUTLINE = "#2A1E30" -- the dark line round the hero, the coin, the flag

-- The hero: a cap, hair, a tunic, a belt, trousers, boots.
local CAP = "#D9483B"
local CAP_SHADE = "#A3303A"
local CAP_SHINE = "#F27B5C"
local HAIR = "#5A3A2A"
local SKIN = "#F6C9A0"
local SKIN_SHADE = "#D99A78"
local TUNIC = "#F2A541" -- the hero's colour before there was art
local TUNIC_SHADE = "#C77A2A"
local BELT = "#6B4226"
local TROUSERS = "#3E5C9A"
local TROUSERS_SHADE = "#2C4274"
local BOOTS = "#4A2E26"

-- Coins.
local GOLD = "#FFD84A"
local GOLD_SHADE = "#D49A22"
local GOLD_SHINE = "#FFF7C2"
local GOLD_EDGE = "#8A5414"

-- The flag and its pole.
local CLOTH = "#4ADE80"
local CLOTH_SHADE = "#23A85A"
local CLOTH_SHINE = "#A8F5C4"
local POLE = "#F2F2F2"
local POLE_SHADE = "#A9AFBF"

-- Spikes: bright steel with red tips, on an iron plate.
local STEEL = "#D4DCE6"
local STEEL_SHINE = "#FFFFFF"
local STEEL_SHADE = "#7C8798"
local SPIKE_LINE = "#262A36"
local SPIKE_TIP = "#E04848"
local SPIKE_TIP_SHADE = "#9E2A3A"
local IRON = "#4A4F63"
local IRON_SHADE = "#33374A"

-- The ground: grass over earth with stones in it.
local GRASS_SHINE = "#A6E86A"
local GRASS = "#5CC33D"
local GRASS_SHADE = "#2F8A3A"
local FLOWERS = { {}, { "#FFFFFF" }, { "#F48FB1", "#FFE066" } } -- on each grass tile
local EARTH = "#94633F"
local EARTH_SPECK = "#AD7B52"
local EARTH_SHADE = "#734A30"
local EARTH_EDGE = "#3E2618" -- a block's sides and underside
local STONE_SHINE = "#D2C6B8"
local STONE = "#B3A496"
local STONE_SHADE = "#7D7068"
local WOOD = "#C08A52"
local WOOD_SHADE = "#8A5A32"

-- The sparkle is white, so the effect's colours (assets/fx/sparkle.toml)
-- tint it; its arms' ends are part clear.
local SPARK = "#FFFFFF"
local SPARK_FADE = "#FFFFFF8C"

-- The hero: 16 x 18, facing right --------------------------------------------
--
-- Its box (the Collider) is 12 x 18 pixels, the middle of the frame; arms and
-- legs swing a little past it. Each frame is the head, a body (the arms),
-- and legs, put together by hero(), drawn as text: one character a pixel,
-- looked up in the key ("." is clear). systems/looks.luau mirrors it to face
-- left.

local HERO = {
  o = OUTLINE, C = CAP, c = CAP_SHADE, w = CAP_SHINE, H = HAIR,
  S = SKIN, s = SKIN_SHADE, E = OUTLINE, T = TUNIC, t = TUNIC_SHADE,
  B = BELT, P = TROUSERS, p = TROUSERS_SHADE, K = BOOTS,
}

local HEAD = {
  "................",
  ".....ooooo......",
  "....oCCwwCo.....",
  "...oCCCCCwCo....",
  "...oCCCCCCCCooo.",
  "...occccccccccco",
  "...oHHSSSSESSo..",
  "...oHSSSSSESSo..",
  "....osSSSSSSo...",
  ".....oossso.....",
}

local BODIES = {
  -- Arms down at the sides.
  rest = {
    "....oTTTTTTo....",
    "...oTTTTTTTto...",
    "...oStTTTTtSo...",
    "....oBBBBBBo....",
  },
  -- Swinging: the front arm forward, the back one behind.
  swing = {
    "....oTTTTTTo....",
    "...oTTTTTTTtoo..",
    "..oStTTTTTtSSo..",
    "...ooBBBBBBoo...",
  },
  swing2 = {
    "....oTTTTTTo....",
    "..ooTTTTTTTto...",
    "..oSStTTTTttSo..",
    "...ooBBBBBBo....",
  },
  -- Both arms up, for the jump.
  up = {
    "..oSoTTTTTToSo..",
    "...otTTTTTTto...",
    "....oTTTTTTo....",
    "....oBBBBBBo....",
  },
  -- Arms out to the sides, for the fall.
  out = {
    "....oTTTTTTo....",
    "..ooTTTTTTTtoo..",
    ".oSStTTTTTTtSSo.",
    "..oooBBBBBBooo..",
  },
}

local LEGS = {
  stand = {
    "....oPPpoPPo....",
    "....oPPpoPPo....",
    "...oKKKooKKKo...",
    "...oooo..oooo...",
  },
  run1 = {
    "...oPPpooPPo....",
    "..oPPpo..oPPo...",
    ".oKKKo....oKKo..",
    ".oooo.....ooo...",
  },
  run2 = {
    "....oPPpPPo.....",
    "....oPPoPPo.....",
    "...oKKKoKKo.....",
    "...ooooooo......",
  },
  run3 = {
    "....oPPooPPpo...",
    "...oPPo..opPPo..",
    "..oKKo....oKKKo.",
    "..ooo.....oooo..",
  },
  tuck = {
    "...oPPpppPPo....",
    "...oKKKooKKKo...",
    "...oooo..oooo...",
    "................",
  },
  dangle = {
    "....oPPpoPPo....",
    "...oPPpo.oPPo...",
    "...oKKo..oKKo...",
    "...ooo....ooo...",
  },
}

-- The head, a body, and legs stacked; crouch drops the head and body by
-- that many pixels (a landing's squash), the legs staying put, so the
-- body's last rows go under them.
local function hero(body, legs, crouch)
  crouch = crouch or 0
  local top = {}
  for _, row in HEAD do
    table.insert(top, row)
  end
  for _, row in BODIES[body] do
    table.insert(top, row)
  end
  for _ = 1, crouch do
    table.remove(top)
  end
  local c = art.canvas(16, 18)
  c:rows(0, crouch, top, HERO)
  c:rows(0, #HEAD + 4, LEGS[legs], HERO)
  return c
end

-- A coin: 9 x 9, eight frames of a turn -------------------------------------

local COIN = 9
local COIN_FRAMES = 8

-- The coin turned k eighths of a half turn: an ellipse as wide as the turn
-- shows, gold, its rim darker, a shine at its top left.
local function coin(k)
  local c = art.canvas(COIN, COIN)
  local w = math.max(1, COIN * math.abs(math.cos(math.pi * k / COIN_FRAMES)))
  local rx, ry = w / 2, COIN / 2
  local cx, cy = COIN / 2, COIN / 2
  local function inside(x, y, s)
    s = s or 1
    local u, v = (x + 0.5 - cx) / (rx * s), (y + 0.5 - cy) / (ry * s)
    return u * u + v * v <= 1
  end
  for y = 0, COIN - 1 do
    for x = 0, COIN - 1 do
      if inside(x, y) then
        local edge = not (inside(x + 1, y) and inside(x - 1, y) and inside(x, y + 1) and inside(x, y - 1))
        if edge then
          c:pixel(x, y, GOLD_EDGE)
        elseif w > 4 and not inside(x, y, 0.62) then
          c:pixel(x, y, x + 0.5 > cx and GOLD_SHADE or GOLD)
        else
          c:pixel(x, y, GOLD)
        end
      end
    end
  end
  if w >= 3 then
    -- The shine, and the face's mark (a darker bar down the middle).
    local sx = trunc(cx - rx * 0.45)
    c:pixel(sx, 2, GOLD_SHINE)
    c:pixel(sx, 3, GOLD_SHINE)
    if w > 5 then
      c:rect(trunc(cx), 3, 1, 3, GOLD_SHADE)
    end
  else
    -- Edge on: a bar, dark below the middle.
    for y = 1, COIN - 2 do
      c:pixel(trunc(cx), y, y > 4 and GOLD_SHADE or GOLD)
    end
  end
  return c
end

-- The flag: cloth 17 x 11 in four frames of a wave, and its pole -------------

local CLOTH_W, CLOTH_H = 17, 11
local FLAG_FRAMES = 4

-- The cloth waving: each column lifted by a sine that moves along it, more
-- the farther from the pole; lit where the wave rises.
local function cloth(k)
  local c = art.canvas(CLOTH_W, CLOTH_H)
  for x = 0, CLOTH_W - 2 do
    local along = x / (CLOTH_W - 2)
    local phase = 2 * math.pi * (x / 10) - k * math.pi / 2
    local dy = round(1.4 * math.sin(phase) * along)
    local slope = math.cos(phase)
    local top, bottom = 2 + dy, 8 + dy
    for y = top, bottom do
      local col = CLOTH
      if slope > 0.45 then
        col = y == top and CLOTH_SHINE or CLOTH
      elseif slope < -0.45 then
        col = CLOTH_SHADE
      end
      c:pixel(x, y, col)
    end
  end
  c:outline(OUTLINE)
  return c
end

local POLE_W, POLE_H = 6, 45 -- 3 units tall, as the flag's Collider

local function pole()
  local c = art.canvas(POLE_W, POLE_H)
  -- A gold knob on top.
  c:rows(0, 0, { ".oooo.", "oGgGGo", "oGGGGo", ".oooo." }, { o = GOLD_EDGE, G = GOLD, g = GOLD_SHINE })
  -- The pole: white, shaded on its right, outlined.
  for y = 4, POLE_H - 4 do
    c:rows(0, y, { ".oPpo." }, { o = OUTLINE, P = POLE, p = POLE_SHADE })
  end
  -- A stone foot, standing on the ledge.
  c:rows(0, POLE_H - 3, { "oooooo", "oSSsso", "oooooo" }, { o = OUTLINE, S = STONE, s = STONE_SHADE })
  return c
end

-- A spark of light, 5 x 5: what a taken coin bursts into.
local function sparkle()
  local c = art.canvas(5, 5)
  c:rows(0, 0, { "..w..", "..W..", "wWWWw", "..W..", "..w.." }, { W = SPARK, w = SPARK_FADE })
  return c
end

-- The spikes: 28 x 10, four steel teeth with red tips on an iron plate -------

local TOOTH = 7 -- pixels across a tooth's foot
local TOOTH_H = 7 -- and from its foot to its point
local SPIKES_W, SPIKES_H = 4 * TOOTH, TOOTH_H + 3

local function spikes()
  local c = art.canvas(SPIKES_W, SPIKES_H)
  for i = 0, SPIKES_W // TOOTH - 1 do
    local mid = TOOTH * i + TOOTH / 2
    -- A pixel wide at the point, the whole foot at the plate.
    for r = 0, TOOTH_H - 1 do
      local hw = 0.5 + r * (TOOTH / 2 - 0.5) / (TOOTH_H - 1)
      for x = TOOTH * i, TOOTH * (i + 1) - 1 do
        local d = x + 0.5 - mid
        if math.abs(d) <= hw then
          local col
          if r < 3 then
            col = d <= 0 and SPIKE_TIP or SPIKE_TIP_SHADE
          elseif d < -hw + 1 then
            col = STEEL_SHINE
          elseif d <= 0 then
            col = STEEL
          else
            col = STEEL_SHADE
          end
          c:pixel(x, r + 1, col)
        end
      end
    end
  end
  c:outline(SPIKE_LINE)
  -- The plate they stand on, with a rivet under each tooth.
  c:rect(0, SPIKES_H - 2, SPIKES_W, 1, IRON)
  c:rect(0, SPIKES_H - 1, SPIKES_W, 1, IRON_SHADE)
  for x = TOOTH // 2, SPIKES_W - 1, TOOTH do
    c:pixel(x, SPIKES_H - 2, STEEL_SHADE)
  end
  return c
end

-- The ground's tiles: a unit (15 x 15) each ----------------------------------
--
-- systems/looks.luau lays them over every solid block: a grass tile along
-- the top, earth below, a dark edge down each side and along the underside
-- that no other block covers, and grass curling over the top corners. The
-- edges are the same all along, so they stretch to any length. The grass
-- and earth are laid at random from seeds (art.rng), so each tile is the
-- same every time; change a seed and that tile changes.

local T = PPU
local BLADES = 3 -- pixels of grass blades above a grass tile's top

-- Earth into c from row oy down: a few specks and darker clumps, and now and
-- then a pebble (its width and height) with its shadow under it.
local function earth(c, r, oy, pebble)
  for y = oy, c.h - 1 do
    for x = 0, T - 1 do
      local roll = r:float()
      c:pixel(x, y, roll < 0.07 and EARTH_SPECK or roll < 0.15 and EARTH_SHADE or EARTH)
    end
  end
  for _ = 1, r:int(3, 5) do
    local x, y = r:int(0, T - 2), r:int(oy, c.h - 2)
    c:rect(x, y, 2, 2, EARTH_SHADE)
    c:pixel(x, y, EARTH_SPECK)
  end
  if pebble then
    -- A pebble: lit on top, round at its top corners, a shadow under it.
    local sw, sh = pebble[1], pebble[2]
    local sx, sy = r:int(1, T - sw - 1), r:int(oy + 1, c.h - sh - 2)
    for y = sy, sy + sh - 1 do
      for x = sx, sx + sw - 1 do
        if not (y == sy and sw > 2 and (x == sx or x == sx + sw - 1)) then
          c:pixel(x, y, y == sy and STONE_SHINE or y == sy + sh - 1 and STONE_SHADE or STONE)
        end
      end
    end
    c:rect(sx, sy + sh, sw, 1, EARTH_SHADE)
  end
end

-- Grass along a block's top (tile n, from 0): blades above it (the top
-- BLADES rows of the frame), a lit edge, the turf, its ragged foot over the
-- earth.
local function grass(n)
  local r = art.rng(100 + n)
  local c = art.canvas(T, T + BLADES)
  earth(c, r, BLADES + 4)
  local foot = {}
  for x = 0, T - 1 do
    foot[x] = r:pick({ 5, 6, 6, 7 })
  end
  for x = 0, T - 1 do
    c:rect(x, BLADES, 1, foot[x], GRASS)
    c:pixel(x, BLADES, GRASS_SHINE)
    c:pixel(x, BLADES + foot[x] - 1, GRASS_SHADE)
    -- The turf's shadow on the earth under it.
    c:pixel(x, BLADES + foot[x], EARTH_SHADE)
    if r:chance(0.25) then
      c:pixel(x, BLADES + 2 + r:int(0, 2), GRASS_SHADE)
    end
  end
  -- Blades: tufts of one to three pixels standing up, lit at the tip.
  local x = r:int(0, 3)
  while x < T do
    local height = r:int(1, BLADES)
    for y = BLADES - height, BLADES - 1 do
      c:pixel(x, y, y > BLADES - height and GRASS or GRASS_SHINE)
    end
    x += r:int(2, 4)
  end
  -- A flower on a stem, on some of the tiles.
  for _, colour in FLOWERS[n + 1] do
    local fx = r:int(2, T - 3)
    c:pixel(fx, 1, GRASS_SHADE)
    c:pixel(fx, 2, GRASS_SHADE)
    c:pixel(fx, 0, colour)
  end
  return c
end

-- The earth tiles' pebbles {width, height}, one kind a tile; false: plain.
local PEBBLES = { false, false, { 5, 3 }, false, { 3, 2 } }

local function earth_tile(n)
  local r = art.rng(200 + n)
  local c = art.canvas(T, T)
  earth(c, r, 0, PEBBLES[n + 1])
  return c
end

local EDGE_W = 3
local EARTH_MID = mix(EARTH_EDGE, EARTH_SHADE, 0.5) -- half way between

-- A block's left side, the same all the way down so it stretches to any
-- height (mirrored for the right side).
local function side_edge()
  local c = art.canvas(EDGE_W, T)
  c:rect(0, 0, 1, T, EARTH_EDGE)
  c:rect(1, 0, 1, T, EARTH_MID)
  c:rect(2, 0, 1, T, EARTH_SHADE)
  return c
end

-- A block's underside, the same all the way along (it stretches).
local function underside()
  local c = art.canvas(T, EDGE_W)
  c:rect(0, 0, T, 1, EARTH_SHADE)
  c:rect(0, 1, T, 1, EARTH_MID)
  c:rect(0, 2, T, 1, EARTH_EDGE)
  return c
end

-- Grass curling over a block's top left corner and a little way down its
-- side (mirrored for the right corner): 5 x 12.
local function grass_corner()
  local c = art.canvas(5, BLADES + 9)
  c:rows(0, 0, {
    ".....",
    ".g...",
    ".Gg..",
    ".gggg",
    "gGGGG",
    "dGGGG",
    "dGGGG",
    "dGGGG",
    "dGGGd",
    "dGGd.",
    ".dd..",
    ".d...",
  }, { g = GRASS_SHINE, G = GRASS, d = GRASS_SHADE })
  return c
end

-- A one-way platform's plank, 15 x 6: what the hero jumps up through.
local function plank()
  local c = art.canvas(T, 6)
  c:rows(0, 0, {
    "ooooooooooooooo",
    "WWWWWWWWWWWWWWW",
    "wWWoWWWwWWWoWWW",
    "wWWWWWWwWWWWWWW",
    "wwwwwwwwwwwwwww",
    "ooooooooooooooo",
  }, { o = OUTLINE, W = WOOD, w = WOOD_SHADE })
  return c
end

-- The sprite -------------------------------------------------------------------

local frames = {
  -- 0-8 the hero.
  hero("rest", "stand"), -- idle
  hero("rest", "stand", 1), -- idle, breathing out
  hero("swing", "run1"), -- run
  hero("rest", "run2", 1),
  hero("swing2", "run3"),
  hero("rest", "run2", 1),
  hero("up", "tuck"), -- jump
  hero("out", "dangle"), -- fall
  hero("rest", "stand", 2), -- land
}
for k = 0, COIN_FRAMES - 1 do -- 9-16
  table.insert(frames, coin(k))
end
for k = 0, FLAG_FRAMES - 1 do -- 17-20
  table.insert(frames, cloth(k))
end
table.insert(frames, pole()) -- 21
-- 22: the spikes' pivot is the middle of their foot, so prefabs/spikes.toml
-- stands them on the ground where its Transform is.
table.insert(frames, { spikes(), pivot = { 0.5, 0 } })
for n = 0, #FLOWERS - 1 do -- 23-25
  table.insert(frames, grass(n))
end
for n = 0, #PEBBLES - 1 do -- 26-30
  table.insert(frames, earth_tile(n))
end
table.insert(frames, side_edge()) -- 31
table.insert(frames, grass_corner()) -- 32
table.insert(frames, underside()) -- 33
table.insert(frames, plank()) -- 34
table.insert(frames, sparkle()) -- 35

return art.sprite{
  pixels_per_unit = PPU,
  frames = frames,
  -- The clips the animators play (assets/anim/*.toml), by frame number.
  clips = {
    idle = { 0, 1, fps = 2 },
    run = { 2, 3, 4, 5, fps = 12 },
    jump = { 6 },
    fall = { 7 },
    land = { 8, 0, fps = 12, loop = false },
    coin = { 9, 10, 11, 12, 13, 14, 15, 16, fps = 12 },
    flag = { 17, 18, 19, 20, fps = 8 },
  },
}
