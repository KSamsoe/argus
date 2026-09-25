-- The controls (phase 3 item 1): from the pause menu, a page of buttons
-- that show each action's key and rebind it; Back returns to the menu.
return scenario "the controls page shows the keys and rebinds them" {
  scene = "hollow", seed = 1,
  run = function(t)
    t:step(2)
    t.input:hold("pause", true, 1)
    t:step(2)
    -- Down to Controls, the third button.
    for _ = 1, 2 do
      t.input:hold("ui_move", { 0, -1 }, 1)
      t:step(2)
    end
    t.expect:field("@pause.controls", "UiButton.focused"):eq(true)
    t.input:hold("ui_confirm", true, 1)
    t:step(2)
    t.expect:field("@controls", "UiRect.shown"):eq(true)
    t.expect:field("@pause.panel", "UiRect.shown"):eq(false)
    t.expect:field("@controls.jump", "UiButton.focused"):eq(true)
    t.expect:field("@controls.jump", "UiText.shown"):eq("Jump: space, z")
    t.expect:field("@controls.pause", "UiText.shown"):eq("Pause: escape")
    -- Pressed, it waits for a key.
    t.input:hold("ui_confirm", true, 1)
    t:step(2)
    t.expect:field("@controls.jump", "InputBinding.listening"):eq(true)
    t.expect:field("@controls.jump", "UiText.shown"):eq("Jump: press a key")
    -- The key (here through the protocol, as the window hands one over).
    t:call("input.bind", { action = "jump", bindings = { "x" } })
    t:step(2)
    t.expect:field("@controls.jump", "UiText.shown"):eq("Jump: x")
    t.expect:field("@controls.jump", "InputBinding.listening"):eq(false)
    -- Back: the pause menu again.
    t.input:hold("ui_cancel", true, 1)
    t:step(2)
    t.expect:field("@controls", "UiRect.shown"):eq(false)
    t.expect:field("@pause.panel", "UiRect.shown"):eq(true)
    -- Put the key back for the other tests' sake (bindings live in the
    -- daemon's map, not the world).
    t:call("input.bind", { action = "jump", bindings = { "space", "z" } })
  end,
}
