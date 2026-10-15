-- Hollow under a finger (reaching players item 1): a thumb on the left
-- half of the screen is the stick, centred where it lands, and walks the
-- hero; a finger on the Jump button jumps. Played by touch, the camera
-- frames the hero higher, clear of the thumbs. Places are the UI canvas's
-- (960 by 540): Jump's centre is 110 in from the bottom right corner.

local function x(t) return t.expect:field("@player", "Transform.pos.x"):get() end
local function y(t) return t.expect:field("@player", "Transform.pos.y"):get() end

-- How far up the screen the hero is drawn, 0 the bottom and 1 the top.
local function up_screen(t)
  local cam = t:call("world.get", { entity = "@camera" }).components.Camera
  return (y(t) - cam.view_min[2]) / (cam.view_max[2] - cam.view_min[2])
end

local framing = scenario "played by touch, the hero is drawn above the thumbs" {
  scene = "hollow", seed = 1,
  run = function(t)
    t:step(30)
    -- On the keyboard the view stops at the room's floor, and the hero
    -- standing on it is drawn low, where the thumbs would be.
    local keys = up_screen(t)
    if keys > 0.25 then error(string.format("on the keyboard the hero is drawn %.2f up", keys), 0) end
    -- A tap anywhere: touch is the device now, and the view glides lower
    -- (the camera's touch_offset), past the floor.
    t.input:touch(1, "start", 600, 150)
    t:step(2)
    t.input:touch(1, "end", 600, 150)
    t:step(90)
    -- Above the top of the stick (240 of the canvas's 540 up) and of Jump
    -- (175 up).
    local touch = up_screen(t)
    if touch < 240 / 540 then error(string.format("by touch the hero is drawn %.2f up", touch), 0) end
    t:log(string.format("the hero is drawn %.2f up the screen on the keyboard, %.2f by touch", keys, touch))
  end,
}

local walk = scenario "a thumb on the stick walks the hero and a finger on Jump jumps" {
  scene = "hollow", seed = 1,
  run = function(t)
    t:step(30)
    local x0, y0 = x(t), y(t)
    -- The thumb lands, then slides all the way right: running right.
    t.input:touch(1, "start", 150, 390)
    t.input:touch(1, "move", 240, 390)
    t:step(40)
    local x1 = x(t)
    if not (x1 > x0 + 1) then error(string.format("the stick walked from %.2f to %.2f", x0, x1), 0) end
    t.expect:field("@player", "Actions.move.x"):near(1, 0.01)
    -- A second finger on Jump: up off the ground, still running.
    t.input:touch(2, "start", 850, 430)
    t:step(8)
    local y1 = y(t)
    if not (y1 > y0 + 0.3) then error(string.format("Jump rose from %.2f to %.2f", y0, y1), 0) end
    t.input:touch(2, "end", 850, 430)
    -- Lifted, the hero stops being pushed.
    t.input:touch(1, "end", 240, 390)
    t:step(2)
    t.expect:field("@player", "Actions.move.x"):eq(0)
  end,
}

return { walk, framing }
