-- Hollow on its platform (phase 5 item 9). The daemon's null platform
-- stands for Steam: it takes what the game asks for, so the achievements,
-- the rich presence, the overlay's pause, and the button prompts are
-- tested without Steam present.

local function set(t, entity, path, value)
  local tx = t:call("edit.begin", { author = "test:hollow", note = "platform" }).tx
  t:call("edit.set", { tx = tx, entity = entity, path = path, value = value })
  t:call("edit.commit", { tx = tx })
end

local function unlocked(t)
  local out = {}
  for _, a in ipairs(t:call("platform.status", {}).achievements) do
    if a.unlocked then
      table.insert(out, a.id)
    end
  end
  table.sort(out)
  return table.concat(out, ",")
end

local function hud_line(t)
  return t:call("world.get", { entity = "@hud" }).components.UiText.text
end

local function presence(t)
  return t:call("platform.status", {}).state.presence.status
end

return scenario "the way out is two achievements, the prompt follows the pad, and the overlay pauses" {
  scene = "hollow", seed = 1,
  run = function(t)
    t:call("platform.reset", {})
    t:step(2)
    if presence(t) ~= "In the cave" then
      error("friends see: " .. tostring(presence(t)), 0)
    end
    -- The way out without a fall: two achievements, each answered.
    set(t, "@player", "Transform.pos", { 62, 1.8 })
    t.input:hold("move", { 1, 0 }, 120)
    t:run_until { event = "run.finished", max_frames = 240 }
    t:step(1)
    t.expect:events("achievement.unlock", { id = "out_of_the_hollow" }):count():eq(1)
    t.expect:events("achievement.unlock", { id = "sure_footed" }):count():eq(1)
    t.expect:events("platform.achieved", { id = "sure_footed", api = "ACH_TRAVEL_FAR_SINGLE" }):count():eq(1)
    t.expect:events("platform.error"):count():eq(0)
    if unlocked(t) ~= "out_of_the_hollow,sure_footed" then
      error("unlocked: " .. unlocked(t), 0)
    end
    -- The finish line names restart's key; on an Xbox pad, its button.
    local line = hud_line(t)
    if not string.find(line, "Press R to go again", 1, true) then
      error("the line: " .. line, 0)
    end
    t.input:hold("device", 1, 600)
    t:step(3)
    t.expect:field("@hud", "InputGlyph.label"):eq("View")
    t.expect:field("@hud", "InputGlyph.shown"):eq("xbox")
    line = hud_line(t)
    if not string.find(line, "Press View to go again", 1, true) then
      error("the line on a pad: " .. line, 0)
    end
    -- Steam's overlay opening pauses the game, as Escape does.
    t.expect:field("@pause", "UiRect.shown"):eq(false)
    t:call("platform.overlay", { open = true })
    t:step(2)
    t.expect:field("@pause", "UiRect.shown"):eq(true)
    t:call("platform.overlay", { open = false })
    t:step(2)
    t.expect:field("@pause", "UiRect.shown"):eq(true)
  end,
}
