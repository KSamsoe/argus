-- The deep (phase 3 item 6): 197 generated rooms beyond the cave's left
-- wall (tools/deep.py), 200 with the cave, the shaft, and the hall. Rooms
-- stream around the hero: those within two doorways of the hero's room are
-- loaded, the persistent room (hero, camera, HUD, menus) always is, and the
-- rest are not. A room remembers what happened in it. A room change is an
-- event and plays the scene's fade. The index answers about rooms that are
-- not loaded.

local function set(t, entity, path, value)
  local tx = t:call("edit.begin", { author = "test:hollow", note = "the deep" }).tx
  t:call("edit.set", { tx = tx, entity = entity, path = path, value = value })
  t:call("edit.commit", { tx = tx })
end

-- The rooms loaded, and the scene's answer.
local function live(t)
  local r = t:call("scene.rooms", {})
  local out = {}
  for _, x in ipairs(r.rooms) do
    if x.loaded then table.insert(out, x.name) end
  end
  return out, r
end

local function has(list, name)
  for _, x in ipairs(list) do
    if x == name then return true end
  end
  return false
end

-- Stand the hero in a deep room, clear of its plank stack: the grid runs
-- left from the cave, 24 wide, and down, 14 high.
local function into(t, c, r)
  set(t, "@player", "Transform.pos", { -(c + 1) * 24 + 5, -14 * r + 1.8 })
  t:step(2)
end

return scenario "rooms stream around the hero, remember, and the index knows the rest" {
  scene = "hollow", seed = 1,
  run = function(t)
    t:step(2)
    local l, r = live(t)
    if #r.rooms ~= 201 then error("200 rooms and the persistent one: " .. #r.rooms, 0) end
    t:log(#l .. " of " .. #r.rooms .. " rooms loaded at the cave mouth")
    if r.current ~= "a" or r.follow ~= "@player" then error("the hero starts in the cave", 0) end
    for _, name in ipairs({ "persistent", "a", "b", "c", "deep-0-0" }) do
      if not has(l, name) then error(name .. " is loaded at the start", 0) end
    end
    t.expect:field("@player", "Transform.pos.x"):near(3, 0.5)

    -- Something happens in the shaft: the gem is taken.
    set(t, "@gem", "Gem.taken", true)
    t:step(1)

    -- Into the deep through the cave's left wall: a room change, the fade.
    into(t, 0, 0)
    t.expect:events("room.transition", { from = "a", to = "deep-0-0" }):count():eq(1)
    t.expect:events("timeline.started", { name = "rooms.fade" }):count():eq(1)

    -- Far down the maze: the cave, the shaft, and the hall stream out.
    into(t, 10, 6)
    l, r = live(t)
    t:log(#l .. " rooms loaded at deep-10-6: " .. table.concat(l, ", "))
    if r.current ~= "deep-10-6" then error("the hero's room: " .. tostring(r.current), 0) end
    if has(l, "b") or has(l, "a") then error("the first rooms are far behind: " .. table.concat(l, ", "), 0) end
    if not has(l, "persistent") then error("the persistent room never goes", 0) end
    if #l > 14 then error("a handful of rooms, not " .. #l, 0) end
    -- The hero, the camera, and the HUD came along.
    t.expect:count({ has = { "Gem" } }):eq(0)
    local hud = t.expect:field("@hud", "UiText.shown"):get()
    if not string.find(hud, "Hollow", 1, true) then error("the HUD: " .. tostring(hud), 0) end
    -- The shaft remembers its gem.
    local b = t:call("scene.rooms", { room = "b" })
    local what = {}
    for _, m in ipairs(b.remembered) do
      local parts = {}
      for _, s in ipairs(m.set) do table.insert(parts, s[1] .. "=" .. tostring(s[2])) end
      table.insert(what, m.entity .. " {" .. table.concat(parts, ", ") .. "} added " .. table.concat(m.added, ","))
    end
    t:log("b remembers: " .. table.concat(what, "; "))
    if #b.remembered ~= 1 or b.remembered[1].entity ~= "@b/gem" then
      error("b remembers its gem: " .. table.concat(what, "; "), 0)
    end

    -- The index answers about rooms that are not loaded, from the files.
    local q = t:call("world.query", { where = { room = "deep-19-0", has = { "glow" } }, select = { "id", "Describe.text" } })
    if q.source ~= "index" or q.total ~= 2 then
      error("the index answers for deep-19-0: " .. tostring(q.source) .. ", " .. tostring(q.total), 0)
    end
    t:log(q.rows[1][2])
    local all = t:call("world.query", { where = { has = { "crystal" } }, select = { "count()" }, source = "index" })
    t:log(all.rows[1][1] .. " crystals in the deep, from the index")
    local here = t:call("world.query", { where = { has = { "crystal" } }, select = { "count()" } })
    if here.unloaded_rooms == nil or here.unloaded_rooms < 180 then
      error("a live answer says what it could not see: " .. tostring(here.unloaded_rooms), 0)
    end

    -- Back to the shaft: its gem is still taken, and still not drawn.
    set(t, "@player", "Transform.pos", { 34, 3 })
    t:step(3)
    t.expect:field("@gem", "Gem.taken"):eq(true)
    t.expect:field("@gem", "Sprite.visible"):eq(false)

    -- A walk of 25 rooms, each through a doorway of the last: what a room
    -- change costs as a player meets it (section 17: stream-in under 4 ms,
    -- "must fit inside a frame so transitions never hitch"). The number
    -- is a release build's; a debug build is held to a loose bound.
    into(t, 0, 0)
    local seen, at, walked = { ["deep-0-0"] = true }, "deep-0-0", 0
    local rooms_by = {}
    for _, x in ipairs(r.rooms) do rooms_by[x.name] = x end
    for _ = 1, 25 do
      local nextroom = nil
      for _, n in ipairs(rooms_by[at].neighbors) do
        if not seen[n] and string.sub(n, 1, 5) == "deep-" then nextroom = n break end
      end
      if nextroom == nil then
        for _, n in ipairs(rooms_by[at].neighbors) do
          if string.sub(n, 1, 5) == "deep-" then nextroom = n break end
        end
      end
      seen[nextroom] = true
      local c, row = string.match(nextroom, "deep%-(%d+)%-(%d+)")
      into(t, tonumber(c), tonumber(row))
      at = nextroom
      walked += 1
    end
    t.expect:events("room.transition", { to = at }):exists()
    local p = t:call("prof.systems", { last = 2 * walked })
    for _, row in ipairs(p.rows) do
      if row[1] == "rooms.stream" then
        t:log(string.format("rooms.stream over a walk of %d rooms: worst %.2f ms, mean %.3f ms a tick", walked, row[5], row[3]))
        if row[5] > 40 then error("a room change took " .. row[5] .. " ms", 0) end
      end
    end

    -- A seek across a room change the hero walked through (no edit in
    -- between, so no keyframe at it): the rooms, and what they remember,
    -- come back with the world, and the replay is the run.
    set(t, "@player", "Transform.pos", { 3, 1.8 })
    t:step(2)
    local tick = t:status().tick
    t:step((61 - tick % 60) % 60)
    local walk_from = t:status().tick
    t.input:hold("move", { -1, 0 }, 90)
    t:run_until { event = "room.transition", max_frames = 90 }
    local ev = t:call("obs.events", { name = "room.transition", limit = 1000 })
    local last = ev.rows[#ev.rows]
    local crossed = last.frame
    if last.fields.to ~= "deep-0-0" then error("walked into deep-0-0: " .. tostring(last.fields.to), 0) end
    t:step(30)
    local s = t:call("sim.seek", { frame = crossed + 2 })
    if s.diverged ~= nil then error("the replay across the room change diverged", 0) end
    if not (s.from <= crossed and s.replayed > 0 and (s.checked or 0) > 0) then
      error("the seek replayed the room change: from " .. tostring(s.from) .. ", crossed " .. crossed .. ", checked " .. tostring(s.checked), 0)
    end
    l, r = live(t)
    if r.current ~= "deep-0-0" then error("back just after the doorway: " .. tostring(r.current), 0) end
    local b2 = t:call("scene.rooms", { room = "b" })
    if not b2.loaded then error("b is two rooms from deep-0-0", 0) end
    t.expect:field("@gem", "Gem.taken"):eq(true)
    t:log("walked from frame " .. walk_from .. " through the doorway at frame " .. crossed .. "; sought to " .. (crossed + 2) .. ": " .. s.replayed .. " ticks replayed from keyframe " .. s.from .. ", " .. s.checked .. " events checked, none different")
    -- Nothing failed while the cave and the hall were streamed out: the
    -- hermit's and the run's systems found their entities gone (found by
    -- phase 3 item 8, whose mod room took the hero three rooms from the
    -- hermit, whose system stopped for good).
    local se = t:call("script.errors", {})
    if se.total ~= 0 then error("script errors: " .. tostring(se.rows[1].message), 0) end
  end,
}
