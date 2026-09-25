-- A run of the hollow, from the phase 2 playtest's marks. "The level needs
-- a goal to reach that isn't death": reaching the exit at the end of the
-- hall finishes the run and says how long it took and how often the hero
-- fell; R (or Start) plays it again from the cave mouth. "Player is still
-- movable after beating the level, can even still fall off": through the
-- door, the hero is held there out of sight until R. "Put something up here
-- like a collectible": a gem, taken on touch and counted.
return system "sim.run" {
  stage = "sim",
  -- After the overlaps are known, so a touch is seen on its tick.
  after = { "physics.overlap" },
  reads = { "Actions", "Run", "Gem" },
  writes = { "Run", "Gem", "UiText", "Transform", "KinematicBody", "Sprite" },
  doc = "Counts the hero's falls and gems, finishes the run at the exit (run.finished {time, falls, gems}) and holds the hero there out of sight, restarts it on the restart action (or the pause menu's), and writes the HUD line.",
  run = function(q)
    local hero = q:entity("@player")
    local exit = q:entity("@exit")
    local hud = q:entity("@hud")
    local run = hud:get("Run")
    if hero:get("Actions").restart_pressed or run.restart then
      run.restart = false
      hero:get("Transform").pos = vector.create(3, 1.8, 0)
      hero:get("KinematicBody").velocity = vector.create(0, 0, 0)
      hero:get("Sprite").visible = true
      run.started = q.time
      run.falls = 0
      run.gems = 0
      run.done = false
      run.time = 0
      for _, g in q:each("Gem") do
        g.taken = false
      end
      q:emit("run.restarted", {})
    end
    if not run.done then
      run.falls += #q:events("player.fell")
      for _, ev in ipairs(q:events("overlap.begin")) do
        local other = nil
        if ev.entity == hero then
          other = ev.other
        elseif ev.other == hero then
          other = ev.entity
        end
        if other == exit then
          run.done = true
          run.time = q.time - run.started
          q:emit("run.finished", { time = run.time, falls = run.falls, gems = run.gems })
          q:play("sfx/out")
        elseif other ~= nil and other:has("Gem") then
          local g = other:get("Gem")
          if g.active and not g.taken then
            g.taken = true
            run.gems += 1
            q:emit("gem.taken", { entity = other })
            q:play("sfx/gem")
          end
        end
      end
    end
    -- Through the door: held in the doorway, out of sight, until R.
    if run.done then
      local door = exit:get("Transform").pos
      hero:get("Transform").pos = vector.create(door.x, 1.8, 0)
      hero:get("KinematicBody").velocity = vector.create(0, 0, 0)
      hero:get("Sprite").visible = false
    end
    -- A gem that is off, or taken, is not drawn.
    local gems = 0
    for e, g in q:each("Gem") do
      if g.active then
        gems += 1
      end
      e:get("Sprite").visible = g.active and not g.taken
    end
    local falls = run.falls == 0 and "no falls" or (run.falls == 1 and "1 fall" or (run.falls .. " falls"))
    local d = hud:get("UiText")
    if run.done then
      local found = ""
      if gems > 0 then
        found = run.gems > 0 and ", with the gem" or ", gem missed"
      end
      d.text = string.format("Out of the hollow in %.1f s, %s%s. Press R to go again.", run.time, falls, found)
    else
      local f = run.falls == 0 and "No falls yet" or (falls:sub(1, 1):upper() .. falls:sub(2))
      d.text = string.format("Hollow. Find the way out. %s.%s", f, run.gems > 0 and " Gem found." or "")
    end
  end,
}
