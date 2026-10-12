-- The frame the HUD, the pause screen, and the game over screen sit in
-- (scenes/main/hud.toml, paused.toml, game_over.toml): 16 x 16 pixels,
-- drawn as text (one character a pixel). Those files nine-slice it at 5
-- (slice = 5), so its corners and edges keep their size however big a
-- panel is, and its middle stretches. Dark glass, a steel edge, and the
-- hero's yellow at the corners, which are cut off so the frame is not a
-- hard box. Change a colour or a row and the game shows it at once.

local shared = require("shared") -- assets/shared.lib.luau

local key = {
  C = shared.HERO_YELLOW, -- the corners: the hero's yellow, one colour for both
  E = "#4F6672", -- the edge
  i = "#0B1013E4", -- the glass's dark rim, inside the edge
  s = "#1A2227E4", -- a line of light on the glass, near its top
  g = "#0E161BE4", -- the glass (E4: a little see-through)
}

local c = art.canvas(16, 16)
c:rows(0, 0, {
  ".CCCEEEEEEEECCC.",
  "CiiiiiiiiiiiiiiC",
  "CissssssssssssiC",
  "CiggggggggggggiC",
  "EiggggggggggggiE",
  "EiggggggggggggiE",
  "EiggggggggggggiE",
  "EiggggggggggggiE",
  "EiggggggggggggiE",
  "EiggggggggggggiE",
  "EiggggggggggggiE",
  "EiggggggggggggiE",
  "CiggggggggggggiC",
  "CiggggggggggggiC",
  "CiiiiiiiiiiiiiiC",
  ".CCCEEEEEEEECCC.",
}, key)

return art.sprite{ frames = { c } }
