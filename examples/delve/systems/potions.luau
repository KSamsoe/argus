-- Healing potions: a monster the hero kills sometimes leaves one where it
-- fell, rolled on the run's "loot" stream (one roll a death, so a seed
-- always gives the same drops, and the floor's own stream is untouched);
-- the hero picks up every potion on the cell it walks onto. Drinking one is
-- the hero's command (systems/player_turn.luau).

return system "sim.potions" {
  stage = "sim",
  after = { "sim.player_turn" },
  reads = { "Potion", "Transform", "Player", "Turn" },
  writes = { "Potion", "Transform", "Sprite", "Describe", "Player" },
  rng = "loot",
  doc = "Drops a potion where a monster the hero killed this turn fell, at its Monster.drop chance in a hundred (the loot stream), and picks up every potion on the hero's cell into Player.potions. Emits potion.dropped and potion.picked.",
  run = function(q)
    local pe = q:entity("@player")
    local pl = pe:get("Player")
    local turn = q:entity("@turn"):get("Turn").n
    local rng = q:rng()

    for _, ev in ipairs(q:events("death")) do
      if ev.cause == "hero" then
        local roll = rng:range(1, 100)
        if roll <= ev.drop then
          local p = q:spawn({ "Potion", "Transform", "Sprite", "Describe" })
          p:get("Describe").name = "potion"
          p:get("Transform").pos = vector.create(ev.x, ev.y, 0)
          local sp = p:get("Sprite")
          sp.kind = "rect"
          sp.size = vector.create(0.4, 0.55, 0)
          sp.color = "#E0405A"
          sp.z = 3
          q:emit("potion.dropped", { entity = p, kind = ev.kind, x = ev.x, y = ev.y, roll = roll, turn = turn })
        end
      end
    end

    if pl.dead then return end
    local pos = pe:get("Transform").pos
    local here = {}
    for e, _, tr in q:each("Potion", "Transform") do
      if tr.pos.x == pos.x and tr.pos.y == pos.y then table.insert(here, e:id()) end
    end
    for _, id in ipairs(here) do
      q:entity(id):despawn()
      pl.potions += 1
      q:emit("potion.picked", { x = pos.x, y = pos.y, carried = pl.potions, turn = turn })
    end
  end,
}
