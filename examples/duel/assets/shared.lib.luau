-- What the duel's art scripts share: the two fighters' colours, the gold of
-- the UI frame's corners, and the colour arithmetic they draw by. A script
-- takes it with local shared = require("shared"), so a fighter's colour is
-- one edit here, and saving this file draws every picture that takes it
-- again. It is not a picture itself: a release leaves it out.

local shared = {}

-- Each fighter's helmet and shoulders (sprites/actors.art.luau): its main
-- colour, its shade, and its shine. ui.toml's `one` and `two` are the
-- same blue and red, for the HUD's words.
shared.ONE = { main = "#4EA8F2", shade = "#2A6CB0", shine = "#B4E0FF" }
shared.TWO = { main = "#F2604E", shade = "#B03A2A", shine = "#FFC4B4" }

-- The corners of the frame the HUD and the menus sit in (ui/panel.art.luau).
shared.GOLD = "#F2C14E"

-- The nearest whole number, a half going to the even one (2.5 is 2, 3.5
-- is 4).
function shared.round(v)
  local whole = math.floor(v)
  local rest = v - whole
  if rest > 0.5 or (rest == 0.5 and whole % 2 == 1) then
    whole += 1
  end
  return whole
end

-- A level of red, green, or blue: v rounded, kept from 0 to 255.
function shared.level(v)
  return math.clamp(shared.round(v), 0, 255)
end

-- A colour's red, green, and blue, from 0 to 255. Each colour is asked of
-- art.rgba once and remembered: the floor (sprites/room.art.luau) asks for
-- a few hundred colours tens of thousands of times.
local split = {}

function shared.channels(col)
  local rgb = split[col]
  if not rgb then
    local r, g, b = art.rgba(col)
    rgb = { r, g, b }
    split[col] = rgb
  end
  return rgb[1], rgb[2], rgb[3]
end

return shared
