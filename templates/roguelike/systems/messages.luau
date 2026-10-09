-- The message line: what this turn did, in words. Systems before this one
-- emitted events (q:emit); q:events() hands this one all of this turn's,
-- oldest first, so the words come in the order things happened. To tell
-- the player about something new, emit an event for it and add a branch.

return system "sim.messages" {
  stage = "sim",
  after = { "sim.monsters" },
  -- A bump or a restart freezes the rest of the turn; this runs anyway, to
  -- say so.
  unfrozen = true,
  writes = { "UiText" },
  doc = "Writes the turn's messages on @messages: the hero's hits and kills, the monsters' bites, the hero's death, a bump into rock, and a new run. A turn that says nothing leaves the last line.",
  run = function(q)
    local said = {}
    for _, ev in ipairs(q:events()) do
      if ev.name == "run.new" then
        table.insert(said, "A new run begins.")
      elseif ev.name == "bump" then
        table.insert(said, "A wall.")
      elseif ev.name == "damage" and ev.by == "@player" then
        if ev.left > 0 then
          table.insert(said, "You hit the " .. ev.kind .. ".")
        end
      elseif ev.name == "death" then
        table.insert(said, "You kill the " .. ev.kind .. ".")
      elseif ev.name == "damage" then
        table.insert(said, "The " .. ev.kind .. " bites you for " .. ev.amount .. ".")
      elseif ev.name == "player.died" then
        table.insert(said, "The " .. ev.kind .. " kills you. Press R for a new run.")
      end
    end
    if #said > 0 then
      q:entity("@messages"):get("UiText").text = table.concat(said, " ")
    end
  end,
}
