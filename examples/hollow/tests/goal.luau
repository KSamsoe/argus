-- The way out: from past the pit the hero runs into the exit, the run
-- finishes once and the HUD says how it went; restart plays it again from
-- the cave mouth. (The phase 2 playtest's mark: "The level needs a goal to
-- reach that isn't death".)
return scenario "the way out finishes the run and restart plays it again" {
  scene = "hollow", seed = 1,
  run = function(t)
    local tx = t:call("edit.begin", { author = "test:hollow", note = "past the pit" }).tx
    t:call("edit.set", { tx = tx, entity = "@player", path = "Transform.pos", value = { 62, 1.8 } })
    t:call("edit.commit", { tx = tx })
    t.input:hold("move", { 1, 0 }, 120)
    t:run_until { event = "run.finished", max_frames = 240 }
    t.expect:events("run.finished"):count():eq(1)
    t.expect:events("sound.played", { sound = "sfx/out" }):count():eq(1)
    t.expect:field("@hud", "Run.done"):eq(true)
    t.expect:field("@hud", "Run.falls"):eq(0)
    t:step(60)
    t.expect:events("run.finished"):count():eq(1)
    t.input:hold("restart", true, 2)
    t:step(3)
    t.expect:events("run.restarted"):count():eq(1)
    t.expect:field("@hud", "Run.done"):eq(false)
    t.expect:field("@player", "Transform.pos.x"):near(3, 0.5)
  end,
}
