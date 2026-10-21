-- The actors' sheet: the two fighters, the swing's slash, the braziers, and
-- the specks the effects are made of, 31 frames at 12 pixels a world unit.
-- systems/looks.luau, the scene's files, and the effects in assets/fx/ pick
-- the frames by number, so keep the order of the list at the bottom (a new
-- frame goes on its end). Change a colour or a row below and the game
-- shows it at once, while it runs.

-- The fighters' colours and the colour arithmetic are in
-- assets/shared.lib.luau, which the UI frame (ui/panel.art.luau) takes too.
local shared = require("shared")
local level = shared.level

-- The colours ----------------------------------------------------------------

-- A fighter, one character a pixel in its pictures below ("." is clear).
-- Y, y, and L are its own colour; the rest both share.
local function fighter_key(colour)
  return {
    o = "#1B140C", -- outline (added round the picture)
    k = "#1B140C", -- a dark line inside it: the visor's rim, the hand
    Y = colour.main, -- helmet and shoulders
    y = colour.shade, -- their shade
    L = colour.shine, -- their shine
    V = "#8BEAFF", -- visor
    v = "#2C86A8", -- visor, dark
    S = "#46505E", -- suit
    s = "#2C333D", -- suit, shade
    G = "#E2E8EE", -- blade
    g = "#7A8590", -- blade, dark (the hilt)
    B = "#2E333C", -- boots
    b = "#1F2329", -- the far boot, from the side
  }
end
local ONE, TWO = fighter_key(shared.ONE), fighter_key(shared.TWO)

-- A fighter hit is its picture in white, with red wherever it has its
-- outline's colour (the dark lines inside it too, the same colour).
local HIT_WHITE, HIT_RED = "#FFFFFF", "#FF4A4A"
local HIT = {}
for ch, col in ONE do
  HIT[ch] = if col == ONE.o then HIT_RED else HIT_WHITE
end

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

-- The slash, the spark, and the puff are white, and the shadow black:
-- their Sprites' and effects' colours tint them.

-- A fighter, 12 x 12 ---------------------------------------------------------
--
-- A swordsman in a round helmet, seen from above and a little in front, as
-- three views: from the side (facing right; the game mirrors it to face
-- left), from the front, and from the back. Each view is a body (8 rows)
-- on legs (2 rows); a step swaps the legs. The outline is added round it.

local SIDE = {
  "..LLYYY...",
  ".LYYYYYY..",
  ".YYYYYkVV.",
  ".YYYYYkVv.",
  ".yYYYYYYy.",
  "..yyyyyy..",
  ".YSSSkgGGG",
  ".ySSSs....",
}
local FRONT = {
  "..LLYYYY..",
  ".LYYYYYYy.",
  ".YkVVVVky.",
  ".YkVVVvky.",
  ".yYYYYYYyG",
  "..yyyyyy.G",
  ".YSSSSSSYG",
  ".ySSSSSSyg",
}
local BACK = {
  "..LLYYYY.G",
  ".LYYYYYYyG",
  ".YYYYYYYyG",
  ".YYYYYYYyG",
  ".yYYYYYYyg",
  "..yyyyyy..",
  ".YssssssY.",
  ".ysssssy..",
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

-- A fighter frame: the body over its legs, a pixel in from the frame's
-- edge, then a line of the key's outline colour round all of it, so it
-- reads against any floor.
local function frame(body, legs, key)
  local c = art.canvas(12, 12)
  c:rows(1, 1, body, key)
  c:rows(1, 9, legs, key)
  c:outline(key.o)
  return c
end

-- A fighter's twelve frames, in the order systems/looks.luau counts them:
-- each view standing, stepping, and on its other step; then each view hit.
local function fighter(key)
  local views = { { SIDE, SIDE_LEGS }, { FRONT, FRONT_LEGS }, { BACK, FRONT_LEGS } }
  local out = {}
  for _, v in views do
    table.insert(out, frame(v[1], v[2].stand, key))
    table.insert(out, frame(v[1], v[2].step, key))
    table.insert(out, frame(v[1], v[2].step2, key))
  end
  for _, v in views do
    table.insert(out, frame(v[1], v[2].stand, HIT))
  end
  return out
end

-- The slash, 18 x 24 ---------------------------------------------------------
--
-- A crescent swept from top to bottom in front of the fighter (facing
-- right; the swing's Transform turns it the way it swings), in three
-- frames: the blade's path drawn brightest at its leading edge and fading
-- behind it. lead is how far down the sweep the edge is, 0 to 1.

local function slash(lead, strength)
  local c = art.canvas(18, 24)
  for y = 0, 23 do
    for x = 0, 17 do
      local px, py = x + 0.5, y + 0.5
      -- Inside the outer circle and outside the inner one, set back from
      -- it: a crescent, thickest straight ahead.
      local ox, oy = px - 6, 12 - py
      local ix, iy = px - 2, 12 - py
      if ox * ox + oy * oy <= 11 * 11 and ix * ix + iy * iy > 10 * 10 then
        local along = (90 - math.deg(math.atan2(oy, ox))) / 180 -- 0 at the top, 1 at the bottom
        if along <= lead then
          local a = math.max(0, 1 - (lead - along) / 0.6) * strength
          -- Brighter at the outer edge, where the blade's tip runs.
          local edge = math.sqrt(ox * ox + oy * oy) / 11
          a *= 0.55 + 0.45 * edge
          if a > 0 then
            c:pixel(x, y, art.rgb(255, 255, 255, level(255 * a)))
          end
        end
      end
    end
  end
  return c
end

-- The small things -----------------------------------------------------------

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

-- A soft dark oval, w x h, for under a fighter: alpha at its darkest in
-- the middle, fading to nothing at its edge.
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
local frames = {}
for _, c in fighter(ONE) do -- 0 to 11: player 1, blue
  table.insert(frames, c)
end
for _, c in fighter(TWO) do -- 12 to 23: player 2, red
  table.insert(frames, c)
end
table.insert(frames, slash(0.45, 1)) -- 24 slash, starting
table.insert(frames, slash(0.8, 1)) -- 25 slash, halfway
table.insert(frames, slash(1.0, 0.6)) -- 26 slash, done and fading
table.insert(frames, spark()) -- 27 spark
table.insert(frames, puff()) -- 28 puff
table.insert(frames, shadow(10, 4, 120)) -- 29 shadow
table.insert(frames, brazier()) -- 30 brazier

return art.sprite{
  pixels_per_unit = 12,
  frames = frames,
}
