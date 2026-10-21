-- What the duel's scenarios share: reading an entity, starting a match from
-- the title menu, and setting two fighters up face to face.
local M = {}

-- An entity's components now, as a table: M.get(t, "@one").Fighter.speed.
function M.get(t, id)
  return t:call("world.get", { entity = id }).components
end

-- A field's value now.
function M.field(t, id, path)
  return t.expect:field(id, path):get()
end

-- The run's first ticks, the title menu up.
function M.menu(t)
  t:step(12)
  if not M.field(t, "@menu", "UiRect.shown") then
    error("no title menu after 12 ticks", 2)
  end
end

-- From the title menu, a match on this machine: Two players here has the
-- focus as the menu opens, so confirm presses it, and the fight is on from
-- the tick after.
function M.start(t)
  M.menu(t)
  t.input:tap("ui_confirm", true)
  t:step(2)
  local state = M.field(t, "@arena", "Match.state")
  if state ~= "fight" then
    error("Two players here did not start a fight: Match.state is " .. state, 2)
  end
end

-- Puts a fighter at x, y facing fx, fy, in the running world only (sim.set
-- never changes the files).
function M.place(t, id, x, y, fx, fy)
  t:call("sim.set", { entity = id, path = "Transform.pos", value = { x, y } })
  t:call("sim.set", { entity = id, path = "Fighter.facing", value = { fx, fy } })
end

-- The two face to face, near the middle: @one at x, @two `gap` to its right.
function M.face_off(t, x, gap)
  M.place(t, "@one", x, 0, 1, 0)
  M.place(t, "@two", x + gap, 0, -1, 0)
end

return M
