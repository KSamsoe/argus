return scenario "the show cues a wave and the hero waves once" {
  scene = "stage", seed = 1,
  run = function(t)
    -- The cue at tick 240 crossfades the walk into the wave.
    t:run_until { frames = 242 }
    t.expect:field("@hero", "Animator.state"):eq("wave")
    t.expect:field("@hero", "Skeleton.from"):eq("walk")
    t.expect:field("@hero", "Skeleton.mix"):gt(0.5)
    t:run_until { frames = 88 }
    t.expect:events("waved", { entity = "@hero" }):count():eq(1)
    t.expect:field("@world", "Show.waves"):eq(1)
    -- And back to standing when it is done.
    t:run_until { frames = 10 }
    t.expect:field("@hero", "Animator.state"):eq("idle")
  end,
}
