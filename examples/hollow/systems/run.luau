-- A run of the hollow, from the phase 2 playtest's marks. "The level needs
-- a goal to reach that isn't death": reaching the exit at the end of the
-- hall finishes the run and says how long it took and how often the hero
-- fell; R (or Start) plays it again from the cave mouth. "Player is still
-- movable after beating the level, can even still fall off": through the
-- door, the hero is held there out of sight until R. "Put something up here
-- like a collectible": a gem, taken on touch and counted. Finishing plays
-- the outro timeline (defs/timeline.toml, phase 3 item 5); R skips it.
-- The feather in the cave is the run's too: taken on touch, the hero has a
-- jump in mid-air for the rest of it (AirJump, which sim.air_jump spends),
-- and a new run takes the jump away and puts the feather back.
return system "sim.run" {
  stage = "sim",
  -- After the overlaps are known, so a touch is seen on its tick.
  after = { "physics.overlap" },
  reads = { "Actions", "Run", "Gem", "InputGlyph", "Feather", "AirJump" },
  writes = { "Run", "Gem", "UiText", "Transform", "KinematicBody", "Sprite", "Timeline", "Feather", "AirJump" },
  doc = "Counts the hero's falls and gems, gives the hero the feather's jumps in mid-air when they touch it (feather.taken {entity, jumps}) for the rest of the run, finishes the run at the exit (run.finished {time, falls, gems}), plays the outro timeline and holds the hero there out of sight, restarts it on the restart action (or the pause menu's, skipping the outro; the feather goes back), and writes the HUD line.",
  run = function(q)
    local hero = q:entity("@player")
    local hud = q:entity("@hud")
    local run = hud:get("Run")
    local air = hero:has("AirJump") and hero:get("AirJump") or nil
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
      run.number += 1
      for _, g in q:each("Gem") do
        g.taken = false
      end
      -- The feather's gift is this run's: gone, and the feather back.
      if air ~= nil then
        air.max = 0
        air.left = 0
      end
      -- A new run skips what is left of the outro.
      for _, tl in q:each("Timeline") do
        tl.skip = true
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
        -- The exit is in the hall, which streams out when the hero is two
        -- rooms from it (phase 3 item 6): known by its id when touched,
        -- never looked up while it may not be there.
        if other ~= nil and other:name() == "c/exit" then
          run.done = true
          run.time = q.time - run.started
          q:emit("run.finished", { time = run.time, falls = run.falls, gems = run.gems })
          q:play("sfx/out")
          local outro = q:spawn({ "Timeline" }):get("Timeline")
          outro.name = "timeline:outro"
          outro.actor = hero
        elseif other ~= nil and other:has("Gem") then
          local g = other:get("Gem")
          if g.active and not g.taken then
            g.taken = true
            g.taken_in = run.number
            run.gems += 1
            q:emit("gem.taken", { entity = other })
            q:play("sfx/gem")
          end
        elseif other ~= nil and other:has("Feather") and air ~= nil then
          local f = other:get("Feather")
          if not f.taken and f.jumps > air.max then
            air.max = f.jumps
            -- Usable at once, in mid-air too (the touch may be a jump's).
            air.left = f.jumps
            q:emit("feather.taken", { entity = other, jumps = f.jumps })
            q:play("sfx/feather")
          end
        end
      end
    end
    -- A feather is taken while the hero has its jumps: the hero's AirJump
    -- is what counts, so one whose room streamed back in is as the run is.
    local winged = air ~= nil and air.max > 0
    for e, f in q:each("Feather") do
      local taken = winged and air.max >= f.jumps
      if f.taken ~= taken then f.taken = taken end
      local sprite = e:get("Sprite")
      if sprite.visible == taken then sprite.visible = not taken end
    end
    -- Through the door: held in the doorway, out of sight, until R.
    if run.done then
      local door = q:entity("@exit"):get("Transform").pos
      hero:get("Transform").pos = vector.create(door.x, 1.8, 0)
      hero:get("KinematicBody").velocity = vector.create(0, 0, 0)
      hero:get("Sprite").visible = false
    end
    -- A gem that is off, or taken, is not drawn. One taken in an earlier
    -- run is back: a restart puts back only the gems whose rooms are in,
    -- and a room streamed out remembers its gem as taken (found at the
    -- phase 5 gate by the agent who added the feather).
    local gems = 0
    for e, g in q:each("Gem") do
      if g.taken and g.taken_in ~= run.number then
        g.taken = false
      end
      if g.active then
        gems += 1
      end
      e:get("Sprite").visible = g.active and not g.taken
    end
    -- The line's words from the string tables (phase 3 item 4).
    local falls = run.falls == 0 and q:text("hud.no_falls")
      or (run.falls == 1 and q:text("hud.one_fall") or q:text("hud.falls", { n = run.falls }))
    local d = hud:get("UiText")
    if run.done then
      local found = ""
      if gems > 0 then
        found = q:text(run.gems > 0 and "hud.with_gem" or "hud.gem_missed")
      end
      -- What restart is pressed with, on the device in use (phase 5 item 9).
      local again = hud:get("InputGlyph").label
      if again == "" then
        again = "R"
      end
      d.text = q:text("hud.done", { time = string.format("%.1f", run.time), falls = falls, gem = found, again = again })
    else
      local f = run.falls == 0 and q:text("hud.no_falls_yet") or (falls:sub(1, 1):upper() .. falls:sub(2))
      d.text = q:text("hud.running", {
        falls = f,
        gem = run.gems > 0 and q:text("hud.gem_found") or "",
        feather = winged and q:text("hud.feather") or "",
      })
    end
  end,
}
