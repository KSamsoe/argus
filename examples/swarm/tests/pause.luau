-- The pause menu (phase 5 item 2): Escape opens it and the swarm stands
-- still, the player too, whatever the keys do; the menu is moved through
-- and pressed with the UI's actions; Resume closes it and the game goes
-- on. Restart, after a death, starts the run again from wave 1.

-- The open menu's buttons as the semantic frame reads them, and the one
-- with focus.
local function buttons(t)
  local f = t:call("obs.frame", { format = "semantic" })
  local out, focused = {}, nil
  for _, u in ipairs(f.ui) do
    if u.button then
      table.insert(out, u.text)
      if u.focused then focused = u.text end
    end
  end
  return table.concat(out, " | "), focused
end

scenario "the pause menu stops the swarm and its buttons work" {
  scene = "arena", seed = 7,
  run = function(t)
    t:step(10)
    t.expect:field("@pause", "UiRect.shown"):eq(false)
    t.input:hold("pause", true, 1)
    t:step(2)
    t.expect:field("@pause", "UiRect.shown"):eq(true)
    t.expect:field("@pause/panel/resume", "UiButton.focused"):eq(true)
    t.expect:events("pause.opened"):count():eq(1)
    -- The semantic frame reads the menu out, Resume focused.
    local list, focused = buttons(t)
    if list ~= "Resume | Restart from wave 1 | Controls | Settings" then error("the semantic frame's buttons: " .. list, 0) end
    if focused ~= "Resume" then error("the semantic frame's focus: " .. tostring(focused), 0) end
    -- Frozen: walking does nothing, no enemy comes on, no bullet flies.
    local x = t:call("world.get", { entity = "@player" }).components.Transform.pos[1]
    local one = t:call("world.query", { select = { "id", "Transform.pos" }, where = { has = { "Enemy" } }, limit = 1 }).rows[1]
    local bullets = t:call("world.query", { select = { "id" }, where = { has = { "Bullet" } }, limit = 1 }).total
    t.input:hold("move", { 1, 0 }, 30)
    t:step(30)
    t.expect:field("@player", "Transform.pos.x"):near(x, 0.001)
    t.expect:field(one[1], "Transform.pos.x"):near(one[2][1], 0.001)
    t.expect:field(one[1], "Transform.pos.y"):near(one[2][2], 0.001)
    t.expect:count({ has = { "Bullet" } }):eq(bullets)
    -- Down to Restart, then back up to Resume, and press it.
    t.input:hold("ui_move", { 0, -1 }, 1)
    t:step(2)
    t.expect:field("@pause/panel/restart", "UiButton.focused"):eq(true)
    t.input:hold("ui_move", { 0, 1 }, 1)
    t:step(2)
    t.input:hold("ui_confirm", true, 1)
    t:step(2)
    t.expect:events("pause.resume"):count():eq(1)
    t.expect:field("@pause", "UiRect.shown"):eq(false)
    t.input:hold("move", { 1, 0 }, 30)
    t:step(30)
    t.expect:field("@player", "Transform.pos.x"):gt(x + 1)
    -- Escape again opens it, and Escape closes it.
    t.input:hold("pause", true, 1)
    t:step(2)
    t.expect:field("@pause", "UiRect.shown"):eq(true)
    t.input:hold("pause", true, 1)
    t:step(2)
    t.expect:field("@pause", "UiRect.shown"):eq(false)
    t.expect:events("pause.closed"):count():eq(2)
  end,
}

scenario "Restart after a death starts again from wave 1" {
  scene = "last_stand", seed = 1,
  run = function(t)
    t:run_until { event = "player.died", max_frames = 3000 }
    t.expect:field("@player", "Player.dead"):eq(true)
    t.input:hold("pause", true, 1)
    t:step(2)
    t.input:hold("ui_move", { 0, -1 }, 1)
    t:step(2)
    t.expect:field("@pause/panel/restart", "UiButton.focused"):eq(true)
    t.input:hold("ui_confirm", true, 1)
    t:step(1)
    t.expect:events("run.restarted"):count():eq(1)
    t.expect:field("@pause", "UiRect.shown"):eq(false)
    t.expect:count({ has = { "Enemy" } }):eq(0)
    t.expect:count({ has = { "Bullet" } }):eq(0)
    t.expect:field("@player", "Player.dead"):eq(false)
    t.expect:field("@player", "Player.kills"):eq(0)
    t.expect:field("@player", "Health.cur"):eq(10)
    t.expect:field("@director", "Director.wave"):eq(0)
    -- The next tick, the first wave again.
    t:step(1)
    t.expect:events("wave.spawned", { wave = 1 }):count():eq(2)
    t.expect:count({ has = { "Enemy" } }):eq(400)
    t.expect:field("@director", "Director.wave"):eq(1)
    -- And it plays on: the player stands, fires, and is hit again.
    t:run_until { event = "damage", max_frames = 3000 }
    t.expect:field("@player", "Player.kills"):gt(0)
  end,
}
