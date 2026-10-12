-- The actors' sheet: the hero, the enemy, the bullet, the braziers, and
-- the specks the effects are made of, 22 frames at 12 pixels a world unit.
-- systems/looks.luau, scenes/main/braziers.toml, and the effects in
-- assets/fx/ pick the frames by number, so keep the order of the list at
-- the bottom (a new frame goes on its end). Change a colour or a row
-- below and the game shows it at once, while it runs.

-- The hero's yellow and the colour arithmetic below are in
-- assets/shared.lib.luau, which the UI frame (ui/panel.art.luau) takes too.
local shared = require("shared")
local level, channels = shared.level, shared.channels

-- The colours ----------------------------------------------------------------

-- The hero, one character a pixel in its pictures below ("." is clear).
local HERO = {
  o = "#1B140C", -- outline (added round the picture)
  k = "#1B140C", -- a dark line inside it: the visor's rim, the hand
  Y = shared.HERO_YELLOW, -- helmet and shoulders (the UI frame's corners too)
  y = "#C4872A", -- their shade
  L = "#FFE9A3", -- their shine
  V = "#8BEAFF", -- visor
  v = "#2C86A8", -- visor, dark
  S = "#46505E", -- suit
  s = "#2C333D", -- suit, shade
  G = "#C3CCD4", -- gun
  g = "#67727E", -- gun, dark
  B = "#2E333C", -- boots
  b = "#1F2329", -- the far boot, from the side
}
local HIT_WHITE, HIT_RED = "#FFFFFF", "#FF4A4A" -- the hero hit: white, its outline red

-- The enemy is grey on purpose: the colour gameplay gives its Sprite
-- (orange from sim.waves, paler once hit from sim.hits) tints it
-- (multiplies it).
local ENEMY = {
  o = "#222222", -- outline (added round the picture)
  D = "#787878", -- shade
  B = "#B4B4B4", -- body
  H = "#E6E6E6", -- light
  e = "#101010", -- brows, pupils, mouth, and the cracks once hit
  W = "#FFFFFF", -- eyes, fangs
}

-- The brazier on each corner pillar (braziers.toml): an iron bowl of
-- coals. Its flames are particles (fx/flame), its glow a Light.
local BRAZIER = {
  o = "#0E0F12", -- outline
  I = "#3E434C", -- iron
  i = "#272B31", -- iron, dark
  h = "#6E7682", -- iron, lit
  c = "#C2452A", -- coals
  C = "#FF9A3C", -- coals, hot
  Y = "#FFE7A0", -- coals, hottest
}

-- The bullet, the spark, and the puff are white, and the shadow black:
-- their Sprites' and effects' colours tint them.

-- Colours worked out ---------------------------------------------------------

-- The fallen hero's colours: each one 55% of the way to its own grey, then
-- darkened to 70% (each step rounded, a half to the even level).
local function dulled(col)
  local r, g, b = channels(col)
  local grey = (r * 3 + g * 6 + b) / 10
  local function dull(v)
    return level(level(v + (grey - v) * 0.55) * 0.7)
  end
  return art.rgb(dull(r), dull(g), dull(b))
end

-- The hero hit is its pictures in white, with red wherever it has its
-- outline's colour (the dark lines inside it too, the same colour);
-- fallen, in its dulled colours.
local HIT, FALLEN = {}, {}
for ch, col in HERO do
  HIT[ch] = if col == HERO.o then HIT_RED else HIT_WHITE
  FALLEN[ch] = dulled(col)
end

-- The hero, 12 x 12 ----------------------------------------------------------
--
-- A gunner in a round yellow helmet, seen from above and a little in front,
-- as three views: from the side (facing right; the game mirrors it to face
-- left), from the front, and from the back. Each view is a body (8 rows)
-- on legs (2 rows); a step swaps the legs. The outline is added round it.

local HERO_SIDE = {
  "..LLYYY...",
  ".LYYYYYY..",
  ".YYYYYkVV.",
  ".YYYYYkVv.",
  ".yYYYYYYy.",
  "..yyyyyy..",
  ".YSSSkGGGG",
  ".ySSSsgg..",
}
local HERO_FRONT = {
  "..LLYYYY..",
  ".LYYYYYYy.",
  ".YkVVVVky.",
  ".YkVVVvky.",
  ".yYYYYYYy.",
  "..yyyyyy..",
  ".YSSSSSSYg",
  ".ySSSSSSyG",
}
local HERO_BACK = {
  "..LLYYYY.G",
  ".LYYYYYYyG",
  ".YYYYYYYyG",
  ".YYYYYYYyG",
  ".yYYYYYYyg",
  "..yyyyyy.g",
  ".YsggggsYg",
  ".ysggggsy.",
}

-- The legs: standing, and the two steps. From the side the far leg is
-- darker; the back view walks on the front view's legs.
local SIDE_LEGS = {
  stand = { "..ss.SS...", "..bb.BB..." },
  step = { ".ss...SS..", ".bb....BB." },
  step2 = { ".SS...ss..", ".BB....bb." },
}
local FRONT_LEGS = {
  stand = { "..SS..SS..", "..BB..BB.." },
  step = { "..BB..SS..", "......BB.." },
  step2 = { "..SS..BB..", "..BB......" },
}

-- A hero frame: the body over its legs, a pixel in from the frame's edge,
-- then a line of the key's outline colour round all of it, so it reads
-- against any floor.
local function hero(body, legs, key)
  local c = art.canvas(12, 12)
  c:rows(1, 1, body, key)
  c:rows(1, 9, legs, key)
  c:outline(key.o)
  return c
end

-- Fallen: the side view in its dulled colours, turned a quarter clockwise
-- to lie on its back.
local function fallen()
  return hero(HERO_SIDE, SIDE_LEGS.stand, FALLEN):rotate(1)
end

-- The enemy, 12 x 12 ---------------------------------------------------------
--
-- A horned imp: angry brows, eyes that look the way it faces (right; the
-- game mirrors it to face the hero), fangs, and two feet that waddle. Two
-- frames of walking, and the same two cracked once it has been hit. The
-- outline is added round it.

local ENEMY_BODY = {
  "H........H",
  "BH......HB",
  ".BBBBBBBB.",
  "BeBBBBBBeB",
  "BBeBBBBeBB",
  "BWeBBBBWeB",
  "BWWBBBBWWB",
  "DBeWeeWeBD",
  ".DDeeeeDD.",
}
local ENEMY_FEET = { { ".DD....DD." }, { "..DD..DD.." } }
-- Where the cracks run once it is hit: x, y in the picture above.
local CRACKS = { { 5, 1 }, { 4, 2 }, { 5, 3 }, { 3, 2 }, { 6, 4 } }

local function enemy(feet, cracked)
  local c = art.canvas(12, 12)
  c:rows(1, 1, ENEMY_BODY, ENEMY)
  c:rows(1, 10, feet, ENEMY)
  if cracked then
    for _, p in CRACKS do
      c:pixel(1 + p[1], 1 + p[2], ENEMY.e)
    end
  end
  c:outline(ENEMY.o)
  return c
end

-- The small things -----------------------------------------------------------

-- The bullet, 7 x 7: a hot middle, then a halo fading out.
local function bolt()
  local c = art.canvas(7, 7)
  for y = 0, 6 do
    for x = 0, 6 do
      local dx, dy = x - 3, y - 3
      local d = math.sqrt(dx * dx + dy * dy)
      local a = 1
      if d >= 1.2 then
        local fade = math.max(0, 1 - (d - 1.2) / 2.6)
        a = fade * math.sqrt(fade) * 0.8 -- fade to the power 1.5
      end
      c:pixel(x, y, art.rgb(255, 255, 255, level(255 * a)))
    end
  end
  return c
end

-- A spark, 3 x 3: a plus, its alpha in each pixel.
local function spark()
  local alpha = { { 50, 200, 50 }, { 200, 255, 200 }, { 50, 200, 50 } }
  local c = art.canvas(3, 3)
  for y = 0, 2 do
    for x = 0, 2 do
      c:pixel(x, y, art.rgb(255, 255, 255, alpha[y + 1][x + 1]))
    end
  end
  return c
end

-- A puff of dust, 7 x 7: a soft lumpy ball (each pixel's alpha 75% to 100%
-- of a smooth ball's, at random).
local function puff()
  local r = art.rng(3)
  local c = art.canvas(7, 7)
  for y = 0, 6 do
    for x = 0, 6 do
      local dx, dy = x - 3, y - 3
      local d = math.sqrt(dx * dx + dy * dy) / 3.6
      local a = math.max(0, 1 - d * d) * (0.75 + 0.25 * r:float())
      c:pixel(x, y, art.rgb(255, 255, 255, level(255 * a)))
    end
  end
  return c
end

-- A soft dark oval, w x h, for under a hero or an enemy: alpha at its
-- darkest in the middle, fading to nothing at its edge.
local function shadow(w, h, alpha)
  local c = art.canvas(w, h)
  for y = 0, h - 1 do
    for x = 0, w - 1 do
      local u = (x - (w - 1) / 2) / (w / 2)
      local v = (y - (h - 1) / 2) / (h / 2)
      local d = u * u + v * v
      if d < 1 then
        c:pixel(x, y, art.rgb(0, 0, 0, level(alpha * math.min(1, 1.6 * (1 - d)))))
      end
    end
  end
  return c
end

local function brazier()
  local c = art.canvas(12, 12)
  c:rows(0, 0, {
    "............",
    "............",
    "............",
    "..oooooooo..",
    ".ohcCYYCcho.",
    ".oicCYCCcio.",
    "..oiiiiiio..",
    "..ohIIIIho..",
    "...oIIIIo...",
    "....oiio....",
    "...oIooIo...",
    "...oo..oo...",
  }, BRAZIER)
  return c
end

-- The sheet ------------------------------------------------------------------

-- The frames, in the order systems/looks.luau counts them.
return art.sprite{
  pixels_per_unit = 12,
  frames = {
    hero(HERO_SIDE, SIDE_LEGS.stand, HERO), -- 0 hero side, standing
    hero(HERO_SIDE, SIDE_LEGS.step, HERO), -- 1 hero side, step
    hero(HERO_SIDE, SIDE_LEGS.step2, HERO), -- 2 hero side, other step
    hero(HERO_FRONT, FRONT_LEGS.stand, HERO), -- 3 hero front, standing
    hero(HERO_FRONT, FRONT_LEGS.step, HERO), -- 4 hero front, step
    hero(HERO_FRONT, FRONT_LEGS.step2, HERO), -- 5 hero front, other step
    hero(HERO_BACK, FRONT_LEGS.stand, HERO), -- 6 hero back, standing
    hero(HERO_BACK, FRONT_LEGS.step, HERO), -- 7 hero back, step
    hero(HERO_BACK, FRONT_LEGS.step2, HERO), -- 8 hero back, other step
    hero(HERO_SIDE, SIDE_LEGS.stand, HIT), -- 9 hero side, hit
    hero(HERO_FRONT, FRONT_LEGS.stand, HIT), -- 10 hero front, hit
    hero(HERO_BACK, FRONT_LEGS.stand, HIT), -- 11 hero back, hit
    fallen(), -- 12 hero fallen
    enemy(ENEMY_FEET[1], false), -- 13 enemy, step
    enemy(ENEMY_FEET[2], false), -- 14 enemy, other step
    enemy(ENEMY_FEET[1], true), -- 15 enemy hit, step
    enemy(ENEMY_FEET[2], true), -- 16 enemy hit, other step
    bolt(), -- 17 bullet
    spark(), -- 18 spark
    puff(), -- 19 puff
    shadow(10, 4, 120), -- 20 shadow
    brazier(), -- 21 brazier
  },
}
