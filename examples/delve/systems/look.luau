-- How Delve's things are drawn (the art pass). Presentation only: sim.floor
-- and sim.potions spawn the monsters, the stairs, and the potions as
-- described shapes; each tick this gives them their sprites (tools/art.py
-- draws them), and it writes nothing a gameplay system reads: Sprite, the
-- monsters' Emissive, the stairs' and the torch's Light, the camera's
-- Lighting and PostVignette, the menu buttons' panels, the message line's
-- visibility, and the sparks' bursts. It draws on no random stream. Time
-- on a discrete clock is turns, so everything here moves a turn at a time.
--
-- - Monsters: their kind's sprite, facing the hero, two idle frames that
--   swap each turn (out of step with each other), a white flash the turn
--   they are struck, and a glow of their own (Emissive), so they are seen
--   outside the torch's reach.
-- - The hero: a red flash the turn a monster strikes them, green the turn
--   a potion heals them; facing the way they last stepped; fallen when
--   dead, and the torch burns low.
-- - The stairs: their sprite and a cold light over them. Potions: drawn
--   at their own colour (fx/shine), so they are seen in the dark.
-- - The dark: the deeper the floor, the less ambient light, never so
--   little the floor cannot be read; high contrast lifts it. The vignette
--   closes in as the hero's hit points fall.
-- - Sparks where a blow lands, more where something dies.
-- - The menu: the focused button lit; the message line's plate hidden
--   until there is a message.

local SPRITES = { rat = "sprites/rat", bat = "sprites/bat", goblin = "sprites/goblin" }
local ONE = vector.create(1, 1, 0)
-- Frames (tools/art.py): 0 and 1 idle for all; a monster's struck flash;
-- the hero fallen and hurt.
local STRUCK, FALLEN, HURT = 2, 2, 3
local HEALED = "#B4FFBC"
local SHINE = "fx/shine"
-- How much a monster shows at its own colour in the dark (Emissive).
local GLOW = 0.45
-- The ambient's level on floor 1, a step less a floor, the least it falls
-- to, and the least with high contrast.
local LEVEL, STEP, FLOOR_LEVEL, CONTRAST = 0.62, 0.025, 0.45, 0.95
local TORCH, TORCH_LOW = 1.3, 0.5

-- A field written only when it changes (a number when it moves by more
-- than its f32 storage loses).
local function set(c, field, v)
  if c[field] ~= v then c[field] = v end
end
local function near(c, field, v)
  if math.abs(c[field] - v) > 0.001 then c[field] = v end
end

-- An id's number (e:27 is 27), for frames out of step.
local function num(id)
  return tonumber(string.match(id, "^e:(%d+)")) or 0
end

return system "sim.look" {
  stage = "sim",
  -- After every gameplay system of the turn, so it dresses what they made.
  after = { "sim.messages", "sim.potions", "sim.monsters", "sim.floor_art" },
  on_load = true,
  -- The menu freezes the turn but not the look of its buttons.
  unfrozen = true,
  reads = { "Monster", "Stairs", "Potion", "Player", "Health", "Transform", "Turn", "Actions", "UiNode", "UiText", "UiSettings", "UiButton", "Sprite", "Emissive", "Light", "Lighting", "PostVignette", "UiPanel" },
  writes = { "Sprite", "Emissive", "Light", "Lighting", "PostVignette", "UiPanel", "UiNode" },
  doc = "Presentation only: gives the monsters their sprites (facing the hero, idle frames by turn, a white flash when struck) and a glow, the stairs and potions theirs, the hero theirs (a red flash when hurt, green when healed, fallen when dead), a light over the stairs, the camera's ambient level by floor (lifted by high contrast) and its vignette by the hero's hit points, sparks where blows land, the focused menu button its light, and the message line its plate once there is a message. Writes nothing gameplay reads.",
  run = function(q)
    local turn = q:entity("@turn"):get("Turn").n
    local pe = q:entity("@player")
    local pl = pe:get("Player")
    local hero = pe:get("Transform").pos
    local menu_open = q:entity("@menu"):get("UiNode").visible

    -- What this turn struck and killed, and where.
    local struck, hurt, healed = {}, false, false
    local sparks = {}
    local dead = {}
    for _, ev in ipairs(q:events("death")) do
      dead[tostring(ev.entity)] = true
      table.insert(sparks, { x = ev.x, y = ev.y, n = 16 })
    end
    for _, ev in ipairs(q:events("damage")) do
      local target = tostring(ev.target)
      if target == "@player" then
        hurt = true
        table.insert(sparks, { x = hero.x, y = hero.y, n = 4 })
      elseif not dead[target] then
        struck[target] = true
      end
    end
    for _, ev in ipairs(q:events("potion.drunk")) do
      if ev.healed > 0 then healed = true end
    end

    -- The monsters. A new floor's get their glow (Emissive) on the tick
    -- they appear, all of them, in the order q:each gives them: a component
    -- added moves an entity to another table, and moving them all in order
    -- keeps the order sim.floor later despawns them in, and so the ids it
    -- hands the next floor's monsters (whose initiative ties break by id).
    local new = {}
    for e, mo, sp, tr in q:each("Monster", "Sprite", "Transform") do
      local id = e:id()
      set(sp, "atlas", SPRITES[mo.kind] or "sprites/rat")
      set(sp, "size", ONE)
      set(sp, "frame", struck[id] and STRUCK or (turn + num(id)) % 2)
      set(sp, "flip_x", hero.x < tr.pos.x)
      set(sp, "color", "")
      if struck[id] then
        table.insert(sparks, { x = tr.pos.x, y = tr.pos.y, n = 7 })
      end
      if not e:has("Emissive") then table.insert(new, id) end
    end
    for _, id in ipairs(new) do
      q:entity(id):insert("Emissive", { amount = GLOW })
    end

    -- The stairs, and their light.
    local unlit = {}
    for e, _, sp in q:each("Stairs", "Sprite") do
      set(sp, "atlas", "sprites/stairs")
      set(sp, "size", ONE)
      set(sp, "color", "")
      if not e:has("Light") then table.insert(unlit, e:id()) end
    end
    for _, id in ipairs(unlit) do
      q:entity(id):insert("Light", { color = "#7FC4FF", intensity = 1.1, radius = 3.5, falloff = 1.4, shadows = false })
    end

    -- The potions.
    for _, _, sp in q:each("Potion", "Sprite") do
      set(sp, "atlas", "sprites/potion")
      set(sp, "size", ONE)
      set(sp, "color", "")
      set(sp, "material", SHINE)
    end

    -- The hero.
    local hs = pe:get("Sprite")
    local torch = pe:get("Light")
    if pl.dead then
      set(hs, "frame", FALLEN)
      set(hs, "color", "")
      near(torch, "intensity", TORCH_LOW)
    else
      set(hs, "frame", hurt and HURT or turn % 2)
      set(hs, "color", healed and HEALED or "")
      near(torch, "intensity", TORCH)
      local m = pe:get("Actions").move
      if not menu_open then
        if m.x > 0.5 then
          set(hs, "flip_x", false)
        elseif m.x < -0.5 then
          set(hs, "flip_x", true)
        end
      end
    end

    -- The dark, and the vignette.
    local level = math.max(FLOOR_LEVEL, LEVEL - STEP * math.max(0, pl.floor - 1))
    if q:entity("@settings"):get("UiSettings").high_contrast then
      level = CONTRAST
    end
    local hp = pe:get("Health")
    local missing = 1 - hp.cur / math.max(1, hp.max)
    for _, lit, vig in q:each("Lighting", "PostVignette") do
      near(lit, "level", level)
      near(vig, "amount", 0.13 + 0.29 * missing)
    end

    -- Sparks where blows landed.
    for _, s in ipairs(sparks) do
      q:emit("particles.burst", { emitter = pe, count = s.n, pos = vector.create(s.x, s.y, 0) })
    end

    -- The menu: the focused button lit.
    if menu_open then
      for _, b, p in q:each("UiButton", "UiPanel") do
        set(p, "color", b.focused and "button_focus" or "button")
      end
    end

    -- The message line's plate only once there is something to say.
    local me = q:entity("@messages")
    set(me:get("UiNode"), "visible", me:get("UiText").text ~= "")
  end,
}
