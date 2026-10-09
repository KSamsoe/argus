-- The core loop: a hero who stands in the middle and lets the auto-fire
-- work clears wave 1 alive, and wave 2 comes after the gap, bigger. If you
-- make enemies tougher or the hero weaker, this is the test that says
-- whether wave 1 can still be won without moving.
local function get(t, id)
  return t:call("world.get", { entity = id }).components
end

return scenario "a hero who stands and fires clears wave 1, and a bigger wave 2 comes" {
  scene = "main", seeds = range(1, 3),
  run = function(t)
    local w = get(t, "@arena").Waves
    t:run_until { event = "wave.cleared", max_frames = 3000 }
    t.expect:count({ has = { "Enemy" } }):eq(0)
    t.expect:events("enemy.killed"):count():eq(w.first)
    -- Every bullet but the last on each enemy hurts without killing.
    t.expect:events("enemy.hit"):count():eq(w.first * (w.enemy_health - 1))
    t.expect:field("@player", "Player.score"):eq(w.first)
    t.expect:field("@player", "Player.dead"):eq(false)
    t.expect:field("@player", "Health.cur"):gte(1)

    -- After Waves.gap ticks of quiet, wave 2, with `more` enemies more.
    t:run_until { event = "wave.spawned", max_frames = w.gap + 10 }
    t.expect:events("wave.spawned", { wave = 2, count = w.first + w.more }):count():eq(1)
    t.expect:count({ has = { "Enemy" } }):eq(w.first + w.more)

    -- The HUD says so (UiText.shown is its line with the holes filled).
    local hud = t.expect:field("@hud", "UiText.shown"):get()
    for _, says in ipairs({ "Wave 2", "Score " .. w.first }) do
      if string.find(hud, says, 1, true) == nil then
        error("the HUD reads \"" .. hud .. "\", without \"" .. says .. "\"", 0)
      end
    end
  end,
}
