-- The player: five 18 x 18 frames, facing right, drawn as text (one
-- character a pixel). Frame 0 stands; 1 to 4 are a step on each foot.
-- Change a row or a colour below and the game shows it at once, while it
-- runs. The art is 9 pixels to a world unit; the player's Sprite is 2 x 2
-- units (scenes/main/player.toml).

local INK = "#1B1A22"
local key = {
  o = INK, E = INK,                -- outlines, the eye, the boots
  H = "#6B4630", h = "#4A2F22",    -- hair, and its dark side
  S = "#F5CDA4",                   -- skin
  Y = "#F2C14E", y = "#C98B32",    -- the shirt, and its dark side
  B = "#3A2E3A",                   -- the belt
  L = "#7A5236", f = "#553826",    -- the near leg, and the far one
}

-- The head and shirt, the same in every frame (moved down a pixel when the
-- body sinks mid-stride).
local top = {
  "......ooooo.......",
  ".....oHHHHHo......",
  "....oHHHHHHHo.....",
  "....oHHHHHHHHo....",
  "....ohHHHSSSSo....",
  "....ohHHSSSESo....",
  "....ohhHSSSESo....",
  ".....ohhSSSSo.....",
  ".....oYYooooYo....",
  "....oyYYYYYYYo....",
  "....oyYYYYYYYo....",
  "....oyYYYYYYYo....",
  ".....oBBBBBBo.....",
}

-- The near arm, swung against the near leg.
local arms = {
  mid = { { 9, 9, "y" }, { 9, 10, "y" }, { 9, 11, "S" } },
  fwd = { { 10, 9, "y" }, { 11, 10, "y" }, { 12, 11, "S" } },
  back = { { 8, 9, "y" }, { 7, 10, "y" }, { 6, 11, "S" } },
}

-- The legs: three rows under the shirt.
local legs = {
  stand = { ".......ff.LL......", ".......ff.LL......", ".......oo.oo......" },
  stride = { ".......ff.LL......", "......ff...LL.....", ".....oo.....oo...." },
  pass = { "........fLL.......", "........fLL.......", "........ooo......." },
  stride2 = { ".......LL.ff......", "......LL...ff.....", ".....oo.....oo...." },
}

local function frame(leg, arm, sink)
  local c = art.canvas(18, 18)
  -- A soft shadow under the feet, darker in the middle.
  for x = 3, 14 do
    c:pixel(x, 16, art.alpha("#000000", (x >= 4 and x <= 13) and 110 / 255 or 50 / 255))
  end
  c:rows(0, 13, legs[leg], key)
  c:rows(0, sink, top, key)
  for _, p in arms[arm] do
    c:pixel(p[1], p[2] + sink, key[p[3]])
  end
  return c
end

return art.sprite{
  frames = {
    frame("stand", "mid", 0),
    frame("stride", "back", 1),
    frame("pass", "mid", 0),
    frame("stride2", "fwd", 1),
    frame("pass", "mid", 0),
  },
  clips = {
    idle = { 0 },
    walk = { 1, 2, 3, 4, fps = 10 },
  },
}
