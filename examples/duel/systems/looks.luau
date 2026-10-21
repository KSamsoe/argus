-- Duel's looks and sounds. Presentation only: it reads gameplay (where the
-- fighters face, who is dashing or hit, the swings out, this tick's events)
-- and writes only how things are drawn and heard, so the game plays the
-- same without it. Each tick it:
-- - picks each fighter's frame: its own twelve (player 1's first, then
--   player 2's), the view the way it faces (mirrored to face left), its
--   legs stepping while it walks, white and blinking while it shows a hit,
--   and pale while it dashes;
-- - sweeps each swing's slash (three frames over its life), tinted in its
--   fighter's colour;
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
  reads = { "Fighter", "PlayerSlot", "Actions", "Match", "Swing", "Transform" },
  writes = { "Sprite", "Transform", "Emitter", "UiText" },
  doc = "Presentation only: picks each fighter's frame of sprites/actors (its view the way it faces, stepping while it walks, white and blinking while hit, pale while dashing), sweeps each swing's slash in its fighter's colour, colours the win screen's title, and plays the sounds and starts the effects of this tick's swing, dash, point, clash, and match.won events. Writes nothing gameplay reads.",
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

    for _, s, sp in q:each("Swing", "Sprite") do
      local owner = s.owner
      local life = owner:get("Fighter").swing_ticks
      local done = 1 - (s.ttl + 1) / life -- 0 as it starts, nearly 1 at its end
      set(sp, "frame", SLASH + math.min(2, math.floor(done * 3)))
      set(sp, "color", TINT[owner:get("PlayerSlot").slot] or "#FFFFFF")
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
