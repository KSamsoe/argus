-- Building the UI trees the inspector and the menu are made of: a node
-- under a parent, and a button that sends an event when pressed.
local M = {}

-- A UI node under parent, with the components names lists besides UiNode
-- and Parent.
function M.spawn(q, parent, names)
  local all = { "UiNode", "Parent" }
  for _, n in ipairs(names or {}) do table.insert(all, n) end
  local e = q:spawn(all)
  e:get("Parent").entity = parent
  return e
end

-- A button under parent that sends event when pressed: its words from the
-- string table's key (none: set later), pad its padding, align its words'
-- ("center" when nil).
function M.button(q, parent, event, key, pad, align)
  local b = M.spawn(q, parent, { "UiPanel", "UiButton", "UiText" })
  b:get("UiNode").pad = pad
  b:get("UiPanel").color = "button"
  b:get("UiButton").event = event
  local t = b:get("UiText")
  if key ~= nil then t.key = key end
  t.align = align or "center"
  return b
end

return M
