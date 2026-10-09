-- The whole level, played as a player would: run right, jump the pit, jump
-- the spikes, jump up onto the ledge, and touch the flag, taking the three
-- coins on the way without a fall; then R plays it again. When this fails
-- after a change to the hero's feel (player.toml's CharacterController) or
-- to the level, the level can no longer be played this way: change the
-- jump points below, or the level, to fit.
--
-- A scenario runs the game headless with scripted input (t.input), and
-- t.expect says what must be true by then. `argus test` runs every file in
-- tests/.

local function hero_x(t)
  return t:call("world.get", { entity = "@player" }).components.Transform.pos[1]
end

-- Run on until the hero's middle is past x, then make a full jump and wait
-- for the landing (the engine's body.landed event).
local function jump_at(t, x)
  for _ = 1, 600 do
    if hero_x(t) >= x then
      t.input:hold("jump", true, 20)
      t:step(2)
      t:run_until { event = "body.landed", max_frames = 120 }
      return
    end
    t:step(1)
  end
  error("the hero never got to x " .. x, 2)
end

return scenario "the level is played from the start to the flag, then again" {
  scene = "level", seed = 1,
  run = function(t)
    t:step(10)                        -- the hero drops onto the ground
    t.input:hold("move", { 1, 0 })    -- right, until released

    jump_at(t, 15.0)                  -- the pit, x 16 to 19
    t.expect:field("@player", "Transform.pos.x"):gt(19)
    t.expect:field("@player", "Transform.pos.y"):near(0.6, 0.01)

    jump_at(t, 24.0)                  -- the spikes, x 25 to 27
    t.expect:field("@player", "Transform.pos.x"):gt(27)

    jump_at(t, 34.0)                  -- up onto the ledge: x 36 on, 2.5 high
    t.expect:field("@player", "Transform.pos.y"):near(3.1, 0.01)

    t:run_until { event = "goal.reached", max_frames = 300 }
    t.expect:events("goal.reached"):count():eq(1)
    t.expect:events("hero.hurt"):count():eq(0)
    t.expect:events("coin.taken"):count():eq(3)
    t.expect:field("@player", "Hero.done"):eq(true)
    t.expect:field("@hud", "UiText.text"):eq("You reached the flag! Coins 3/3, falls 0. Press R to play again.")
    -- The camera came along, and stopped at the level's right edge.
    t.expect:field("@camera", "Camera.view_max.x"):near(49, 0.01)

    -- R starts the level again: the hero back at the start, the coins back.
    t.input:release("move")
    t.input:hold("restart", true, 1)
    t:step(2)
    t.expect:events("level.restarted"):count():eq(1)
    t.expect:field("@player", "Transform.pos.x"):near(3, 0.01)
    t.expect:field("@player", "Hero.coins"):eq(0)
    t.expect:field("@player", "Hero.done"):eq(false)
    t.expect:field("@coin.1", "Sprite.visible"):eq(true)
    t.expect:field("@hud", "UiText.text"):eq("Coins 0/3    Falls 0")
  end,
}
