-- The creatures: 16 x 16 frames facing right, a row of four for each. The
-- hero's first (frames 0 and 1 idle, 2 hurt, 3 fallen), then the rat's
-- (from 4), the goblin's (8), and the bat's (12, not in the game yet),
-- each idle, idle, struck, and biting. systems/look.luau picks the frame
-- each turn by its number and mirrors it to face the way the creature
-- looks. A new kind of monster is a new picture here, its row added after
-- the bat's, and a line in look.luau's ROWS.
--
-- What to change first: the colours, then the pictures (HERO, RAT, GOBLIN,
-- BAT), drawn as text: one character a pixel. Change one and save, and the
-- game shows it while it runs. A cell is 16 x 16 pixels; the camera
-- (scenes/dungeon/camera.toml) shows 32 screen pixels a cell at 960 x 540,
-- so each pixel here is drawn 2 x 2. The colours the dungeon's tiles use
-- too (shared.X below) are in assets/shared.lib.luau.

-- The colours and the colour mixing the art scripts share.
local shared = require("shared")
local mix = shared.mix

local CELL = 16 -- pixels a cell, a frame's width and height

local INK = shared.INK -- the line round every creature

-- A blow landing: every colour pulled toward a flash (by K, from 0 to 1),
-- the line round it in another colour. The hero goes red, a monster white.
local HURT, HURT_LINE, HURT_K = "#FF4A3A", "#FF2A2A", 0.3
local FLASH, FLASH_LINE, FLASH_K = "#FFF6EC", "#E02828", 0.7

-- ----- the creatures -----------------------------------------------------
-- Each faces right (systems/look.luau mirrors them). A line round each is
-- added by rule (INK), so leave a clear pixel at every edge. The second
-- idle frame, the hit flash, the bite, and the hero's fall are made from
-- these by rule too. Each one's feet is the row (counted from 0 at the
-- top) where it breathes: the rows above it sink a pixel over it, and
-- the rows below it stay put.

local HERO_FEET = 12
local HERO_KEY = {
  r = "#C8343A", -- plume
  H = "#B4BCCB", -- helmet
  h = "#737C8E", -- helmet's shade
  S = "#EFC6A0", -- skin
  e = "#241712", -- eye
  T = "#3570A8", -- tunic
  t = "#24507E", -- tunic's shade
  B = "#7A5230", -- belt
  L = "#3A3448", -- legs
  F = "#5E3E28", -- boots
  W = "#E4E8F0", -- blade
  w = "#9AA2B4", -- blade's edge
  G = "#E0B050", -- hilt
  D = "#8E5E34", -- shield
  d = "#5E3C22", -- shield's rim
  M = "#C8CCD6", -- shield's boss
}
local HERO = {
  "................",
  "......rrr....W..",
  ".....HHHHH...Ww.",
  "....HHHHHHH..Ww.",
  "....hHHHHHH..Ww.",
  "....hhSSSeS..Ww.",
  ".....SSSSSS..Ww.",
  "..dd..TTTT..GGG.",
  ".dDDd.TTTTTSSS..",
  ".dMDdTTTTTt.....",
  ".dDDdTTTTTt.....",
  "..dd.BBBBBB.....",
  ".....TTt.TTt....",
  ".....LL...LL....",
  ".....LL...LL....",
  "....FFF..FFF....",
}

local RAT_FEET = 13
local RAT_KEY = {
  F = "#8A6C56", -- fur
  f = shared.RAT_FUR_SHADE, -- fur's shade
  W = "#B49A84", -- belly
  E = shared.RAT_PINK, -- ears, nose, tail
  R = "#FF3A30", -- eye
}
local RAT = {
  "................",
  "................",
  "................",
  "................",
  "................",
  "................",
  "................",
  "........E.E.....",
  ".......FFFFF....",
  ".....FFFFFRFF...",
  "...FFFFFFFFFFE..",
  ".E.fFFFFFFWW....",
  "..EfFFFFFWWW....",
  "...ffFFFWWW.....",
  "....F.F.F.F.....",
  "................",
}

local GOBLIN_FEET = 12
local GOBLIN_KEY = {
  G = "#6EA43E", -- skin
  g = "#4A7A2A", -- skin's shade
  R = "#FF4030", -- eyes
  k = "#F2EEDC", -- teeth
  V = "#7A4E2C", -- leather
  v = "#55361E", -- leather's shade
  D = shared.GOBLIN_BLADE, -- blade
  h = shared.GOBLIN_HILT, -- hilt
  L = "#3E3428", -- legs
}
local GOBLIN = {
  "................",
  "................",
  "................",
  "..g........g....",
  "..gGGGGGGGgg....",
  "...GGGRGGRG.....",
  "...GGGGGGGG.....",
  "....GkkkkG...D..",
  "...vVVVVVVv..D..",
  "..GvVVVVVVvG.D..",
  "..G.VVVVVV.Ghh..",
  "....VVvVVV......",
  "....VV..VV......",
  "....LL..LL......",
  "....LL..LL......",
  "...gg..gg.......",
}

-- Not in the game yet: ready for README idea 3, a bat that flutters.
local BAT_FEET = 10
local BAT_KEY = {
  W = "#5E3E86", -- wing
  w = shared.BAT_WING_SHADE, -- wing's shade
  B = "#7A5A9E", -- body
  E = "#9C7CC0", -- ears
  Y = "#FFE040", -- eyes
  k = "#F4F0F8", -- fangs
}
local BAT = {
  "................",
  "................",
  "................",
  "................",
  ".W...........W..",
  ".WW..E...E..WW..",
  ".WWW.BBBBB.WWW..",
  ".WwWWBYBYBWWwW..",
  "..wWWBBBBBWWw...",
  "..w.WBBBBBW.w...",
  "..w...kBk...w...",
  "................",
  "................",
  "................",
  "................",
  "................",
}

-- ----- the frames, by rule -----------------------------------------------

-- The picture drawn from its rows at (x, y) (a pixel past the edge is left
-- out), with no line round it.
local function picture(rows, key, x, y)
  local c = art.canvas(CELL, CELL)
  c:rows(x or 0, y or 0, rows, key)
  return c
end

-- A line round the picture's shape: every clear pixel beside a drawn one,
-- so a creature reads against any floor.
local function outlined(c, line)
  c:outline(line or INK)
  return c
end

-- The second idle frame, a breath: the rows above feet one pixel lower,
-- the feet where they were (look.luau swaps the two idle frames each turn).
local function bob(rows, feet)
  local out = { string.rep(".", CELL) }
  for i = 1, feet do
    table.insert(out, rows[i])
  end
  for i = feet + 2, #rows do
    table.insert(out, rows[i])
  end
  return out
end

-- The frame a blow lands on: every colour pulled toward toward by k, the
-- line round it in line.
local function struck(rows, key, toward, line, k)
  local hit = {}
  for ch, colour in key do
    hit[ch] = mix(colour, toward, k)
  end
  return outlined(picture(rows, hit), line)
end

-- The hero fallen: the picture turned a quarter round, lying on the floor
-- of its cell (its lowest pixel a row above the bottom, for the line).
local function fallen(c)
  local turned = c:rotate(1)
  local low = 0
  for y = 0, CELL - 1 do
    for x = 0, CELL - 1 do
      local colour = turned:get(x, y)
      if #colour == 7 or string.sub(colour, 8, 9) ~= "00" then
        low = y
      end
    end
  end
  local out = art.canvas(CELL, CELL)
  out:draw(turned, 0, CELL - 2 - low)
  return out
end

-- A creature's first three frames: idle, idle a pixel lower, and struck.
local function creature(rows, key, feet, toward, line, k)
  return {
    outlined(picture(rows, key)),
    outlined(picture(bob(rows, feet), key)),
    struck(rows, key, toward, line, k),
  }
end

-- A monster's four: idle, idle, struck (a white flash), and biting (the
-- picture a pixel forward and a pixel up, a jump at what it bites).
local function monster(rows, key, feet)
  local row = creature(rows, key, feet, FLASH, FLASH_LINE, FLASH_K)
  table.insert(row, outlined(picture(rows, key, 1, -1)))
  return row
end

-- ----- the frames, in order ----------------------------------------------

local frames = {}
local function add(row)
  for _, c in row do
    table.insert(frames, c)
  end
end

local hero = creature(HERO, HERO_KEY, HERO_FEET, HURT, HURT_LINE, HURT_K)
table.insert(hero, outlined(fallen(picture(HERO, HERO_KEY)))) -- its fourth: fallen
add(hero) -- 0-3
add(monster(RAT, RAT_KEY, RAT_FEET)) -- 4-7
add(monster(GOBLIN, GOBLIN_KEY, GOBLIN_FEET)) -- 8-11
add(monster(BAT, BAT_KEY, BAT_FEET)) -- 12-15

return art.sprite{ frames = frames }
