-- The title menu, the win screen, and the room online.
--
-- input.menu runs every tick, first. It shows the title menu while no match
-- is on (Match.state menu) and stands the fight still under it: q:freeze()
-- stops every system not marked `unfrozen` for the rest of the tick. It
-- shows the win screen while a match is won. And it follows q:net(): once
-- both players are in an online room the match starts on its own, opening
-- this scene afresh on both machines, where q:net() says `playing`, so the
-- fight begins without the menu. In a match q:net() says only that, never
-- which player this machine is: the world must be the same on every
-- machine, so nothing in it may depend on which one it is.
--
-- sim.menu does what the menu's buttons ask. A button is pressed on a frozen
-- tick (input.menu froze it before the UI knew), so it is unfrozen too.
-- Hosting and joining are asks of whatever runs the game (the desktop
-- game, the web page): q:net() then says how they went.

-- The menu's status line: what the room online is doing.
local function status(n)
  if n.state == "room" then
    return "Room " .. (n.code or "?") .. ": waiting for the other player"
  elseif n.state == "asking" then
    return "Asking the relay"
  elseif n.state == "ended" then
    return n.why or "The match is over"
  end
  return ""
end

return {
  system "input.menu" {
    stage = "input",
    after = { "input.actions" },
    unfrozen = true,
    reads = { "Actions" },
    writes = { "Match", "UiNode", "UiText" },
    doc = "Shows the title menu (@menu) while Match.state is menu, with q:net()'s room in its status line, and freezes the rest of each tick while it is menu; shows the win screen (@win) while it is won. An online match starting (q:net() playing, from its first tick) starts the match; one ending goes back to the menu. Cancel in a match on this machine goes back to the menu.",
    run = function(q)
      local m = q:entity("@arena"):get("Match")
      local n = q:net()
      if n.state == "playing" and not m.online then
        -- An online match: the scene was opened afresh for it.
        m.online = true
        m.state = "start"
      elseif m.online and n.state ~= "playing" then
        -- It is over (a player left, or the relay went): back to the menu,
        -- whose status line says why.
        m.online = false
        m.state = "menu"
      elseif not m.online and m.state ~= "menu" then
        for _, a in q:each("Actions") do
          if a.ui_cancel_pressed then
            m.state = "menu"
          end
        end
      end

      local win = q:entity("@win"):get("UiNode")
      if win.visible ~= (m.state == "won") then
        win.visible = m.state == "won"
      end
      local menu = q:entity("@menu"):get("UiNode")
      local shown = m.state == "menu"
      if menu.visible ~= shown then
        menu.visible = shown
      end
      if m.state == "menu" then
        local line = q:entity("@menu/panel/status"):get("UiText")
        local says = status(n)
        if line.text ~= says then
          line.text = says
        end
        q:freeze()
      end
    end,
  },

  system "sim.menu" {
    stage = "sim",
    after = { "ui.input" },
    unfrozen = true,
    writes = { "Match", "UiTextBox", "UiRoot" },
    doc = "The title menu's buttons: Two players here starts a match on this machine (Match.state start); Host online asks for a room for two (q:net_host); Join online joins the room whose code is in the box (q:net_join), or starts typing one; Enter in the box joins too.",
    run = function(q)
      if #q:events("menu.here") > 0 then
        q:entity("@arena"):get("Match").state = "start"
      end
      if #q:events("menu.host") > 0 then
        q:net_host(2, "arena")
      end
      -- The code: from the box when Enter sends it, or when Join is pressed
      -- with one typed. Codes are capitals; spaces are dropped.
      local box = q:entity("@menu/panel/join/code")
      local typed = box:get("UiTextBox")
      local code = nil
      for _, ev in ipairs(q:events("menu.code")) do
        code = ev.text
      end
      if #q:events("menu.join") > 0 then
        if typed.text ~= "" then
          code = typed.text
        else
          -- Nothing typed yet: type it, in the box, which takes the focus.
          typed.typing = true
          q:entity("@menu"):get("UiRoot").focus = box
        end
      end
      if code ~= nil then
        code = string.upper((string.gsub(code, "%s", "")))
        if code ~= "" then
          q:net_join(code)
        end
      end
    end,
  },
}
