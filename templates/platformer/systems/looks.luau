-- How the level looks. Presentation only: it reads the game (where the
-- blocks are, how the hero moves, what happened this tick) and writes only
-- what is drawn, so taking this file out changes how the game looks and
-- nothing about how it plays. The pictures are assets/sprites and
-- assets/backdrop, drawn by assets/sprites.art.luau and backdrop.art.luau.
--
-- - The blocks. Every solid block still drawn as a plain rectangle (a
--   Sprite with no atlas, as prefabs/platform.toml has) is dressed: tiles
--   are laid over it as its children, sized to it, and the rectangle is
--   hidden. Grass along its top, earth below, a dark edge down each side
--   and along the underside where no other block covers it, grass curled
--   over its top corners. A one-way platform gets wooden planks instead.
--   So a block you add, of any size, is dressed the tick it appears.
-- - The backdrop. Each Parallax layer (scenes/level/clouds.toml,
--   hills.far.toml, hills.near.toml) keeps `follow` of the camera's
--   movement, so far hills slide by slower than near ones.
-- - The hero faces the way it moves (assets/anim/hero.toml picks its
--   frames: run, jump, fall).
-- - Dust where the hero jumps and lands, sparkles where a coin is taken
--   and at the flag (scenes/level/dust.toml and sparkle.toml).
--
-- It runs in the late stage after camera.update, so the backdrop is placed
-- for the view this tick shows.

local ATLAS = "sprites"
-- Frames of assets/sprites, in the order assets/sprites.art.luau draws
-- them (its first lines list them).
local GRASS, GRASS_KINDS = 23, 3
local EARTH, EARTH_KINDS = 26, 5
local EDGE, CORNER, UNDERSIDE, PLANK = 31, 32, 33, 34

local PX = 1 / 15 -- one pixel of the art, in world units
local BLADES = 3 * PX -- grass blades stand this far above a block's top

local function v2(x, y)
  return vector.create(x, y, 0)
end

-- The same tile for the same place every run: which of the kinds, and
-- whether it is mirrored, from a hash of where it is.
local function pick(x, y, kinds)
  local k = math.floor(x) * 31 + math.floor(y) * 1009 + 7
  local h = math.floor((k * 2654435761) % 4294967296 / 65536)
  return h % kinds, (h // kinds) % 2 == 1
end

-- The parts of [lo, hi] left after taking away every span in spans.
local function uncovered(lo, hi, spans)
  local parts = { { lo, hi } }
  for _, s in ipairs(spans) do
    local rest = {}
    for _, p in ipairs(parts) do
      if s[2] <= p[1] or s[1] >= p[2] then
        table.insert(rest, p)
      else
        if s[1] > p[1] then
          table.insert(rest, { p[1], s[1] })
        end
        if s[2] < p[2] then
          table.insert(rest, { s[2], p[2] })
        end
      end
    end
    parts = rest
  end
  return parts
end

-- One tile: a child of the block, placed from the block's middle.
local function tile(q, block, frame, x, y, w, h, z, flip)
  local e = q:spawn({ "Transform", "GlobalTransform", "Parent", "Sprite" })
  e:get("Parent").entity = block
  e:get("Transform").pos = v2(x, y)
  local s = e:get("Sprite")
  s.atlas = ATLAS
  s.frame = frame
  s.size = v2(w, h)
  s.z = z
  s.flip_x = flip == true
end

-- Lays one block's tiles. b is the block (its world rect l, r, bottom, top
-- and its z); blocks is every solid block, to find the sides another
-- covers.
local function dress(q, b, blocks)
  local block = q:entity(b.id)
  local w, h = b.r - b.l, b.top - b.bottom
  local cols, rows = math.ceil(w - 0.001), math.ceil(h - 0.001)
  for i = 0, cols - 1 do
    local x0 = -w / 2 + i
    local cw = math.min(1, w / 2 - x0)
    local cx = x0 + cw / 2
    local wx = b.l + i
    if b.one_way then
      tile(q, block, PLANK, cx, h / 2 - 3 * PX, cw, 6 * PX, b.z)
    else
      for j = 0, rows - 1 do
        local y1 = h / 2 - j
        local rh = math.min(1, y1 + h / 2)
        local cy = y1 - rh / 2
        if j == 0 then
          local kind, flip = pick(wx, b.top, GRASS_KINDS)
          tile(q, block, GRASS + kind, cx, cy + BLADES / 2, cw, rh + BLADES, b.z, flip)
        else
          local kind, flip = pick(wx, b.top - j, EARTH_KINDS)
          tile(q, block, EARTH + kind, cx, cy, cw, rh, b.z, flip)
        end
      end
    end
  end
  if b.one_way then
    return
  end

  -- The sides, the underside, and the top corners, where no other block is.
  local left, right, under = {}, {}, {}
  for _, o in ipairs(blocks) do
    if o.id ~= b.id then
      if o.l < b.l and o.r >= b.l then
        table.insert(left, { o.bottom, o.top })
      end
      if o.r > b.r and o.l <= b.r then
        table.insert(right, { o.bottom, o.top })
      end
      if o.bottom < b.bottom and o.top >= b.bottom then
        table.insert(under, { o.l, o.r })
      end
    end
  end
  for _, sided in ipairs({ { -1, left }, { 1, right } }) do
    local side, spans = sided[1], sided[2]
    local x = side * (w / 2 - 1.5 * PX)
    for _, p in ipairs(uncovered(b.bottom, b.top, spans)) do
      local lo, hi = p[1], p[2]
      if hi >= b.top then
        -- The grass curls over this corner; the edge starts under it.
        local corner_h = 12 * PX
        tile(q, block, CORNER, side * (w / 2 - 2.5 * PX), h / 2 + BLADES - corner_h / 2, 5 * PX, corner_h, b.z + 0.02, side > 0)
        hi -= 4 * PX
      end
      local mid = (lo + hi) / 2 - (b.bottom + b.top) / 2
      tile(q, block, EDGE, x, mid, 3 * PX, hi - lo, b.z + 0.01, side > 0)
    end
  end
  for _, p in ipairs(uncovered(b.l, b.r, under)) do
    tile(q, block, UNDERSIDE, (p[1] + p[2]) / 2 - (b.l + b.r) / 2, -h / 2 + 1.5 * PX, p[2] - p[1], 3 * PX, b.z + 0.01)
  end
end

local function dress_blocks(q)
  -- Most ticks every block is dressed already: look before building lists.
  local any = false
  for _, body, s in q:each("KinematicBody", "Sprite") do
    any = any or (body.solid and s.visible and s.atlas == "")
  end
  if not any then
    return
  end
  local blocks, bare = {}, {}
  for e, body, c, t, s in q:each("KinematicBody", "Collider", "Transform", "Sprite") do
    if body.solid then
      local x, y = t.pos.x + c.offset.x, t.pos.y + c.offset.y
      local b = {
        id = e:id(),
        l = x - c.size.x / 2,
        r = x + c.size.x / 2,
        bottom = y - c.size.y / 2,
        top = y + c.size.y / 2,
        z = s.z,
        one_way = body.one_way,
      }
      table.insert(blocks, b)
      if s.visible and s.atlas == "" then
        table.insert(bare, b)
      end
    end
  end
  -- Tiles are spawned after the loop: spawning inside it would move the
  -- entities it walks over.
  for _, b in ipairs(bare) do
    dress(q, b, blocks)
    q:entity(b.id):get("Sprite").visible = false
  end
end

local function place_backdrop(q)
  local cam = q:entity("@camera"):get("Camera")
  local x = (cam.view_min.x + cam.view_max.x) / 2
  for _, p, t in q:each("Parallax", "Transform") do
    p.slid = (p.slid + p.drift * q.dt) % p.width
    -- Where the layer would be, then moved a whole number of widths so it
    -- and its copy (width to the right) cover the view.
    local at = x * p.follow - p.slid
    at += p.width * math.floor((x - at) / p.width)
    t.pos = v2(at, t.pos.y)
  end
end

return system "late.looks" {
  stage = "late",
  after = { "camera.update" },
  -- Once when the level loads too, so the very first picture is dressed.
  on_load = true,
  reads = { "KinematicBody", "Collider", "Transform", "Sprite", "Camera", "Parallax" },
  writes = { "Sprite", "Transform", "GlobalTransform", "Parent", "Parallax" },
  doc = "Presentation only: dresses every solid block still drawn as a plain rect with tiles of assets/sprites (grass, earth, edges; planks when one-way) as its children; slides the Parallax layers with the camera; turns the hero to face the way it moves; bursts dust (@dust) where it jumps and lands and sparkles (@sparkle) where a coin is taken and at the flag. Writes nothing gameplay reads.",
  run = function(q)
    dress_blocks(q)
    place_backdrop(q)

    local hero = q:entity("@player")
    local feet = hero:get("Transform").pos - v2(0, hero:get("Collider").size.y / 2)
    local vx = hero:get("KinematicBody").velocity.x
    if vx > 0.5 then
      hero:get("Sprite").flip_x = false
    elseif vx < -0.5 then
      hero:get("Sprite").flip_x = true
    end

    local dust, sparkle = q:entity("@dust"), q:entity("@sparkle")
    for _, ev in ipairs(q:events("body.landed")) do
      if ev.entity == hero then
        q:emit("particles.burst", { emitter = dust, count = 12, pos = feet })
      end
    end
    for _, ev in ipairs(q:events("body.jumped")) do
      if ev.entity == hero then
        q:emit("particles.burst", { emitter = dust, count = 7, pos = feet })
      end
    end
    for _, ev in ipairs(q:events("coin.taken")) do
      q:emit("particles.burst", { emitter = sparkle, count = 14, pos = ev.entity:get("Transform").pos })
    end
    for _ in ipairs(q:events("goal.reached")) do
      local flag = q:entity("@goal"):get("Transform").pos
      q:emit("particles.burst", { emitter = sparkle, count = 40, pos = flag + v2(0, 1.2) })
    end
  end,
}
