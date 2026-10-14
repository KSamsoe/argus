-- The saves page (2D staples item 4): from the menu, three slots. Save to
-- slot 1 and it says what it holds (the floor, the turn, the kills, from
-- the summary the save kept); delete it and it is empty again. No turn
-- passes while the page is used.
return scenario "the saves page saves a slot, says what it holds, and deletes it" {
  scene = "delve", seed = 3,
  run = function(t)
    t:advance { by = "turn", n = 3 }
    local turn = t:call("world.get", { entity = "@turn" }).components.Turn.n
    t.input:hold("pause", true, 1)
    t:advance { by = "turn", n = 1 }
    -- Down to Saves, the fifth button.
    for _ = 1, 4 do
      t.input:hold("ui_move", { 0, -1 }, 1)
      t:advance { by = "turn", n = 2 }
    end
    t.expect:field("@menu.saves", "UiButton.focused"):eq(true)
    t.input:hold("ui_confirm", true, 1)
    t:advance { by = "turn", n = 1 }
    t.expect:field("@saves", "UiRect.shown"):eq(true)
    t.expect:field("@saves.slot1.text", "UiText.shown"):eq("Slot 1: empty")
    t.expect:field("@saves.slot1.load", "UiButton.disabled"):eq(true)
    -- Save to slot 1. A save is written after the step that asked, as a
    -- window's press is a step and its release the next: the press shows
    -- what it saved, and from the release q:saves() lists it.
    t.expect:field("@saves.slot1.save", "UiButton.focused"):eq(true)
    t.input:hold("ui_confirm", true, 1)
    t:advance { by = "turn", n = 1 }
    t:advance { by = "turn", n = 1 }
    t.expect:events("game.saved", { save = "slot1" }):count():eq(1)
    t.expect:field("@saves.slot1.text", "UiText.shown"):eq(
      string.format("Slot 1: floor 1, turn %d, 0 kills", turn)
    )
    t.expect:field("@saves.slot1.load", "UiButton.disabled"):eq(false)
    t.expect:field("@saves.slot2.text", "UiText.shown"):eq("Slot 2: empty")
    -- Delete it: right twice, from Save to Delete.
    for _ = 1, 2 do
      t.input:hold("ui_move", { 1, 0 }, 1)
      t:advance { by = "turn", n = 2 }
    end
    t.expect:field("@saves.slot1.delete", "UiButton.focused"):eq(true)
    t.input:hold("ui_confirm", true, 1)
    t:advance { by = "turn", n = 1 }
    t:advance { by = "turn", n = 1 }
    t.expect:events("game.save_deleted", { save = "slot1" }):count():eq(1)
    t.expect:field("@saves.slot1.text", "UiText.shown"):eq("Slot 1: empty")
    -- None of it took a turn.
    t.expect:field("@turn", "Turn.n"):eq(turn)
  end,
}
