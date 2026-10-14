-- The message log (phase 3 item 4, issue 23): what the turn did, in words
-- from the string tables. Delve is mostly state and text; its events said
-- what happened to the agent (the log) and, until now, never to the player.
-- The newest turn's words are @messages, at the bottom of the log; the turn
-- before's move up into a line of their own, and the oldest go past KEEP.

local KEEP = 40

return system "sim.messages" {
  stage = "sim",
  after = { "sim.monsters", "sim.potions" },
  -- A drink with no potion freezes the rest of the turn (no turn passes);
  -- the line still says so.
  unfrozen = true,
  reads = { "UiNode", "LogLine" },
  writes = { "UiText", "UiNode", "Parent", "LogLine" },
  doc = "Writes the turn's messages at the bottom of the log under the HUD line, moving the turn before's up into a line of their own (the last 40 kept): the hero's hits and kills, potions dropped, picked up, and drunk (or none to drink), the monsters' hits, a death, the stairs, and bumps, each from the string tables (message.*, monster.*).",
  run = function(q)
    local said = {}
    local function name(kind)
      return q:text("monster." .. tostring(kind))
    end
    local killed = {}
    for _, ev in ipairs(q:events("death")) do
      if ev.cause == "hero" then killed[ev.kind] = true end
    end
    for _, ev in ipairs(q:events("damage")) do
      if ev.by == "@player" then
        if not killed[ev.kind] then
          table.insert(said, q:text("message.hit", { monster = name(ev.kind) }))
        end
      elseif ev.target == "@player" then
        table.insert(said, q:text("message.hurt", { monster = name(ev.kind), amount = ev.amount }))
      end
    end
    -- What the hero's own turn did comes first: a kill and what it left,
    -- or a potion picked up, drunk, or not there to drink.
    local first = {}
    for kind in pairs(killed) do
      table.insert(first, q:text("message.kill", { monster = name(kind) }))
    end
    for _, ev in ipairs(q:events("potion.dropped")) do
      table.insert(first, q:text("message.potion_dropped", { monster = name(ev.kind) }))
    end
    for _, ev in ipairs(q:events("potion.picked")) do
      table.insert(first, q:text("message.potion_picked", { count = ev.carried }))
    end
    for _, ev in ipairs(q:events("potion.drunk")) do
      if ev.healed > 0 then
        table.insert(first, q:text("message.potion_drunk", { amount = ev.healed, hp = ev.hp, max = ev.max }))
      else
        table.insert(first, q:text("message.potion_full", { hp = ev.hp, max = ev.max }))
      end
    end
    if #q:events("potion.none") > 0 then
      table.insert(first, q:text("message.no_potions"))
    end
    for i, line in ipairs(first) do
      table.insert(said, i, line)
    end
    for _, ev in ipairs(q:events("player.died")) do
      table.insert(said, q:text("message.died", { monster = name(ev.kind), floor = ev.floor, kills = ev.kills }))
    end
    if #q:events("floor.descend") > 0 then
      table.insert(said, q:text("message.descend"))
    end
    for _, ev in ipairs(q:events("bump")) do
      table.insert(said, q:text(ev.what == "no stairs here" and "message.no_stairs" or "message.wall"))
    end
    if #said > 0 then
      local newest = q:entity("@messages"):get("UiText")
      if newest.text ~= "" then
        -- The turn before's words move up into a line of their own.
        local lines = {}
        for e, _, node in q:each("LogLine", "UiNode") do
          -- By id: the loop's handle moves on to the next line.
          table.insert(lines, { id = e:id(), order = node.order })
        end
        table.sort(lines, function(a, b) return a.order < b.order end)
        -- Orders count up under @messages' 100000; near it, they start
        -- again from 1 (a run of 99,000 turns that each said something).
        if #lines > 0 and lines[#lines].order >= 99000 then
          for i, l in ipairs(lines) do
            q:entity(l.id):get("UiNode").order = i
            l.order = i
          end
        end
        local count = #lines
        local top = count > 0 and lines[#lines].order or 0
        local oldest = count > 0 and lines[1].id or nil
        local line = q:spawn({ "UiNode", "Parent", "UiText", "LogLine" })
        line:get("Parent").entity = q:entity("@log")
        line:get("UiNode").order = top + 1
        local t = line:get("UiText")
        t.text = newest.text
        t.size = 16
        if count + 1 > KEEP then q:entity(oldest):despawn() end
      end
      newest.text = table.concat(said, " ")
      q:entity("@log"):get("UiNode").visible = true
    end
  end,
}
