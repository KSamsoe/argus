-- The pause menu (phase 3 item 1): Escape opens it and the hero stands
-- still whatever the keys do; the menu is moved through and pressed with
-- the UI's actions; Resume closes it and the game goes on; Restart the run
-- starts again from the cave mouth.
return scenario "the pause menu stops the game and its buttons work" {
  scene = "hollow", seed = 1,
  run = function(t)
    t:step(2)
    t.expect:field("@pause", "UiRect.shown"):eq(false)
    t.input:hold("pause", true, 1)
    t:step(2)
    t.expect:field("@pause", "UiRect.shown"):eq(true)
    t.expect:field("@pause.resume", "UiButton.focused"):eq(true)
    t.expect:events("pause.opened"):count():eq(1)
    -- Frozen: running does nothing.
    local x = t:call("world.get", { entity = "@player" }).components.Transform.pos[1]
    t.input:hold("move", { 1, 0 }, 30)
    t:step(30)
    t.expect:field("@player", "Transform.pos.x"):near(x, 0.001)
    -- Down to Restart the run, then back up to Resume, and press it.
    t.input:hold("ui_move", { 0, -1 }, 1)
    t:step(2)
    t.expect:field("@pause.restart", "UiButton.focused"):eq(true)
    t.input:hold("ui_move", { 0, 1 }, 1)
    t:step(2)
    t.input:hold("ui_confirm", true, 1)
    t:step(2)
    t.expect:events("pause.resume"):count():eq(1)
    t.expect:field("@pause", "UiRect.shown"):eq(false)
    t.input:hold("move", { 1, 0 }, 30)
    t:step(30)
    t.expect:field("@player", "Transform.pos.x"):gt(x + 1)
    -- Restart the run from the menu: back to the cave mouth.
    t.input:hold("pause", true, 1)
    t:step(2)
    t.input:hold("ui_move", { 0, -1 }, 1)
    t:step(2)
    t.input:hold("ui_confirm", true, 1)
    t:step(3)
    t.expect:events("run.restarted"):count():eq(1)
    t.expect:field("@player", "Transform.pos.x"):near(3, 0.5)
    t.expect:field("@pause", "UiRect.shown"):eq(false)
  end,
}
