-- The colonists: a row of 16 poses for each one, in the order of PEOPLE
-- (Ada, Bram, Cleo, Dov), each pose a 16 x 16 frame with the colonist in
-- its middle, drawn facing right: 0 standing; 1-2 walking; 3 carrying,
-- standing; 4-5 carrying, walking; 6-7 picking; 8-9 chopping; 10-11
-- eating; 12-13 walking away (up the screen) and 14-15 carrying away, seen
-- from behind. systems/looks.luau picks row * 16 + pose and mirrors it to
-- face left. The art is 9 pixels to a world unit, and every frame stands
-- on its feet (the pivot).
--
-- To change first: the colonists' clothes and hair (PEOPLE). Change a row
-- or a colour and the game shows it at once, while it runs.

-- The colours the colonists share with the rest of the colony's art (the
-- outline, wood, steel, the bowl, the berry) are in assets/shared.lib.luau,
-- with the shadow under their feet; the eye and PEOPLE are theirs alone.
local shared = require("shared")
local INK = shared.INK -- every outline
local EYE = "#1E1410"
local BELT = shared.WOOD[1]
local BOOTS = shared.WOOD[0]
local AXE_HEAD, AXE_EDGE, AXE_HANDLE = shared.STEEL[1], shared.STEEL[0], shared.WOOD[3]
local BOWL = shared.BASKET[1] -- the wooden bowl they eat from
local BERRY = shared.BERRY[1]

-- The colonists, a row each: systems/looks.luau finds a colonist's row by
-- Colonist.name (another name gets a row from its length). Each colour
-- with its shade; style is the hair: short, long, or beard.
local PEOPLE = {
  { name = "Ada", hair = { "#A85432", "#74321E" }, skin = { "#F0C8A0", "#D09A74" },
    shirt = { "#5E8E3E", "#44682C" }, legs = "#6A4A30", style = "long" },
  { name = "Bram", hair = { "#3E2C22", "#261A12" }, skin = { "#D8A070", "#B07A50" },
    shirt = { "#B23E30", "#842C22" }, legs = "#3E4C6C", style = "beard" },
  { name = "Cleo", hair = { "#E8C462", "#B8923C" }, skin = { "#F4D2B4", "#D8AA88" },
    shirt = { "#4A7CBC", "#34598E" }, legs = "#6A5A8C", style = "long" },
  { name = "Dov", hair = { "#6E4C30", "#4A3220" }, skin = { "#A86C4A", "#845238" },
    shirt = { "#C88C32", "#9A6822" }, legs = "#5A6C3A", style = "short" },
}

-- A colonist is 9 x 13 pixels drawn as text, one character a pixel, inside
-- a one-pixel outline: a head of six rows, a body of four, legs of three.
-- Letters: H hair, h its shade, S skin, s its shade, e eye, T shirt, t its
-- shade (the arms), L belt, P legs, B boots, A and K an axe head, a its
-- handle, W a wooden bowl, R a berry. A dot is nothing.
local function key(p)
  return {
    H = p.hair[1], h = p.hair[2], S = p.skin[1], s = p.skin[2], e = EYE,
    T = p.shirt[1], t = p.shirt[2], L = BELT, P = p.legs, B = BOOTS,
    A = AXE_HEAD, K = AXE_EDGE, a = AXE_HANDLE, W = BOWL, R = BERRY,
  }
end

local HEADS = {
  short = {
    "..HHHHH..",
    ".HHHHHHH.",
    ".HhSSSSS.",
    ".HSSeSSe.",
    ".hSSSSSs.",
    "..sSSSs..",
  },
  long = {
    "..HHHHH..",
    ".HHHHHHH.",
    "HHhSSSSS.",
    "HHSSeSSe.",
    "HhSSSSSs.",
    "hh.sSSs..",
  },
  beard = {
    "..HHHHH..",
    ".HHHHHHH.",
    ".HhSSSSS.",
    ".HSSeSSe.",
    ".hSHHHHH.",
    "..hHHHh..",
  },
}

-- The back of each head, for walking away up the screen; long hair falls
-- down the back (BACK_HAIR).
local HEADS_AWAY = {
  short = {
    "..HHHHH..",
    ".HHHHHHH.",
    ".HHHHHHH.",
    ".HHHHHHH.",
    ".hHHHHHh.",
    "..sSSSs..",
  },
  long = {
    "..HHHHH..",
    ".HHHHHHH.",
    "HHHHHHHHH",
    "HHHHHHHHH",
    "hHHHHHHHh",
    "hhHHHHHhh",
  },
}
HEADS_AWAY.beard = HEADS_AWAY.short

local BODIES = {
  down = { ".tTTTTTt.", ".tTTTTTt.", ".tTTTTTt.", ".SLLLLLS." },
  up = { "..TTTTT..", "..TTTTT..", "..TTTTT..", "..LLLLL.." },
  reach = { ".tTTTTTtt", ".tTTTTT.S", ".tTTTTT..", ".SLLLLL.." },
  reach2 = { ".tTTTTTt.", ".tTTTTTt.", ".tTTTTTtS", ".SLLLLL.." },
  bowl = { ".tTTTTTt.", ".tTTTTTt.", ".SWWWWWS.", "..LLLLL.." },
  bite = { ".tTTTTTt.", ".tTTTTTt.", ".SWWWWW..", "..LLLLL.." },
}

local LEGS = {
  stand = { "..PPPPP..", "..PP.PP..", "..BB.BB.." },
  step_a = { "..PPPPP..", "..PP.BB..", "..BB....." },
  step_b = { "..PPPPP..", "..BB.PP..", ".....BB.." },
  wide = { "..PPPPP..", ".PP...PP.", ".BB...BB." },
}

-- Extra pixels over a pose, each { column, row, letter } (the outline's
-- column is 0, the head's top row 0). Arms raised to hold a load over the
-- head (the load is the goods' own sprite, drawn above): hands at the top,
-- sleeves down to the shoulders.
local ARMS_UP = {
  { 1, 0, "S" }, { 9, 0, "S" }, { 1, 1, "t" }, { 9, 1, "t" }, { 1, 2, "t" }, { 9, 2, "t" },
  { 1, 3, "t" }, { 9, 3, "t" }, { 1, 4, "t" }, { 9, 4, "t" }, { 2, 5, "t" }, { 8, 5, "t" },
  { 2, 6, "t" }, { 8, 6, "t" },
}
-- Chopping: the axe swung back behind, then into the tree in front.
local SWING = {
  { 0, 6, "A" }, { 0, 7, "A" }, { 0, 8, "A" }, { 1, 7, "K" }, { 2, 7, "a" }, { 3, 7, "a" },
  { 4, 7, "S" }, { 5, 7, "S" },
}
local STRIKE = {
  { 10, 6, "A" }, { 10, 7, "A" }, { 10, 8, "A" }, { 9, 7, "K" }, { 8, 7, "a" }, { 7, 7, "a" },
  { 5, 7, "S" }, { 6, 7, "S" },
}
-- Eating: the hand comes up to the mouth with a berry.
local BITE = { { 8, 5, "R" }, { 8, 6, "t" }, { 8, 7, "S" } }
-- Long hair down the back, seen from behind.
local BACK_HAIR = {
  { 3, 6, "h" }, { 4, 6, "H" }, { 5, 6, "H" }, { 6, 6, "H" }, { 7, 6, "h" },
  { 4, 7, "h" }, { 5, 7, "h" }, { 6, 7, "h" },
}

-- The poses, in the order of a row: a body, legs, extra pixels, and
-- whether it is seen from behind.
local POSES = {
  { body = "down", legs = "stand" }, -- 0 standing
  { body = "down", legs = "step_a" }, -- 1, 2 walking
  { body = "down", legs = "step_b" },
  { body = "up", legs = "stand", extra = ARMS_UP }, -- 3 carrying, standing
  { body = "up", legs = "step_a", extra = ARMS_UP }, -- 4, 5 carrying, walking
  { body = "up", legs = "step_b", extra = ARMS_UP },
  { body = "reach", legs = "stand" }, -- 6, 7 picking (a bush, or goods off the ground)
  { body = "reach2", legs = "stand" },
  { body = "up", legs = "wide", extra = SWING }, -- 8, 9 chopping: the swing and the strike
  { body = "up", legs = "wide", extra = STRIKE },
  { body = "bowl", legs = "stand" }, -- 10, 11 eating
  { body = "bite", legs = "stand", extra = BITE },
  { body = "down", legs = "step_a", away = true }, -- 12, 13 walking away
  { body = "down", legs = "step_b", away = true },
  { body = "up", legs = "step_a", extra = ARMS_UP, away = true }, -- 14, 15 carrying away
  { body = "up", legs = "step_b", extra = ARMS_UP, away = true },
}

-- One colonist in one pose: 11 x 16 pixels, the outline round the 9 x 13,
-- the feet at the bottom and a row under them for the shadow.
local function colonist(p, pose)
  local c = art.canvas(11, 16)
  local heads = if pose.away then HEADS_AWAY else HEADS
  local rows = {}
  for _, part in { heads[p.style], BODIES[pose.body], LEGS[pose.legs] } do
    for _, row in part do
      table.insert(rows, "." .. row .. ".")
    end
  end
  local k = key(p)
  c:rows(0, 1, rows, k)
  local extras = {}
  if pose.away and p.style == "long" then
    table.insert(extras, BACK_HAIR)
  end
  if pose.extra then
    table.insert(extras, pose.extra)
  end
  for _, list in extras do
    for _, px in list do
      c:pixel(px[1], px[2] + 1, k[px[3]])
    end
  end
  c:outline(INK)
  shared.shadow(c, 5.5, 14.8, 4.4, 1.3) -- under the feet
  return c
end

-- Every frame, a row a colonist: each pose in the middle of its 16 x 16.
local frames = {}
for _, p in PEOPLE do
  for _, pose in POSES do
    local cell = art.canvas(16, 16)
    cell:draw(colonist(p, pose), 3, 0)
    table.insert(frames, cell)
  end
end

return art.sprite{
  frames = frames,
  pivot = { 0.5, 0.125 }, -- at the feet, two pixels up from the bottom
  pixels_per_unit = 9,
}
