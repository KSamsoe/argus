-- The feather's gift: a jump in mid-air (components/feather.luau). The
-- CharacterController (physics.control) jumps from the ground and in
-- coyote time; a press it cannot use there, in mid-air, it only remembers
-- for landing (its buffer). That press is this system's: while the hero
-- has a jump in mid-air left, it jumps them as a jump from the ground
-- would (the same take-off, held for a full one and cut when let go) and
-- forgets the press, so landing does not jump again. Landing gives the
-- jumps back. sim.run takes the feather and gives it back on a new run.
return system "sim.air_jump" {
  stage = "sim",
  -- After the controller has had this tick's press; the scheduler puts it
  -- before physics.move (there is no before key: script.systems shows it).
  after = { "physics.control" },
  reads = { "Actions", "AirJump", "CharacterController", "KinematicBody" },
  writes = { "AirJump", "CharacterController", "KinematicBody" },
  doc = "The feather's double jump: while the hero is in mid-air with a jump left (AirJump.left), a jump press the controller could only buffer jumps them instead, as high as a jump from the ground (AirJump.height, or the controller's jump_height), variable as it is, and emits hero.air_jump {entity, left}; on the ground AirJump.left is AirJump.max again.",
  run = function(q)
    local hero = q:entity("@player")
    if not hero:has("AirJump") then return end
    local air = hero:get("AirJump")
    local body = hero:get("KinematicBody")
    -- Standing (as the last move left them): every jump in mid-air is back.
    if body.grounded then
      if air.left ~= air.max then air.left = air.max end
      return
    end
    if air.left == 0 or not hero:get("Actions").jump_pressed then return end
    -- A press the controller used (a coyote jump) leaves no buffer; one it
    -- could only remember for landing does.
    local cc = hero:get("CharacterController")
    if cc.buffer <= 0 then return end
    -- The controller's rising gravity, from its jump's height and time to
    -- the top, and the take-off speed that rises the height against it.
    local gravity = 2 * cc.jump_height / (cc.jump_time * cc.jump_time)
    local height = air.height > 0 and air.height or cc.jump_height
    body.velocity = vector.create(body.velocity.x, math.sqrt(2 * gravity * height), 0)
    -- The controller set this tick's gravity for a fall (fall_gravity);
    -- the jump rises from this tick, as one from the ground does.
    body.gravity = gravity
    cc.rising = true
    cc.buffer = 0
    air.left -= 1
    q:emit("hero.air_jump", { entity = hero, left = air.left })
    q:play("sfx/flap")
  end,
}
