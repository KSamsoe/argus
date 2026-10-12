-- The HUD's and the pause panel's frame, 16 x 16 pixels, drawn as text (one
-- character a pixel): a deep blue fill, a cream edge inside a dark line,
-- round corners. scenes/level/hud.toml nine-slices it at 4 pixels (UiPanel
-- slice), drawn 3 canvas pixels to each of its own, so the corners keep
-- their size and the middle stretches to fit the words.

local PANEL = "#1D2440EC" -- the fill, a little see-through
local PANEL_EDGE = "#F6E7C1" -- the coins' cream
local PANEL_LINE = "#0E1222"

local c = art.canvas(16, 16)
c:rows(0, 0, {
  ".oooooooooooooo.",
  "ooeeeeeeeeeeeeoo",
  "oeffffffffffffeo",
  "oeffffffffffffeo",
  "oeffffffffffffeo",
  "oeffffffffffffeo",
  "oeffffffffffffeo",
  "oeffffffffffffeo",
  "oeffffffffffffeo",
  "oeffffffffffffeo",
  "oeffffffffffffeo",
  "oeffffffffffffeo",
  "oeffffffffffffeo",
  "oeffffffffffffeo",
  "ooeeeeeeeeeeeeoo",
  ".oooooooooooooo.",
}, { o = PANEL_LINE, e = PANEL_EDGE, f = PANEL })

return art.sprite{ frames = { c } }
