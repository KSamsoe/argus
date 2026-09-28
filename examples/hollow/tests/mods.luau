-- Mods (phase 3 item 8): the lanterns mod and a mod on it, brighter, laid
-- over Hollow in that order (mods/lanterns, mods/brighter). What each
-- added and changed is in mods.list; then the mod's content played: its
-- door in the game's hall, its room and its music on the bar, its keeper's
-- conversation lighting its lanterns, the choice it adds to the game's
-- hermit, and the line it changes of his.

local function set(t, entity, path, value)
  local tx = t:call("edit.begin", { author = "test:hollow", note = "the mods" }).tx
  t:call("edit.set", { tx = tx, entity = entity, path = path, value = value })
  t:call("edit.commit", { tx = tx })
end

-- The words the conversation's box shows, top to bottom.
local function box(t)
  local q = t:call("world.query", { select = { "UiRect.pos", "UiText.shown" }, where = { has = { "TalkPart", "UiText" } } })
  table.sort(q.rows, function(a, b) return a[1][2] < b[1][2] end)
  local out = {}
  for _, r in ipairs(q.rows) do table.insert(out, r[2]) end
  return table.concat(out, " | ")
end

local function changed(list, kind, name, by)
  for _, c in ipairs(list.changes) do
    if c.kind == kind and c.name == name and c.by == by then return c end
  end
  return nil
end

-- Step a tick at a time until the track sounds.
local function until_started(t, track)
  local before = t:call("obs.events", { name = "music.started", limit = 1000 }).total
  for n = 1, 600 do
    t:step(1)
    local ev = t:call("obs.events", { name = "music.started", limit = 1000 })
    if ev.total > before and ev.rows[#ev.rows].fields.track == track then return n end
  end
  error(track .. " never started", 0)
end

scenario "the lanterns mod and brighter over it: what they add and change" {
  scene = "hollow", seed = 1, mods = { "lanterns", "brighter" },
  run = function(t)
    t:step(2)
    local m = t:call("mods.list", {})
    if #m.loaded ~= 2 or m.loaded[1].id ~= "lanterns" or m.loaded[2].id ~= "brighter" then
      error("lanterns, then brighter", 0)
    end
    local adds = m.loaded[1].adds
    t:log(string.format("lanterns adds %d scripts, %d components, %d def rows, %d rooms, %d entities, %d assets, %d sounds, %d strings",
      adds.scripts, adds.components, adds["def rows"], adds.rooms, adds.entities, adds.assets, adds.sounds, adds.strings))
    if adds.rooms ~= 1 or adds.components ~= 4 or adds.scripts ~= 9 then error("what lanterns adds", 0) end
    -- The load-order rule: later wins, and every change says whose it was.
    local c = changed(m, "string", "hermit.again", "lanterns")
    if c == nil or c.over ~= "the game" then error("lanterns changes the hermit's greeting", 0) end
    c = changed(m, "asset", "lanterns/lantern", "brighter")
    if c == nil or c.over ~= "mod lanterns" then error("brighter replaces lanterns' lantern", 0) end
    c = changed(m, "def row", "timeline:lanterns.kindle.swell", "brighter")
    if c == nil or c.over ~= "mod lanterns" then error("brighter changes a cue of lanterns' timeline", 0) end
    c = changed(m, "string", "lanterns.keeper.lit", "brighter")
    if c == nil then error("brighter changes the keeper's line", 0) end
    -- A mod's own keys in its second language are its own, not changes.
    if changed(m, "string", "lanterns.keeper.name", "lanterns") ~= nil then
      error("lanterns.keeper.name in French is lanterns' own", 0)
    end
    t.expect:field("@lanterns.state", "lanterns_Vigil.kindled"):eq(false)
    t.expect:field("@a/lanterns.lantern", "lanterns_Lantern.lit"):eq(false)

    -- The door the mod adds to the game's hall, and through it.
    set(t, "@player", "Transform.pos", { 46.0, 1.8 })
    t:step(3)
    t.expect:field("@c/lanterns.door", "UiRect.shown"):eq(true)
    t.input:tap("talk")
    t:step(3)
    t.expect:events("lanterns.through", {}):count():eq(1)
    t.expect:field("@player", "InRoom.room"):eq("lanterns-vigil")
    t.expect:field("@player", "Transform.pos.x"):gt(80)
    -- It stands on the vigil's floor: the room's own tilemap, a mod's file.
    t:step(30)
    t.expect:field("@player", "Transform.pos.y"):gt(0.9)

    -- The vigil's music, from the mod's audio.toml, on the cave's bar.
    local q = t:call("obs.events", { name = "music.queued", limit = 1000 })
    local queued = q.rows[#q.rows]
    if queued == nil or queued.fields.track ~= "lanterns/vigil" then
      until_started(t, "lanterns/vigil")
    else
      until_started(t, "lanterns/vigil")
      t:log(string.format("into the vigil: its music asked for %.0f ms before the bar", queued.fields.in_ms))
    end
    t.expect:field("@music", "Music.intensity"):eq(0)

    -- The keeper: his conversation (the mod's dialogue rows), lighting the lanterns.
    set(t, "@player", "Transform.pos", { 95.0, 1.8 })
    t:step(3)
    t.expect:field("@lanterns-vigil/keeper", "UiRect.shown"):eq(true)
    t.input:tap("talk")
    t:step(2)
    local b = box(t)
    t:log(b)
    if b ~= "The lantern keeper | You found the vigil. I keep the lanterns of the hollow, when there is oil. | Continue" then
      error("the keeper's first box: " .. b, 0)
    end
    t.input:tap("ui_confirm")
    t:step(2)
    b = box(t)
    if b ~= "The lantern keeper | Shall I light them? | Light the lanterns | Not now" then
      error("the keeper's choices: " .. b, 0)
    end
    t.input:tap("ui_confirm")
    t:step(2)
    t.expect:field("@lanterns.state", "lanterns_Vigil.kindled"):eq(true)
    b = box(t)
    -- brighter's line, over lanterns'.
    if b ~= "The lantern keeper | There, brighter than they have burned in years. | Continue" then
      error("the keeper once lit: " .. b, 0)
    end
    t.input:tap("ui_confirm")
    t:step(3)
    t.expect:count({ has = { "Talk" } }):eq(0)
    -- The kindling: the lanterns burn, the timeline plays, the bell comes in.
    t:step(5)
    t.expect:field("@lanterns-vigil/lantern.1", "lanterns_Lantern.lit"):eq(true)
    t.expect:events("timeline.started", { name = "lanterns.kindle" }):count():eq(1)
    t.expect:events("sound.played", { sound = "lanterns/kindle" }):exists()
    t.expect:field("@music", "Music.intensity"):eq(1)
    t.expect:field("@lanterns-vigil/lantern.1", "Sprite.color"):eq("#FFC060")

    -- Back through the vigil's door, and on to the cave mouth: its lantern
    -- (the mod's, in the game's room) burns too, and the cave's music is back.
    set(t, "@player", "Transform.pos", { 82.5, 1.8 })
    t:step(3)
    t.input:tap("talk")
    t:step(3)
    t.expect:field("@player", "InRoom.room"):eq("c")
    set(t, "@player", "Transform.pos", { 6.0, 1.8 })
    t:step(3)
    t.expect:field("@a/lanterns.lantern", "lanterns_Lantern.lit"):eq(true)
    until_started(t, "cave")

    -- The hermit: met again, his greeting is the mod's; the mod's choice is
    -- among his.
    set(t, "@hermit", "Hermit.met", true)
    t:step(2)
    t.input:tap("talk")
    t:step(2)
    b = box(t)
    if b ~= "The hermit | Back again? The lantern keeper was asking after you. | Continue" then
      error("the hermit's greeting, changed: " .. b, 0)
    end
    t.input:tap("ui_confirm")
    t:step(2)
    b = box(t)
    t:log(b)
    if not string.find(b, "Who else lives down here?", 1, true) then
      error("the mod's choice among the hermit's: " .. b, 0)
    end
    -- Nothing failed on the way: the game's systems keep working while the
    -- rooms their entities are in stream out.
    local se = t:call("script.errors", {})
    if se.total ~= 0 then error("script errors: " .. tostring(se.rows[1].message), 0) end
  end,
}

scenario "without the mods, none of it is there" {
  scene = "hollow", seed = 1,
  run = function(t)
    t:step(2)
    local m = t:call("mods.list", {})
    if #m.loaded ~= 0 then error("no mods", 0) end
    -- Nor its components: the world has never heard of them.
    local ok = pcall(function() return t:call("world.get", { entity = "@c/lanterns.door" }) end)
    if ok then error("the lantern door is not there without the mod", 0) end
  end,
}
