-- The way out. A mark from the phase 2 playtest said it plainly: "The level
-- needs a goal to reach that isn't death." Reaching the exit at the end of
-- the hall finishes the run and says how long it took and how often the
-- hero fell; R (or Start) plays it again from the cave mouth.
return system "sim.run" {
  stage = "sim",
  -- After the overlaps are known, so touching the exit is seen this tick.
  after = { "physics.overlap" },
  reads = { "Actions", "Run" },
  writes = { "Run", "Describe", "Transform", "KinematicBody" },
  doc = "Counts the hero's falls, finishes the run when the hero reaches the exit (run.finished {time, falls}), restarts it on the restart action, and writes the HUD line.",
  run = function(q)
    local hero = q:entity("@player")
    local exit = q:entity("@exit")
    local hud = q:entity("@hud")
    local run = hud:get("Run")
    if hero:get("Actions").restart_pressed then
      hero:get("Transform").pos = vector.create(3, 1.8, 0)
      hero:get("KinematicBody").velocity = vector.create(0, 0, 0)
      run.started = q.time
      run.falls = 0
      run.done = false
      run.time = 0
      q:emit("run.restarted", {})
    end
    if not run.done then
      run.falls += #q:events("player.fell")
      for _, ev in ipairs(q:events("overlap.begin")) do
        if (ev.entity == hero and ev.other == exit) or (ev.entity == exit and ev.other == hero) then
          run.done = true
          run.time = q.time - run.started
          q:emit("run.finished", { time = run.time, falls = run.falls })
          q:play("sfx/out")
        end
      end
    end
    local falls = run.falls == 0 and "no falls" or (run.falls == 1 and "1 fall" or (run.falls .. " falls"))
    if run.done then
      hud:get("Describe").text = string.format("Out of the hollow in %.1f s, %s. Press R to go again.", run.time, falls)
    else
      hud:get("Describe").text = string.format("Hollow. Find the way out. %s.", run.falls == 0 and "No falls yet" or (falls:sub(1, 1):upper() .. falls:sub(2)))
    end
  end,
}
