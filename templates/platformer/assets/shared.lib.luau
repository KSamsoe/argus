-- What the level's art scripts share: assets/sprites.art.luau and
-- backdrop.art.luau each take it with require("shared"). It holds the art's
-- scale and the helpers that round numbers and mix colours. It is not a
-- picture itself; change it and both scripts draw again.

local shared = {}

shared.PPU = 15 -- pixels a world unit, for everything in the level

-- A number to the nearest whole one, a half to the even one (88.5 is 88).
function shared.round(v)
  local f = math.floor(v)
  local d = v - f
  if d > 0.5 or (d == 0.5 and f % 2 == 1) then
    return f + 1
  end
  return f
end

-- A number with what is after its point dropped (-2.5 is -2, 2.5 is 2).
function shared.trunc(v)
  return v < 0 and math.ceil(v) or math.floor(v)
end

-- Between two colours, t of the way from a to b, each channel rounded as
-- round() does (art.mix rounds a half up instead).
function shared.mix(a, b, t)
  local out = "#"
  for i = 2, 6, 2 do
    local x, y = tonumber(a:sub(i, i + 1), 16), tonumber(b:sub(i, i + 1), 16)
    out ..= string.format("%02X", shared.round(x + (y - x) * t))
  end
  return out
end

return shared
