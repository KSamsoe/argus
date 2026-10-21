-- The title menu: it shows at the start with the fight standing still under
-- it; Two players here starts a match on this machine, and cancel goes back.
-- Host online and Join online ask whatever runs the game for a room, as
-- events (net.host_requested, net.join_requested). Nothing answers them in
-- a test, so q:net() stays alone and the menu stays up.
local duel = require("duel")

return {
  scenario "the title menu shows at the start, the fight stands still under it, and Two players here starts a match" {
    scene = "arena", seed = 1,
    run = function(t)
      -- The menu from the first tick, the fight standing still under it.
      t:step(1)
      t.expect:field("@menu", "UiRect.shown"):eq(true)
      t.expect:field("@arena", "Match.state"):eq("menu")
      duel.menu(t)
      t.expect:field("@menu", "UiRect.shown"):eq(true)
      t.expect:field("@arena", "Match.state"):eq("menu")
      t.expect:field("@menu/panel/here", "UiButton.focused"):eq(true)
      local x = duel.field(t, "@one", "Transform.pos.x")
      t.input:hold("move", { 1, 0 }, 30)
      t:step(30)
      t.expect:field("@one", "Transform.pos.x"):eq(x)

      t.input:tap("ui_confirm", true)
      t:step(2)
      t.expect:field("@menu", "UiRect.shown"):eq(false)
      t.expect:field("@arena", "Match.state"):eq("fight")
      t.expect:events("match.started", { online = false }):count():eq(1)
      t.input:hold("move", { 1, 0 }, 30)
      t:step(30)
      t.expect:field("@one", "Transform.pos.x"):gt(x + 3)

      -- Cancel (Escape) in a match on this machine: back to the menu.
      t.input:tap("ui_cancel", true)
      t:step(2)
      t.expect:field("@menu", "UiRect.shown"):eq(true)
      t.expect:field("@arena", "Match.state"):eq("menu")
    end,
  },

  scenario "Host online asks for a room for two; with no relay to answer it, the game stays alone" {
    scene = "arena", seed = 1,
    run = function(t)
      duel.menu(t)
      t.input:tap("ui_move", { 0, -1 })
      t:step(1)
      t.expect:field("@menu/panel/host", "UiButton.focused"):eq(true)
      t.input:tap("ui_confirm", true)
      t:step(2)
      t.expect:events("net.host_requested", { players = 2, scene = "arena" }):count():eq(1)
      t.expect:field("@arena", "Match.state"):eq("menu")
      t.expect:field("@menu", "UiRect.shown"):eq(true)
      t.expect:field("@menu/panel/status", "UiText.text"):eq("")
      local net = t:call("net.status", {})
      if net.state ~= "alone" then
        error("q:net() is " .. tostring(net.state) .. ", not alone", 0)
      end
    end,
  },

  scenario "Join online types the room's code into its box, and Enter joins" {
    scene = "arena", seed = 1,
    run = function(t)
      duel.menu(t)
      t.input:tap("ui_move", { 0, -1 })
      t:step(2)
      t.input:tap("ui_move", { 0, -1 })
      t:step(2)
      t.expect:field("@menu/panel/join/button", "UiButton.focused"):eq(true)
      -- Join with nothing typed: the box types, from the tick after next
      -- (what is typed before then is not the box's).
      t.input:tap("ui_confirm", true)
      t:step(3)
      t.expect:events("net.join_requested"):count():eq(0)
      t.expect:field("@menu/panel/join/code", "UiTextBox.typing"):eq(true)
      -- The keyboard types the code (input.device's text, as the window's
      -- typing comes), and Enter sends it, in capitals.
      t:call("input.device", { text = { "kq7xm" } })
      t:step(1)
      t.expect:field("@menu/panel/join/code", "UiTextBox.text"):eq("kq7xm")
      t:call("input.device", { text = { string.char(13) } })
      t:step(2)
      t.expect:events("net.join_requested", { code = "KQ7XM" }):count():eq(1)
      t.expect:field("@menu/panel/join/code", "UiTextBox.typing"):eq(false)
      t.expect:field("@arena", "Match.state"):eq("menu")
    end,
  },
}
