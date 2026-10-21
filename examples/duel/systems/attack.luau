-- Swinging: attack swings a hitbox (prefabs/swing.toml) out in front of the
-- fighter for Fighter.swing_ticks, following it, then a wait
-- (Fighter.swing_every) before the next; not while it dashes. A swing that
-- touches the other fighter, unless that one is dashing, is a point: the
-- one hit is knocked back and flashes, and the round stands still
-- (Match.state point) until sim.round starts the next. Two swings that land
-- on the same tick are a clash: both knocked back, no point.
--
-- A touch is the distance between the swing's middle and the other's
-- against the swing's radius and half a body (BODY): no colliders. Every
-- swing, point, and clash is an event (swing, point, clash): the tests
-- count them, and systems/looks.luau plays their sounds and sparks.
local BODY = 0.4 -- a fighter's body: a circle this far round its middle
local FLASH = 30 -- ticks one hit shows it

return system "sim.attack" {
  stage = "sim",
  after = { "sim.move" },
  reads = { "Actions", "PlayerSlot" },
  writes = { "Fighter", "Swing", "Transform", "Match" },
  spawns = { "swing" },
  doc = "The attack action swings a Swing (prefabs/swing) at Fighter.reach in front, for Fighter.swing_ticks, then waits Fighter.swing_every (swing {by}); one touching the other fighter within Fighter.radius plus half a body, unless it dashes, scores a point for its slot (point {by, at, one, two}), knocks the one hit back at Fighter.knockback, flashes it, and stands the round still (Match.state point); two on one tick are a clash {at}, no point.",
  run = function(q)
    local m = q:entity("@arena"):get("Match")

    -- The swings out follow their fighters, and go when their time is up.
    local gone = {}
    for e, s, t in q:each("Swing", "Transform") do
      if s.ttl == 0 then
        table.insert(gone, e:id())
      else
        s.ttl -= 1
        t.pos = s.owner:get("Transform").pos + s.dir * s.owner:get("Fighter").reach
      end
    end
    for _, id in gone do
      q:entity(id):despawn()
    end

    -- The waits count down; where each fighter is now (sim.move has moved
    -- it), and whether it is dashing.
    local fighters = {}
    local swings = {}
    for e, f, a, t in q:each("Fighter", "Actions", "Transform") do
      if f.swing_wait > 0 then
        f.swing_wait -= 1
      end
      if f.flash > 0 then
        f.flash -= 1
      end
      table.insert(fighters, { id = e:id(), pos = t.pos, dashing = f.dash > 0 })
      if m.state == "fight" and a.attack_pressed and f.swing_wait == 0 and f.dash == 0 then
        f.swing_wait = f.swing_every
        table.insert(swings, { id = e:id(), at = t.pos + f.facing * f.reach, dir = f.facing, ttl = f.swing_ticks - 1 })
      end
    end
    if m.state ~= "fight" then
      return
    end

    -- New swings, spawned after the loop over what they are made of.
    for _, s in swings do
      local by = q:entity(s.id)
      q:spawn_prefab("swing", {
        ["Transform.pos"] = s.at,
        ["Transform.rot"] = math.atan2(s.dir.y, s.dir.x),
        ["Swing.owner"] = by,
        ["Swing.dir"] = s.dir,
        ["Swing.ttl"] = s.ttl,
      })
      q:emit("swing", { by = by })
    end

    -- What each swing out touches.
    local hits = {}
    for _, s, t in q:each("Swing", "Transform") do
      local owner = s.owner
      local reach = owner:get("Fighter").radius + BODY
      for _, f in fighters do
        if f.id ~= owner:id() and not f.dashing and vector.magnitude(f.pos - t.pos) <= reach then
          table.insert(hits, { by = owner:id(), target = f.id, dir = s.dir, at = f.pos })
        end
      end
    end
    if #hits == 0 then
      return
    end

    -- Each one hit is knocked back the way the swing went, and flashes.
    for _, h in hits do
      local f = q:entity(h.target):get("Fighter")
      f.push = h.dir * q:entity(h.by):get("Fighter").knockback
      f.flash = FLASH
    end
    local spent = {}
    for e in q:each("Swing") do
      table.insert(spent, e:id())
    end
    for _, id in spent do
      q:entity(id):despawn()
    end

    m.state = "point"
    m.wait = m.pause
    if #hits == 1 then
      local slot = q:entity(hits[1].by):get("PlayerSlot").slot
      if slot == 1 then
        m.one += 1
      else
        m.two += 1
      end
      q:emit("point", { by = slot, at = hits[1].at, one = m.one, two = m.two })
    else
      q:emit("clash", { at = (hits[1].at + hits[2].at) * 0.5 })
    end
  end,
}
