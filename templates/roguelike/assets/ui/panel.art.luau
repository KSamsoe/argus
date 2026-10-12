-- The plate the HUD and the pause screen sit on: 16 x 16 pixels of dark
-- stone in an iron frame, lit along its top and left, with a brass rivet
-- in each corner. Nine-sliced at 4 pixels and drawn at twice its size
-- (UiPanel slice and scale, in scenes/dungeon/hud.toml and
-- pause.panel.toml): the frame keeps its size and the middle stretches.

local N = 16 -- pixels a side

-- The dark stone and the dark iron are the message line's plate's too (and
-- the iron the torches' brackets'), so they are in assets/shared.lib.luau.
local shared = require("shared")

local FILL = art.alpha(shared.PLATE_DARK, 236 / 255) -- the dark stone, a little see-through
local IRON = "#6E6880"
local IRON_HI = "#A49DB6" -- the lit top and left
local IRON_LO = shared.IRON_LO
local BRASS = "#E2B866" -- the rivets

local c = art.canvas(N, N, FILL)

-- The ring inset pixels in from the edge, in colour (its corners cut by
-- cut pixels).
local function ring(inset, colour, cut)
  local a, b = inset + (cut or 0), N - 1 - inset - (cut or 0)
  c:line(a, inset, b, inset, colour)
  c:line(a, N - 1 - inset, b, N - 1 - inset, colour)
  c:line(inset, a, inset, b, colour)
  c:line(N - 1 - inset, a, N - 1 - inset, b, colour)
end

ring(0, IRON_LO, 1) -- a dark edge, its corner pixels left out
ring(1, IRON) -- the iron
c:line(2, 1, N - 3, 1, IRON_HI) -- lit along the top
c:line(1, 2, 1, N - 3, IRON_HI) -- and the left
ring(2, IRON_LO) -- a dark line inside it
for _, p in { { 1, 1 }, { N - 2, 1 }, { 1, N - 2 }, { N - 2, N - 2 } } do
  c:pixel(p[1], p[2], BRASS)
end

return art.sprite{ frames = { c } }
