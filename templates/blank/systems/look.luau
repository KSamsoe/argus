-- Turns the player to face the way they walk. player.png is drawn facing
-- right; this mirrors the sprite while they walk left. Presentation only:
-- it reads the move action and writes nothing but the sprite.
return system "late.look" {
  stage = "late",
  reads = { "Actions", "Player", "Sprite" },
  writes = { "Sprite" },
  doc = "Mirrors the player's sprite to face the way they walk.",
  run = function(q)
    for _, a, _, s in q:each("Actions", "Player", "Sprite") do
      if a.move.x ~= 0 then
        s.flip_x = a.move.x < 0
      end
    end
  end,
}
