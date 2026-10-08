-- The controls (phase 5 item 2): from the pause menu, a page that shows
-- the pause key and rebinds it, and says how to walk (four keys or a
-- stick, not one key to rebind); Back returns to the menu.
return scenario "the controls page shows the pause key and rebinds it" {
  scene = "arena", seed = 7,
  run = function(t)
    t:step(2)
    t.input:hold("pause", true, 1)
    t:step(2)
    -- Down to Controls, the third button.
    for _ = 1, 2 do
      t.input:hold("ui_move", { 0, -1 }, 1)
      t:step(2)
    end
    t.expect:field("@pause/panel/controls", "UiButton.focused"):eq(true)
    t.input:hold("ui_confirm", true, 1)
    t:step(2)
    t.expect:field("@pause/controls", "UiRect.shown"):eq(true)
    t.expect:field("@pause/panel", "UiRect.shown"):eq(false)
    t.expect:field("@pause/controls/pause", "UiButton.focused"):eq(true)
    t.expect:field("@pause/controls/pause", "UiText.shown"):eq("Pause: escape")
    t.expect:field("@pause/controls/move", "UiText.shown"):eq("Walk: W A S D, or the left stick")
    -- The semantic frame reads the page out: the line, and the buttons
    -- with the focused one.
    local f = t:call("obs.frame", { format = "semantic" })
    local list, focused, walk = {}, nil, false
    for _, u in ipairs(f.ui) do
      if u.button then
        table.insert(list, u.text)
        if u.focused then focused = u.text end
      elseif u.text == "Walk: W A S D, or the left stick" then
        walk = true
      end
    end
    if table.concat(list, " | ") ~= "Pause: escape | Back" then error("the semantic frame's buttons: " .. table.concat(list, " | "), 0) end
    if focused ~= "Pause: escape" then error("the semantic frame's focus: " .. tostring(focused), 0) end
    if not walk then error("the semantic frame says how to walk", 0) end
    -- Pressed, it waits for a key.
    t.input:hold("ui_confirm", true, 1)
    t:step(2)
    t.expect:field("@pause/controls/pause", "InputBinding.listening"):eq(true)
    t.expect:field("@pause/controls/pause", "UiText.shown"):eq("Pause: press a key")
    -- The key (here through the protocol, as the window hands one over).
    t:call("input.bind", { action = "pause", bindings = { "p" } })
    t:step(2)
    t.expect:field("@pause/controls/pause", "UiText.shown"):eq("Pause: p")
    t.expect:field("@pause/controls/pause", "InputBinding.listening"):eq(false)
    -- Back: the pause menu again.
    t.input:hold("ui_cancel", true, 1)
    t:step(2)
    t.expect:field("@pause/controls", "UiRect.shown"):eq(false)
    t.expect:field("@pause/panel", "UiRect.shown"):eq(true)
    -- Put the key back for the other tests' sake (bindings live in the
    -- daemon's map, not the world).
    t:call("input.bind", { action = "pause", bindings = { "escape" } })
  end,
}
