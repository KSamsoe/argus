-- The message log (phase 3 item 4, issue 23): what a turn did, said to the
-- player in the string table's words. A hero who waits is struck, then
-- killed; one who asks for stairs where there are none is told so. The log
-- keeps the turns before, the newest at the bottom; the wheel over it
-- scrolls back, and takes no turn.

local function field(t, path, where)
  local q = t:call("world.query", { select = { path }, where = where })
  return q.rows[1] and q.rows[1][1]
end

return {
  scenario "the message line says what the turn did" {
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
  },

  scenario "the log keeps the turns before and the wheel scrolls it back without a turn" {
    scene = "delve", seed = 1,
    run = function(t)
      for _ = 1, 6 do
        t.input:tap("descend")
        t:advance { by = "turn", n = 1 }
      end
      local lines = t:call("world.query", { select = { "count()" }, where = { has = { "LogLine" } } })
      assert(lines.rows[1][1] == 5, "five turns before the newest: " .. tostring(lines.rows[1][1]))
      local log = { has = { "UiScroll" } }
      assert(field(t, "UiScroll.at_end", log) == true, "at the end")
      local at = field(t, "UiScroll.y", log)
      assert(at > 0, "scrolled to the newest: " .. tostring(at))
      local turn = field(t, "Turn.n", { has = { "Turn" } })
      -- The wheel over the log: one notch back, and no turn passes.
      local rect = field(t, "UiRect.pos", log)
      t.input:hold("pointer", { rect[1] + 20, rect[2] + 20 })
      t.input:tap("ui_scroll", 1)
      t:advance { by = "turn", n = 1 }
      t.expect:field("@turn", "Turn.n"):eq(turn)
      assert(field(t, "UiScroll.y", log) < at, "scrolled back")
      assert(field(t, "UiScroll.at_end", log) == false)
      -- A new turn's words: the log stays where it was put.
      local back = field(t, "UiScroll.y", log)
      t.input:tap("descend")
      t:advance { by = "turn", n = 1 }
      t.expect:field("@turn", "Turn.n"):eq(turn + 1)
      assert(field(t, "UiScroll.y", log) == back, "it stays where it was scrolled to")
    end,
  },
}
