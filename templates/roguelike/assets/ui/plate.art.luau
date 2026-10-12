-- The message line's plate: 16 x 16 pixels, the HUD's dark (see
-- panel.art.luau) with a thin cold edge. Nine-sliced at 3 pixels and drawn
-- at twice its size (UiPanel slice and scale, in
-- scenes/dungeon/messages.toml): the edge keeps its size and the middle
-- stretches.

local N = 16 -- pixels a side

-- The HUD's dark stone and dark iron, in assets/shared.lib.luau.
local shared = require("shared")

local FILL = art.alpha(shared.PLATE_DARK, 205 / 255) -- the dark, more see-through than the HUD's
local EDGE = art.alpha(shared.IRON_LO, 240 / 255) -- the thin edge
local LIT = art.alpha("#2A2533", 230 / 255) -- a faint light inside its top

local c = art.canvas(N, N, FILL)

-- The edge: a line along each side, a pixel short of each end, so its
-- corner pixels are left out.
c:line(1, 0, N - 2, 0, EDGE)
c:line(1, N - 1, N - 2, N - 1, EDGE)
c:line(0, 1, 0, N - 2, EDGE)
c:line(N - 1, 1, N - 1, N - 2, EDGE)
c:line(2, 1, N - 3, 1, LIT)

return art.sprite{ frames = { c } }
