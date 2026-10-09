return scenario "the hero stands, then walks, and its feet land" {
  scene = "stage", seed = 1,
  run = function(t)
    -- A second and a half of idle, then the animator crossfades to the
    -- walk; every footstep is a Spine event.
    t:run_until { frames = 60 }
    t.expect:field("@hero", "Animator.state"):eq("idle")
    t.expect:field("@hero", "Skeleton.animation"):eq("idle")
    t:run_until { frames = 140 }
    t.expect:field("@hero", "Skeleton.animation"):eq("walk")
    t.expect:field("@hero", "Transform.pos.x"):gt(-2.0)
    t.expect:events("step", { entity = "@hero" }):count():gte(3)
    -- Nine slots each, every one a part under its puppet.
    t.expect:count({ has = { "SkeletonPart" } }):eq(18)
    t.expect:field("@hero", "Skeleton.parts"):eq(9)
  end,
}
