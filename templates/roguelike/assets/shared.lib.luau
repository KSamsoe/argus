-- What the art scripts share: the colours more than one picture uses, and
-- the colour mixing they draw with. A script takes it with
-- local shared = require("shared"), so a colour here is one edit for every
-- picture that uses it, and saving this file draws them all again. It is
-- not a picture itself: a release leaves it out.

local shared = {}

-- The line round every creature (sprites/creatures.art.luau), also mixed
-- into the rock for its pits (tiles/dungeon.art.luau).
shared.INK = "#0E0C12"

-- Creatures' colours that what a fight leaves on the floor reuses: the
-- creatures are drawn in sprites/creatures.art.luau, their remains in
-- tiles/dungeon.art.luau.
shared.RAT_FUR_SHADE = "#634B3B" -- the rat's fur's shade, and the tuft it leaves
shared.RAT_PINK = "#D99A9A" -- its ears, nose, and tail, and the tail it leaves
shared.GOBLIN_BLADE = "#CED6E2" -- the goblin's knife, held and dropped
shared.GOBLIN_HILT = "#4A3020"
shared.BAT_WING_SHADE = "#3E2860" -- the bat's wing's shade, and the torn wing it leaves

-- The UI plates' dark stone and dark iron (ui/panel.art.luau and
-- ui/plate.art.luau); the torches' brackets on the walls are the same iron.
shared.PLATE_DARK = "#120F17"
shared.IRON_LO = "#332E3D"

-- Colour a pulled toward colour b by t (0 to 1). As art.mix, but a channel
-- that lands exactly halfway between two values goes to the even one.
function shared.mix(a, b, t)
  local out = {}
  for i = 2, 6, 2 do
    local from, to = tonumber(string.sub(a, i, i + 1), 16), tonumber(string.sub(b, i, i + 1), 16)
    local v = from + (to - from) * t
    local n = math.floor(v)
    if v - n > 0.5 or (v - n == 0.5 and n % 2 == 1) then
      n += 1
    end
    table.insert(out, n)
  end
  return art.rgb(out[1], out[2], out[3], 255)
end

return shared
