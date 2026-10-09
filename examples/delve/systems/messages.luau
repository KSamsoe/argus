-- The message line (phase 3 item 4): what the turn did, in words from the
-- string tables. Delve is mostly state and text; its events said what
-- happened to the agent (the log) and, until now, never to the player.

return system "sim.messages" {
  stage = "sim",
  after = { "sim.monsters", "sim.potions" },
  -- A drink with no potion freezes the rest of the turn (no turn passes);
  -- the line still says so.
  unfrozen = true,
  writes = { "UiText" },
  doc = "Writes the turn's messages under the HUD line: the hero's hits and kills, potions dropped, picked up, and drunk (or none to drink), the monsters' hits, a death, the stairs, and bumps, each from the string tables (message.*, monster.*).",
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
      q:entity("@messages"):get("UiText").text = table.concat(said, " ")
    end
  end,
}
