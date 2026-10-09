-- Restart: after game over, the restart action (Enter, R, or the south
-- button) starts the run again from wave 1: enemies and bullets gone, the
-- hero whole in the middle, the score at zero.
--
-- If you add state to the run (a pickup, a power level), reset it here
-- too, or it carries over into the next run.
return system "sim.restart" {
  stage = "sim",
  reads = { "Actions", "Player", "Enemy", "Bullet" },
  writes = { "Player", "Health", "Transform", "Sprite", "Waves", "UiNode" },
  doc = "On the restart action after the hero has died: despawns every enemy and bullet, makes the hero whole at the arena's centre, and sets the waves back to before wave 1.",
  run = function(q)
    local pe = q:entity("@player")
    local pl = pe:get("Player")
    if not pl.dead or not pe:get("Actions").restart_pressed then return end

    local gone = {}
    for e in q:each("Enemy") do table.insert(gone, e:id()) end
    for e in q:each("Bullet") do table.insert(gone, e:id()) end
    for _, id in ipairs(gone) do q:entity(id):despawn() end

    local arena = q:entity("@arena")
    pl.dead = false
    pl.score = 0
    pl.cooldown = 0
    pl.safe = 0
    local hp = pe:get("Health")
    hp.cur = hp.max
    pe:get("Transform").pos = arena:get("Transform").pos
    pe:get("Sprite").visible = true

    local w = arena:get("Waves")
    w.wave = 0
    w.alive = 0
    w.timer = 60
    q:entity("@game_over"):get("UiNode").visible = false
    q:emit("game.restarted", {})
  end,
}
