-- The dungeon's music: how tense it is, set each turn (a turn is a tick on
-- the discrete clock). The danger near the hero (monsters within a few
-- cells, more for each, and more as the hero's hit points fall) and the
-- floor's depth make the intensity; audio.toml's [music.dungeon] brings in
-- its high line, then its pulse, as it rises. Presentation only: it reads
-- the game and writes nothing but the music.
return system "late.music" {
  stage = "late",
  reads = { "Player", "Health", "Monster", "Transform" },
  writes = { "Music" },
  doc = "Sets Music.intensity from the monsters within five cells of the hero, the hero's hit points, and the floor's depth.",
  run = function(q)
    local pe = q:entity("@player")
    local hero = pe:get("Player")
    local m = q:entity("@turn"):get("Music")
    if hero.dead then
      m.intensity = 0
      return
    end
    local p = pe:get("Transform").pos
    local near = 0
    for _, _, tr in q:each("Monster", "Transform") do
      local d = math.abs(tr.pos.x - p.x) + math.abs(tr.pos.y - p.y)
      if d <= 5 then
        near += 1
      end
    end
    local hp = pe:get("Health")
    local hurt = 1 - hp.cur / math.max(1, hp.max)
    local depth = math.min(0.3, (math.max(1, hero.floor) - 1) * 0.1)
    m.intensity = math.clamp(depth + near * 0.3 + hurt * 0.4, 0, 1)
  end,
}
