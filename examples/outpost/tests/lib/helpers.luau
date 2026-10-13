-- What Outpost's scenarios share: the one row a query finds, a field set
-- by an edit, and an action pressed.
local M = {}

-- The one row world.query finds, or an error saying how many it found.
function M.one(t, select, where)
  local q = t:call("world.query", { select = select, where = where })
  if #q.rows ~= 1 then error("one row for " .. select[1] .. ", not " .. #q.rows, 0) end
  return q.rows[1]
end

-- One field of one entity set, as an edit with a note saying why.
function M.set(t, entity, path, value, note)
  local tx = t:call("edit.begin", { author = "test:outpost", note = note }).tx
  t:call("edit.set", { tx = tx, entity = entity, path = path, value = value })
  t:call("edit.commit", { tx = tx })
end

-- An action tapped, and two ticks for the game to take it.
function M.press(t, action, value)
  t.input:tap(action, value)
  t:step(2)
end

return M
