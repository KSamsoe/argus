-- Lanterns burning (the lanterns mod): a dark lantern is dim; a lit one
-- is warm and flickers, each from its own phase.
return system "lanterns.flicker" {
  stage = "late",
  reads = { "lanterns_Lantern" },
  writes = { "Sprite" },
  doc = "Draws each lantern dark until lit, then warm, its height flickering a few percent.",
  run = function(q)
    for e, l in q:each("lanterns_Lantern") do
      local s = e:get("Sprite")
      if l.lit then
        s.color = "#FFC060"
        s.size = vector.create(0.5, 0.8 * (1 + 0.06 * math.sin(q.time * 9 + l.phase)), 0)
      else
        s.color = "#403828"
        s.size = vector.create(0.5, 0.8, 0)
      end
    end
  end,
}
