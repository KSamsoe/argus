-- Swarm's looks: what everything is drawn with. Presentation only: gameplay
-- spawns the swarm and the bullets as described shapes and colours them
-- (sim.waves, sim.fire, sim.dash_look); this gives each a sprite of the one
-- atlas (sprites/swarm, drawn by tools/art.py) the tick it appears, keeps
-- the colour gameplay gave it as the sprite's tint, and writes nothing a
-- gameplay system or the plugin reads. Per tick it costs a look at the
-- bullets (a hundred or so) and one floor tile; the swarm is dressed only
-- on the tick a wave spawns, however large.
--
-- - The floor: 14 x 7 tiles of 12 units (sprites/floor) under the camera.
--   Each shows its world cell's tile (a hash of the cell picks one of eight,
--   and mirrors it), so the deck stands still as the hero walks over it;
--   they move only when the hero crosses a cell's edge.
-- - The swarm: mites (three faces) and dashers (a diamond), unlit and drawn
--   at their own colour under the hero's light (fx/body). A dasher's frame
--   follows its phase on the tick sim.dash says it began: flared winding up
--   and charging, hollow while spent.
-- - The bullets: a glow, added (fx/glow), a little larger than the shape.
-- - The hero's sparks: a burst of red when he is hurt, and a big one of his
--   own colour when he dies (prefabs/hero_fx.toml).

local ATLAS = "sprites/swarm"
local BODY = "fx/body"
local GLOW = "fx/glow"
-- Frames of sprites/swarm (tools/art.py writes them in this order).
local MITE = 13 -- 13 to 15
local DART, DART_FLARED, DART_SPENT = 16, 17, 18
local BOLT = 19
local BOLT_SIZE = vector.create(5 / 9, 5 / 9, 0) -- its 5 pixels at 9 a unit

local FLOOR = "sprites/floor"
local TILE = 12 -- units a tile
local COLS, ROWS = 14, 7 -- enough for the 60-unit view, its shake, and a wide window
local VARIANTS = 8
local FLOOR_Z = -10

local PHASES = {
  { "dash.windup", DART_FLARED },
  { "dash.charge", DART_FLARED },
  { "dash.recover", DART_SPENT },
  { "dash.recovered", DART },
}

-- The world cell's tile, the same every run: one of the eight, mirrored or not.
local function pick(cx, cy)
  local h = (cx * 7919 + cy * 104729 + 1013) % 65521
  h = (h * 31337 + 7) % 65521
  return h % VARIANTS, (h // VARIANTS) % 2 == 1
end

local function place(ft, t, sp, cx, cy)
  ft.cx, ft.cy = cx, cy
  t.pos = vector.create((cx + 0.5) * TILE, (cy + 0.5) * TILE, 0)
  local frame, flip = pick(cx, cy)
  sp.frame = frame
  sp.flip_x = flip
end

local function lay_floor(q, p)
  local bx = math.floor(p.x / TILE) - COLS // 2
  local by = math.floor(p.y / TILE) - ROWS // 2
  if q:count("FloorTile") == 0 then
    q:spawn_many(COLS * ROWS, { "Transform", "Sprite", "FloorTile" }, function(e, i)
      local ft, sp = e:get("FloorTile"), e:get("Sprite")
      ft.col = (i - 1) % COLS
      ft.row = (i - 1) // COLS
      sp.atlas = FLOOR
      sp.size = vector.create(TILE, TILE, 0)
      sp.z = FLOOR_Z
      place(ft, e:get("Transform"), sp, bx + ft.col, by + ft.row)
    end)
    return
  end
  local moved = false
  for _, ft, t, sp in q:each("FloorTile", "Transform", "Sprite") do
    local cx, cy = bx + ft.col, by + ft.row
    -- The tiles move together: when one shows its cell, they all do.
    if not moved and ft.cx == cx and ft.cy == cy then
      return
    end
    moved = true
    place(ft, t, sp, cx, cy)
  end
end

-- A new wave's sprites: dashers first, then the rest of the swarm.
local function dress_swarm(q)
  for _, _, sp in q:each("Dasher", "Sprite") do
    if sp.frame == 0 then
      sp.atlas = ATLAS
      sp.frame = DART
      sp.material = BODY
    end
  end
  local n = 0
  for _, _, sp in q:each("Enemy", "Sprite") do
    if sp.frame == 0 then
      sp.atlas = ATLAS
      sp.frame = MITE + n % 3
      sp.material = BODY
      n += 1
    end
  end
end

return system "late.looks" {
  stage = "late",
  -- On load too, so the first picture has its floor.
  on_load = true,
  reads = { "Transform", "Enemy", "Dasher", "Bullet", "FloorTile", "Sprite" },
  writes = { "Transform", "Sprite", "FloorTile", "Emitter" },
  doc = "Presentation only: lays the deck's tiles (sprites/floor) under the camera, each its world cell's; gives the swarm (on the tick a wave spawns) and each new bullet their sprites of sprites/swarm, keeping gameplay's colours as the tint; turns a dasher's frame with its phase (dash.* events); bursts the hero's sparks when he is hurt or dies. Writes nothing gameplay reads.",
  run = function(q)
    lay_floor(q, q:entity("@player"):get("Transform").pos)
    if #q:events("wave.spawned") > 0 then
      dress_swarm(q)
    end
    for _, phase in ipairs(PHASES) do
      for _, ev in ipairs(q:events(phase[1])) do
        -- Shot down later in the tick it began its phase: gone.
        if ev.entity:alive() then
          ev.entity:get("Sprite").frame = phase[2]
        end
      end
    end
    for _, _, sp in q:each("Bullet", "Sprite") do
      if sp.frame ~= BOLT then
        sp.atlas = ATLAS
        sp.frame = BOLT
        sp.material = GLOW
        sp.size = BOLT_SIZE
      end
    end
    if #q:events("damage") > 0 then
      q:entity("@herofx/hurt"):get("Emitter").burst = 18
    end
    if #q:events("player.died") > 0 then
      q:entity("@herofx/death"):get("Emitter").burst = 90
    end
  end,
}
