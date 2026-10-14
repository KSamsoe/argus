-- Line of sight (2D staples items 0 and 1): a monster sees the hero when a
-- ray from its cell to the hero's meets no wall. With @turn's
-- DebugView.sight on, sim.monsters draws each sight line, which the
-- semantic frame lists under debug: green where it sees, red to the wall.
-- Off, nothing is drawn.

local function set(t, entity, path, value)
  local tx = t:call("edit.begin", { author = "test:delve", note = "sight lines" }).tx
  t:call("edit.set", { tx = tx, entity = entity, path = path, value = value })
  t:call("edit.commit", { tx = tx })
end

local function lines(t)
  local f = t:call("obs.frame", { format = "semantic" })
  local seen, walled = 0, 0
  for _, s in ipairs(f.debug and f.debug.shapes or {}) do
    if s.by == "sim.monsters" and s.line then
      if s.color == "#60ff90" then seen += 1 else walled += 1 end
    end
  end
  return seen, walled
end

return scenario "monsters see the hero only past no wall, and the sight lines draw" {
  scene = "delve", seed = 3,
  run = function(t)
    t:advance { by = "turn", n = 1 }
    local seen, walled = lines(t)
    if seen + walled ~= 0 then
      error(string.format("sight lines drawn with the debug view off: %d", seen + walled), 0)
    end
    set(t, "@turn", "DebugView.sight", true)
    t:advance { by = "turn", n = 1 }
    seen, walled = lines(t)
    -- The first floor's monsters start in other rooms: walls are between.
    if walled == 0 then
      error(string.format("no sight line ends at a wall (%d see the hero)", seen), 0)
    end
  end,
}
