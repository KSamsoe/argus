-- A sign's words show while the hero is near it, and only then: its UI
-- panel, hung over it; the HUD is a UI text at the top.
return scenario "a sign speaks while the hero is near it" {
  scene = "hollow", seed = 1,
  run = function(t)
    t:step(2)
    t.expect:field("@c/sign", "UiNode.visible"):eq(false)
    t.expect:field("@c/sign", "UiRect.shown"):eq(false)
    t.expect:field("@hud", "UiRect.shown"):eq(true)
    t.expect:field("@hud", "UiText.shown"):eq("Hollow. Find the way out. No falls yet.")
    -- Beside the hall's sign, before the pit.
    local tx = t:call("edit.begin", { author = "test:hollow", note = "by the sign" }).tx
    t:call("edit.set", { tx = tx, entity = "@player", path = "Transform.pos", value = { 50.5, 1.8 } })
    t:call("edit.commit", { tx = tx })
    t:step(90)
    t.expect:field("@c/sign", "UiRect.shown"):eq(true)
    t.expect:field("@c/sign", "UiText.shown"):eq("Mind the pit.")
    t.expect:field("@b/sign", "UiRect.shown"):eq(false)
    -- Over the sign, once the camera has come: its panel is above it.
    local f = t:call("obs.frame", { format = "semantic" })
    local words = nil
    for _, u in ipairs(f.ui) do
      if u.text == "Mind the pit." then words = u end
    end
    assert(words ~= nil, "the sign's words are in the semantic frame")
    local sign = nil
    for _, row in ipairs(f.entities) do
      if row[1] == "@c/sign" then sign = row[4] end
    end
    assert(sign ~= nil, "the sign is in the frame")
    assert(words.bounds[4] <= sign[2], "the words sit over the sign")
    assert(words.bounds[1] < sign[3] and words.bounds[3] > sign[1], "and across it")
  end,
}
