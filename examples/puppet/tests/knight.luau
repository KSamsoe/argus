return scenario "the knight walks in its own skin" {
  scene = "stage", seed = 1,
  run = function(t)
    t:run_until { frames = 120 }
    t.expect:field("@knight", "Skeleton.shown"):eq("walk")
    t.expect:field("@knight", "Skeleton.loops"):gte(1)
    t.expect:field("@knight", "Skeleton.parts"):eq(9)
    t.expect:events("step", { entity = "@knight" }):count():gte(2)
  end,
}
