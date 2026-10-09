-- Coins: taken when the hero touches one, counted in Hero.coins, and hidden
-- until the level starts again (systems/goal.luau puts them back).
--
-- A coin is any entity with a Coin and a Collider (prefabs/coin.toml is
-- one; copy scenes/level/coin.1.toml to add another). A Collider with no
-- KinematicBody is not solid: the hero passes through it, and the engine
-- only reports the touch.
return system "sim.pickups" {
  stage = "sim",
  after = { "physics.overlap" },
  reads = { "Hero", "Coin" },
  writes = { "Hero", "Coin", "Sprite" },
  doc = "Takes a coin the hero touches (Coin.taken, Hero.coins, coin.taken {entity, coins}) and draws only the coins not taken.",
  run = function(q)
    local hero = q:entity("@player")
    local h = hero:get("Hero")
    for _, ev in ipairs(q:events("overlap.begin")) do
      local other = nil
      if ev.entity == hero then
        other = ev.other
      elseif ev.other == hero then
        other = ev.entity
      end
      if other ~= nil and other:has("Coin") and not other:get("Coin").taken then
        other:get("Coin").taken = true
        h.coins += 1
        q:emit("coin.taken", { entity = other, coins = h.coins })
      end
    end
    -- A coin is drawn while it is there to take.
    for _, c, s in q:each("Coin", "Sprite") do
      if s.visible == c.taken then
        s.visible = not c.taken
      end
    end
  end,
}
