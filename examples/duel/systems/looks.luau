-- Duel's looks and sounds. Presentation only: it reads gameplay (where the
-- fighters face, who is dashing or hit, the swings out, this tick's events)
-- and writes only how things are drawn and heard, so the game plays the
-- same without it. Each tick it:
-- - picks each fighter's frame: its own twelve (player 1's first, then
--   player 2's), the view the way it faces (mirrored to face left), its
--   legs stepping while it walks, white and blinking while it shows a hit,
--   and pale while it dashes;
-- - draws each swing's slash (prefabs/slash.toml, spawned on its swing
--   event), tinted in its fighter's colour: three frames sweeping in front
--   of the fighter while the swing's hitbox is out, then the last, staying
--   where it was and fading, for SLASH_FADE ticks more. Online, the other
--   player's swing can be learned of several ticks late, after a rollback,
--   when most of its hitbox's few ticks are over; the fade keeps it in
--   sight. It is the same on every machine, as everything in the world
--   must be;
-- - colours the win screen's title in the winner's colour;
-- - plays the sounds (audio.toml) and starts the effects (assets/fx/) of
--   this tick's swing, dash, point, clash, and match.won events.
--
-- The pictures are frames of sprites/actors, numbered as the list at the
-- end of assets/sprites/actors.art.luau has them.

local PER_FIGHTER = 12 -- each fighter's frames: player 1's from 0, player 2's from 12
local SIDE, FRONT, BACK = 0, 3, 6 -- each view standing, then its two steps
local HIT = 9 -- each view hit: side, front, back
local SLASH = 24 -- the slash, three frames as it sweeps
local SLASH_FADE = 16 -- ticks a slash stays, fading, after its swing's hitbox: more than a match rolls back (8), less than the wait between swings (30)
local STEP = 0.1 -- seconds each frame of a walk shows
local TINT = { "#9CD0FF", "#FFA898" } -- each slot's slash: its colour, paler
local PALE = "#FFFFFF5A" -- a fighter while it dashes: see-through
local TITLE = { "one", "two" } -- the win screen's title: the winner's colour (ui.toml)

-- Writes a field only when it changes.
local function set(c, field, v)
  if c[field] ~= v then
    c[field] = v
  end
end

-- A slash's look at its age: sweeping in front of its fighter while the
-- hitbox is out (Fighter.swing_ticks), then its last frame where it was,
-- fading. False once it is done.
local function draw_slash(sl, t, sp)
  local owner = sl.owner
  local f = owner:get("Fighter")
  local life = f.swing_ticks
  local tint = TINT[owner:get("PlayerSlot").slot] or "#FFFFFF"
  if sl.age < life then
    t.pos = owner:get("Transform").pos + sl.dir * f.reach
    set(sp, "frame", SLASH + math.min(2, math.floor(sl.age / life * 3)))
    set(sp, "color", tint)
    return true
  end
  local left = life + SLASH_FADE - sl.age -- ticks it has still to fade
  if left <= 0 then
    return false
  end
  set(sp, "frame", SLASH + 2)
  set(sp, "color", tint .. string.format("%02X", math.floor(255 * left / (SLASH_FADE + 1))))
  return true
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
  reads = { "Fighter", "PlayerSlot", "Actions", "Match", "Transform" },
  writes = { "Sprite", "Transform", "Emitter", "UiText", "Slash" },
  spawns = { "slash" },
  doc = "Presentation only: picks each fighter's frame of sprites/actors (its view the way it faces, stepping while it walks, white and blinking while hit, pale while dashing), draws each swing's slash (prefabs/slash, spawned on its swing event) in its fighter's colour, sweeping while the hitbox is out and then fading where it was for 16 ticks more, so a swing learned of late online still shows, colours the win screen's title, and plays the sounds and starts the effects of this tick's swing, dash, point, clash, and match.won events. Writes nothing gameplay reads.",
  run = function(q)
    local m = q:entity("@arena"):get("Match")

    for _, f, slot, a, sp in q:each("Fighter", "PlayerSlot", "Actions", "Sprite") do
      local face = f.facing
      local view = if math.abs(face.x) >= math.abs(face.y) then SIDE elseif face.y > 0 then BACK else FRONT
      local frame = view
      local walking = m.state == "fight" and f.dash == 0 and (a.move.x ~= 0 or a.move.y ~= 0)
      if walking then
        -- A step, standing, the other step, standing.
        local phase = math.floor(q.time / STEP) % 4
        if phase == 0 then
          frame = view + 1
        elseif phase == 2 then
          frame = view + 2
        end
      end
      if f.flash > 0 and (f.flash // 4) % 2 == 0 then
        frame = HIT + view // 3
      end
      set(sp, "frame", (slot.slot - 1) * PER_FIGHTER + frame)
      if face.x ~= 0 then
        set(sp, "flip_x", face.x < 0)
      end
      set(sp, "color", if f.dash > 0 then PALE else "#FFFFFF")
    end

    -- The slashes out age, and go when they have faded, or when a round
    -- or a match begins afresh; this tick's swings start theirs.
    local fresh = #q:events("round.started") > 0 or #q:events("match.started") > 0
    local done = {}
    for e, sl, t, sp in q:each("Slash", "Transform", "Sprite") do
      sl.age += 1
      if fresh or not draw_slash(sl, t, sp) then
        table.insert(done, e:id())
      end
    end
    for _, id in done do
      q:entity(id):despawn()
    end
    for _, ev in ipairs(q:events("swing")) do
      local f = ev.by:get("Fighter")
      local slash = q:spawn_prefab("slash", {
        ["Transform.rot"] = math.atan2(f.facing.y, f.facing.x),
        ["Slash.owner"] = ev.by,
        ["Slash.dir"] = f.facing,
      })
      draw_slash(slash:get("Slash"), slash:get("Transform"), slash:get("Sprite"))
    end

    if m.winner > 0 then
      set(q:entity("@win/panel/title"):get("UiText"), "color", TITLE[m.winner] or "accent")
    end

    -- The sounds, and the effects, of what happened this tick.
    if #q:events("swing") > 0 then
      q:play("sfx/swing")
    end
    for _, ev in ipairs(q:events("dash")) do
      q:play("sfx/dash")
      burst(q, "fx/puff", ev.at)
    end
    for _, ev in ipairs(q:events("point")) do
      q:play("sfx/hit")
      q:play("sfx/point")
      burst(q, "fx/spark", ev.at)
    end
    for _, ev in ipairs(q:events("clash")) do
      q:play("sfx/hit")
      burst(q, "fx/spark", ev.at)
    end
    if #q:events("match.won") > 0 then
      q:play("sfx/win")
    end
  end,
}
