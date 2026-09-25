-- The message line (phase 3 item 4): what the turn did, in words from the
-- string tables. Delve is mostly state and text; its events said what
-- happened to the agent (the log) and, until now, never to the player.

return system "sim.messages" {
  stage = "sim",
  after = { "sim.monsters" },
  writes = { "UiText" },
  doc = "Writes the turn's messages under the HUD line: the hero's hits and kills, the monsters' hits, a death, the stairs, and bumps, each from the string tables (message.*, monster.*).",
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
    for kind in pairs(killed) do
      table.insert(said, 1, q:text("message.kill", { monster = name(kind) }))
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
