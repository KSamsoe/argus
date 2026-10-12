-- The wooden panel the HUD (scenes/main/hud.toml) and the pause screen
-- (scenes/main/paused.toml) sit on: 16 x 16 pixels, dark wood inside a
-- lighter frame, a nail in each corner. Their UiPanel nine-slices it at 4
-- pixels and draws it at twice its pixels (scale), so the frame and the
-- nails keep their size however big the panel is. Change a colour and the
-- game shows it at once.

-- The frame is the colony's wood (assets/shared.lib.luau); the rest is the
-- panel's own.
local WOOD = require("shared").WOOD
local EDGE = "#1A120C" -- the line round it all
local FRAME = WOOD[3] -- the frame's lit top and left
local FRAME_SHADE = WOOD[2] -- its bottom and right
local FRAME_INNER = WOOD[1] -- the frame's inner ring
local INNER_EDGE = "#1A120CEC" -- the dark line inside the frame
local FILL = "#2A1C14EC" -- the dark wood in the middle, a touch see-through
local NAIL = "#E2D2A8"

local c = art.canvas(16, 16)
for y = 0, 15 do
  for x = 0, 15 do
    local ring = math.min(x, y, 15 - x, 15 - y) -- 0 at the edge, counting in
    local col = FILL
    if ring == 0 then
      local corner = (x == 0 or x == 15) and (y == 0 or y == 15)
      col = if corner then "none" else EDGE -- the corners left clear, rounding it
    elseif ring == 1 then
      col = if x == 1 or y == 1 then FRAME else FRAME_SHADE
    elseif ring == 2 then
      col = FRAME_INNER
    elseif ring == 3 then
      col = INNER_EDGE
    end
    c:pixel(x, y, col)
  end
end

-- A nail in each corner of the frame.
for _, p in { { 1, 1 }, { 14, 1 }, { 1, 14 }, { 14, 14 } } do
  c:pixel(p[1], p[2], NAIL)
end

return art.sprite{ frames = { c } }
