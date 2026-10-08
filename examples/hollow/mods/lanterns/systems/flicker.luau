-- Lanterns burning (the lanterns mod): a dark lantern is dim; a lit one
-- is warm and flickers, each from its own phase. Since the hollow is dark
-- (phase 5 item 3) a lit lantern also shines: its Light comes on with it.
return system "lanterns.flicker" {
  stage = "late",
  reads = { "lanterns_Lantern" },
  writes = { "Sprite", "Light" },
  doc = "Draws each lantern dark until lit, then warm, its height flickering a few percent; its Light is on while it is lit.",
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
      if e:has("Light") then
        local light = e:get("Light")
        if light.on ~= l.lit then light.on = l.lit end
      end
    end
  end,
}
