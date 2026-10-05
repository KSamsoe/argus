-- Image goldens (section 9): opt-in for rendering work, and rare. The cave
-- as the run starts, and the hall with the hero in it, each drawn with the
-- software rasteriser and compared with its golden as a person would see
-- it. A change to how Hollow looks fails here with a picture of where;
-- `argus test name=look update=true` records the new look.
return scenario "the cave and the hall look as they did" {
  scene = "hollow", seed = 1,
  run = function(t)
    t:step(1)
    t.expect:image("cave")
    t.input:hold("move", { 1, 0 }, 450)
    t:step(118)
    t.input:hold("jump", true, 18)
    t:step(330)
    t.expect:image("hall")
  end,
}
