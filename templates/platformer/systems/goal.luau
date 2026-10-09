-- The run: touching the flag finishes it, and the restart action (R, or
-- Select on a pad) starts the level again from the beginning at any time.
--
-- Starting again puts back what the run changed: the hero at Hero.start,
-- the counts at zero, every coin back, the flag not reached. Something new
-- that a run changes (a key, a door, an enemy's health) is put back here
-- too.
return system "sim.goal" {
  stage = "sim",
  after = { "physics.overlap" },
  reads = { "Actions", "Hero", "Goal", "Coin" },
  writes = { "Hero", "Goal", "Coin", "Transform", "KinematicBody", "Camera" },
  doc = "Finishes the run when the hero touches the Goal (Goal.reached, Hero.done, goal.reached {coins, falls}); on the restart action starts the level again: the hero at Hero.start, the counts zeroed, the coins and the flag back (level.restarted).",
  run = function(q)
    local hero = q:entity("@player")
    local h = hero:get("Hero")

    if hero:get("Actions").restart_pressed then
      hero:get("Transform").pos = h.start
      hero:get("KinematicBody").velocity = vector.create(0, 0, 0)
      h.coins = 0
      h.falls = 0
      h.done = false
      for _, c in q:each("Coin") do
        c.taken = false
      end
      for _, g in q:each("Goal") do
        g.reached = false
      end
      q:entity("@camera"):get("Camera").tracking = false
      q:emit("level.restarted", {})
      -- This tick's touches happened where the hero was: not this run's.
      return
    end

    for _, ev in ipairs(q:events("overlap.begin")) do
      local other = nil
      if ev.entity == hero then
        other = ev.other
      elseif ev.other == hero then
        other = ev.entity
      end
      if other ~= nil and other:has("Goal") and not h.done then
        other:get("Goal").reached = true
        h.done = true
        q:emit("goal.reached", { coins = h.coins, falls = h.falls })
      end
    end
  end,
}
