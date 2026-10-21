-- Moving: each fighter walks by its move action at a steady speed, facing
-- the way it last moved, and dashes on dash: a short fast burst along its
-- facing (Fighter.dash_ticks), then a wait before the next one
-- (Fighter.dash_every). Only while the round is on (Match.state fight). A
-- knockback from a hit (Fighter.push) carries on through the pause after a
-- point, slowing. Both fighters are kept inside the arena's walls.
local SLOW = 0.86 -- the share of a knockback's speed kept each tick

return system "sim.move" {
  stage = "sim",
  after = { "sim.round" },
  reads = { "Actions", "Match", "Sprite" },
  writes = { "Fighter", "Transform" },
  doc = "Walks each fighter by move at Fighter.speed, facing the way it moves; dash starts a dash of Fighter.dash_ticks at Fighter.dash_speed along its facing (dash {by, at}), then waits Fighter.dash_every; a knockback (Fighter.push) slows each tick; kept inside the arena's rect (@arena's Sprite).",
  run = function(q)
    local arena = q:entity("@arena")
    local fight = arena:get("Match").state == "fight"
    local centre = arena:get("Transform").pos
    local room = arena:get("Sprite").size * 0.5
    local dt = q.dt
    for e, f, a, t, sp in q:each("Fighter", "Actions", "Transform", "Sprite") do
      local pos = t.pos
      if f.dash > 0 then
        f.dash -= 1
      end
      if f.dash_wait > 0 then
        f.dash_wait -= 1
      end

      if fight then
        local m = a.move
        local moving = m.x ~= 0 or m.y ~= 0
        if f.dash == 0 then
          if moving then
            f.facing = vector.normalize(m)
          end
          if a.dash_pressed and f.dash_wait == 0 then
            f.dash = f.dash_ticks
            f.dash_wait = f.dash_every
            q:emit("dash", { by = e, at = pos })
          end
        end
        if f.dash > 0 then
          pos += f.facing * (f.dash_speed * dt)
        elseif moving then
          pos += m * (f.speed * dt)
        end
      end

      -- A knockback, slowing to a stop.
      local push = f.push
      if push.x ~= 0 or push.y ~= 0 then
        pos += push * dt
        f.push = if vector.magnitude(push) < 0.5 then vector.create(0, 0, 0) else push * SLOW
      end

      -- The walls: the arena's half size, less the fighter's own half size.
      local half = room - sp.size * 0.5
      local x = math.clamp(pos.x, centre.x - half.x, centre.x + half.x)
      local y = math.clamp(pos.y, centre.y - half.y, centre.y + half.y)
      t.pos = vector.create(x, y, 0)
    end
  end,
}
