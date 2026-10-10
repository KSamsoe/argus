-- Arena's looks: which picture everything is drawn with. Presentation
-- only: it reads gameplay (where things are, who is hurt) and writes only
-- how things are drawn, so the game plays the same with or without it.
--
-- Gameplay spawns the enemies and bullets as plain shapes and colours them
-- (sim.waves orange, sim.hits paler once hit, sim.fire yellow). Each tick
-- this system:
-- - gives each new enemy its picture (keeping gameplay's colour as the
--   tint: the picture is grey), drawn at its own colour in the dark
--   (fx/body), and a shadow; walks it (two frames, quicker for a quicker
--   enemy), turns it to face the hero, and cracks it once it has been hit;
-- - gives each new bullet a glow, drawn added (fx/glow);
-- - turns the hero toward what it fires at, else the way it walks, steps
--   its legs while it walks, flashes it white when it is hit, tints it
--   red for the rest of its grace, and lays it down (under the enemies,
--   its light out) when it falls;
-- - throws sparks where a bullet hits (fx/spark), and a puff of dust where
--   an enemy was when it died (fx/puff).
--
-- The pictures are frames of sprites/actors, numbered as tools/art.py
-- lists them (sprites/actors.toml says which is which). To change a look,
-- change a number below, or a picture in tools/art.py.

local ATLAS = "sprites/actors"
local BODY = "fx/body" -- drawn at its own colour, however dark the room
local GLOW = "fx/glow" -- added to what is under it: a glow
local SPARK, PUFF = "fx/spark", "fx/puff"

-- Frames of sprites/actors.
local SIDE, FRONT, BACK = 0, 3, 6 -- the hero's views: standing, then two steps
local HIT = 9 -- the hero hit: side, front, back
local FALLEN = 12
local ENEMY, ENEMY_HIT = 13, 15 -- two steps each
local BOLT, SHADOW = 17, 20

-- Sizes in world units: the art is 12 pixels a unit.
local PX = 1 / 12
local ENEMY_SIZE = vector.create(12 * PX, 12 * PX, 0)
local BOLT_SIZE = vector.create(5 * PX, 5 * PX, 0)
local SHADOW_SIZE = vector.create(10 * PX, 4 * PX, 0)
local UNDER = vector.create(0, -5 * PX, 0) -- a shadow's place, under the feet

local STEP = 0.1 -- seconds each frame of the hero's walk shows
local ENEMY_STRIDE = 1.6 -- steps an enemy takes per unit it walks
local FLASH = 8 -- ticks the hero shows white when it is hit
local HURT = "#FF9A9A" -- the hero's tint for the rest of its grace
local HERO_Z, FALLEN_Z = 2, 0.9 -- over the enemies (z 1), and under them once fallen

-- Writes a field only when it changes.
local function set(c, field, v)
  if c[field] ~= v then
    c[field] = v
  end
end

-- The view (SIDE, FRONT, or BACK) a frame of the hero shows.
local function view_of(frame)
  if frame >= FALLEN then
    return SIDE
  elseif frame >= HIT then
    return (frame - HIT) * 3
  end
  return frame - frame % 3
end

local function hero(q, pe)
  local pl = pe:get("Player")
  local sp = pe:get("Sprite")
  local pos = pe:get("Transform").pos
  local move = pe:get("Actions").move
  local walking = (move.x ~= 0 or move.y ~= 0) and not pl.dead

  -- Where it looks: at what it fires at (the nearest enemy in range, as
  -- sim.fire picks it), else the way it walks, else where it last looked.
  local look = nil
  local target = if pl.range > 0 then q:nearest(pos, pl.range, "Enemy") else nil
  if target ~= nil then
    look = target:get("Transform").pos - pos
  elseif walking then
    look = move
  end
  local view, flip = view_of(sp.frame), sp.flip_x
  if look ~= nil then
    if math.abs(look.x) >= math.abs(look.y) then
      view = SIDE
    elseif look.y > 0 then
      view = BACK
    else
      view = FRONT
    end
    if look.x ~= 0 then
      flip = look.x < 0
    end
  end

  -- A step, standing, the other step, standing, while it walks.
  local frame = view
  if walking then
    local phase = math.floor(q.time / STEP) % 4
    if phase == 0 then
      frame = view + 1
    elseif phase == 2 then
      frame = view + 2
    end
  end

  local color = "#FFFFFF"
  if pl.dead then
    -- Fallen: under the enemies that got it, its light out.
    frame, flip = FALLEN, false
  elseif pl.safe > 0 then
    -- sim.hits blinks it (Sprite.visible) while it is safe.
    if pl.safe > pl.grace - FLASH then
      frame = HIT + view // 3
    else
      color = HURT
    end
  end
  set(sp, "frame", frame)
  set(sp, "flip_x", flip)
  set(sp, "color", color)
  set(sp, "z", if pl.dead then FALLEN_Z else HERO_Z)
  set(pe:get("Light"), "on", not pl.dead)
  return pos, pl.dead
end

-- Starts a one-shot effect at pos: it bursts, and its entity goes when the
-- last particle does.
local function burst(q, effect, pos)
  local fx = q:spawn({ "Transform", "Emitter" })
  fx:get("Transform").pos = pos
  local em = fx:get("Emitter")
  em.effect = effect
  em.auto_despawn = true
end

return system "late.looks" {
  stage = "late",
  reads = { "Player", "Actions", "Enemy", "Health", "Bullet", "Transform", "Sprite", "Parent", "GlobalTransform", "Light" },
  writes = { "Sprite", "Transform", "Parent", "GlobalTransform", "Emitter", "Light" },
  doc = "Presentation only: picks the hero's frame (facing what it fires at or where it walks, stepping, white when hit, fallen when dead, its light out then), gives new enemies and bullets their sprites of sprites/actors (keeping gameplay's colours as the tint) and each enemy a shadow, walks the enemies and turns them to the hero, and starts sparks where bullets hit and a puff where enemies die. Writes nothing gameplay reads.",
  run = function(q)
    local hero_pos, dead = hero(q, q:entity("@player"))

    -- The enemies: dressed the tick they come, then walked.
    local fresh = {}
    for e, en, hp, t, sp in q:each("Enemy", "Health", "Transform", "Sprite") do
      if sp.atlas ~= ATLAS then
        sp.atlas = ATLAS
        sp.size = ENEMY_SIZE
        sp.material = BODY
        table.insert(fresh, e:id())
      end
      local step = if dead then 0 else math.floor(q.time * en.speed * ENEMY_STRIDE) % 2
      set(sp, "frame", (if hp.cur < hp.max then ENEMY_HIT else ENEMY) + step)
      set(sp, "flip_x", t.pos.x > hero_pos.x)
    end
    -- A shadow under each new one: a child, so it moves with it.
    for _, id in ipairs(fresh) do
      local s = q:spawn({ "Transform", "GlobalTransform", "Parent", "Sprite" })
      s:get("Parent").entity = q:entity(id)
      s:get("Transform").pos = UNDER
      local sp = s:get("Sprite")
      sp.atlas = ATLAS
      sp.frame = SHADOW
      sp.size = SHADOW_SIZE
      sp.z = 0.5
    end

    -- The bullets: a glow, the tick they are fired.
    for _, _, sp in q:each("Bullet", "Sprite") do
      if sp.atlas ~= ATLAS then
        sp.atlas = ATLAS
        sp.frame = BOLT
        sp.size = BOLT_SIZE
        sp.material = GLOW
      end
    end

    -- Sparks where a hit landed on an enemy still standing.
    local bursts = {}
    for _, ev in ipairs(q:events("enemy.hit")) do
      if ev.entity:alive() then
        table.insert(bursts, { SPARK, ev.entity:get("Transform").pos })
      end
    end
    -- A puff where each enemy that went this tick was. sim.hits (or
    -- sim.restart) despawned it, so its shadow has lost its parent; the
    -- shadow still knows where it was drawn (GlobalTransform), until the
    -- engine moves it next tick.
    if #q:events("enemy.killed") > 0 or #q:events("game.restarted") > 0 then
      local gone = {}
      for e, parent, g, sp in q:each("Parent", "GlobalTransform", "Sprite") do
        local p = parent.entity
        if sp.frame == SHADOW and (p == nil or not p:alive()) then
          table.insert(bursts, { PUFF, g.pos - UNDER })
          table.insert(gone, e:id())
        end
      end
      for _, id in ipairs(gone) do
        q:entity(id):despawn()
      end
    end
    for _, b in ipairs(bursts) do
      burst(q, b[1], b[2])
    end
  end,
}
