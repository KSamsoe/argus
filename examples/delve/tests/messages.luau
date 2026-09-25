-- The message line (phase 3 item 4): what a turn did, said to the player
-- in the string table's words. A hero who waits is struck, then killed; one
-- who asks for stairs where there are none is told so.
return scenario "the message line says what the turn did" {
  scene = "delve", seed = 1,
  run = function(t)
    t:advance { by = "turn", n = 1 }
    t.input:tap("descend")
    t:advance { by = "turn", n = 1 }
    t.expect:field("@messages", "UiText.shown"):eq("There are no stairs here.")
    t:advance { by = "turn", n = 120 }
    t.expect:events("player.died"):count():eq(1)
    local died = t.expect:events("player.died"):first().fields
    local shown = t.expect:field("@messages", "UiText.shown"):get()
    local want = string.format("kills you on floor %d, after %d kills.", died.floor, died.kills)
    if not string.find(shown, want, 1, true) or not string.find(shown, "The " .. died.kind, 1, true) then
      error("the last message: " .. tostring(shown), 0)
    end
    t:log(shown)
  end,
}
