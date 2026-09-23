-- The gate's path: from the cave, over the step, through the shaft's
-- doorway, into the hall. The camera changes rooms twice, the feet make
-- footsteps, and the jump over the step lands with a thud.
return scenario "the hero runs from the cave through the shaft into the hall" {
  scene = "hollow", seed = 1,
  run = function(t)
    t.input:hold("move", { 1, 0 }, 450)
    t:step(118)                          -- up against the step in the cave
    t.input:hold("jump", true, 18)
    t:step(330)                          -- into the hall, short of its pit
    t.expect:field("@player", "Transform.pos.x"):between(44, 56)
    t.expect:events("camera.zone"):count():eq(3)
    t.expect:events("camera.zone", { zone = "c" }):count():eq(1)
    t.expect:field("@camera", "Camera.view_min.x"):near(44, 0.01)  -- held at the hall's left wall
    t.expect:events("sound.played", { sound = "sfx/step" }):count():gte(20)
    t.expect:events("anim.entered", { state = "jump" }):count():gte(1)
    t.expect:events("anim.entered", { state = "land" }):count():gte(1)
    t.expect:events("sound.played", { sound = "sfx/land" }):count():gte(1)
    t.expect:perf("sim"):p95_ms():lt(4.0)
  end,
}
