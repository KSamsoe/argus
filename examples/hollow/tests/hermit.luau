-- The hermit at the cave mouth (phase 3 item 4): a conversation from the
-- dialogue table, played through the UI with the menu actions. Lines and
-- choices; a choice offered only when the world allows it; edits the
-- conversation makes; the game standing still while it is open; and the
-- words in French when the settings say so.

local function set(t, entity, path, value)
  local tx = t:call("edit.begin", { author = "test:hollow", note = "the hermit" }).tx
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

-- Whether any string in a JSON-shaped value holds `words`.
local function holds(v, words)
  if type(v) == "string" then return string.find(v, words, 1, true) ~= nil end
  if type(v) == "table" then
    for _, x in pairs(v) do
      if holds(x, words) then return true end
    end
  end
  return false
end

local function talk(t)
  set(t, "@player", "Transform.pos", { 6, 1.8 })
  t:step(3)
  t.expect:field("@hermit", "UiRect.shown"):eq(true)
  t.input:tap("talk")
  t:step(2)
end

scenario "the hermit talks, offers what the world allows, and remembers" {
  scene = "hollow", seed = 1,
  run = function(t)
    t:step(2)
    t.expect:field("@hermit", "UiRect.shown"):eq(false)
    talk(t)
    t.expect:count({ has = { "Talk" } }):eq(1)
    t.expect:events("dialogue.started", { start = "dialogue:hermit.start" }):count():eq(1)
    t.expect:field("@hermit", "Hermit.met"):eq(true)
    t.expect:field("@hermit", "UiRect.shown"):eq(false)
    local b = box(t)
    t:log(b)
    if b ~= "The hermit | Few come this far. The cave goes on past the shaft, and so can you. | Continue" then
      error("the first box: " .. b, 0)
    end
    t.expect:events("sound.played", { sound = "sfx/voice" }):exists()
    -- Said on the dialogue bus, the line ducks the music (section 13's
    -- "dialogue ducks music by 6 dB over 200 ms", a rule in audio.toml).
    local a = t:call("audio.status", {})
    local duck
    for _, r in ipairs(a.rows or {}) do
      if r[1] == "music" then duck = r[5] end
    end
    if a.device ~= nil or a.rows ~= nil then
      if duck == nil or duck >= 0 then error("the music is ducked while he speaks: " .. tostring(duck), 0) end
      t:log("music ducked " .. duck .. " dB two ticks in")
    end
    -- The game stands still while he talks.
    local x = t.expect:field("@player", "Transform.pos.x"):get()
    t.input:hold("move", { 1, 0 }, 20)
    t:step(20)
    t.expect:field("@player", "Transform.pos.x"):eq(x)
    -- On: two choices (no gem yet); where the cave leads, then back.
    t.input:tap("ui_confirm")
    t:step(2)
    b = box(t)
    if b ~= "The hermit | What is it? | Where does the cave lead? | Goodbye" then
      error("the choices: " .. b, 0)
    end
    t.input:tap("ui_confirm")
    t:step(2)
    t.expect:events("dialogue.chose", { choice = "hermit.ask.lead" }):count():eq(1)
    t.input:tap("ui_confirm")
    t:step(2)
    -- Goodbye: down one, and the conversation is over.
    t.input:tap("ui_move", { 0, -1 })
    t:step(2)
    t.input:tap("ui_confirm")
    t:step(2)
    t.expect:count({ has = { "Talk" } }):eq(0)
    t.expect:count({ has = { "TalkPart" } }):eq(0)
    t.expect:events("dialogue.ended", { node = "hermit.ask.bye" }):count():eq(1)
    -- The game goes on.
    t.input:hold("move", { 1, 0 }, 20)
    t:step(20)
    t.expect:field("@player", "Transform.pos.x"):gt(x)
    -- With the gem found, he is offered it, once; and he remembers meeting.
    set(t, "@hud", "Run.gems", 1)
    talk(t)
    b = box(t)
    if not string.find(b, "Back again?", 1, true) then error("met before: " .. b, 0) end
    t.input:tap("ui_confirm")
    t:step(2)
    t.expect:field(t:call("world.query", { select = { "id" }, where = { has = { "Talk" } } }).rows[1][1], "Talk.choices"):eq(3)
    t.input:tap("ui_move", { 0, -1 })
    t:step(2)
    t.input:tap("ui_confirm")
    t:step(2)
    t.expect:field("@hermit", "Hermit.has_gem"):eq(true)
    t.expect:events("hermit.given"):count():eq(1)
    t.input:tap("ui_confirm")
    t:step(2)
    t.expect:count({ has = { "Talk" } }):eq(0)
    talk(t)
    t.input:tap("ui_confirm")
    t:step(2)
    b = box(t)
    if string.find(b, "Give the gem", 1, true) then error("given once: " .. b, 0) end
  end,
}

return scenario "the game is in French when the settings say so" {
  scene = "hollow", seed = 1,
  run = function(t)
    t:step(2)
    -- Pause, Settings (the fourth button), Language (the fourth there).
    t.input:tap("pause")
    t:step(2)
    for _ = 1, 3 do
      t.input:tap("ui_move", { 0, -1 })
      t:step(2)
    end
    t.input:tap("ui_confirm")
    t:step(2)
    t.expect:field("@settings.language", "UiText.shown"):eq("Language: English")
    for _ = 1, 3 do
      t.input:tap("ui_move", { 0, -1 })
      t:step(2)
    end
    t.expect:field("@settings.language", "UiButton.focused"):eq(true)
    t.input:tap("ui_confirm")
    t:step(2)
    t.expect:field("@settings", "UiSettings.language"):eq("fr")
    t.expect:field("@settings.language", "UiText.shown"):eq("Langue : Français")
    t.expect:field("@settings.title", "UiText.shown"):eq("Réglages")
    t.expect:field("@settings.text", "UiText.shown"):eq("Taille du texte : 100 %")
    t.expect:field("@pause.resume", "UiText.shown"):eq("Reprendre")
    -- Back, resume: the HUD and the hermit in French too.
    t.input:tap("ui_cancel")
    t:step(2)
    t.input:tap("ui_confirm")
    t:step(2)
    t.expect:field("@pause", "UiRect.shown"):eq(false)
    t.expect:field("@hud", "UiText.shown"):eq("Hollow. Trouve la sortie. Aucune chute.")
    talk(t)
    local b = box(t)
    if b ~= "L’ermite | Peu de gens viennent jusqu’ici. La grotte continue après le puits, et toi aussi. | Continuer" then
      error("in French: " .. b, 0)
    end
    -- What the player sees, the agent reads: the semantic frame's UI.
    local f = t:call("obs.frame", { format = "semantic" })
    if not holds(f.ui, "Peu de gens") then
      error("the semantic frame's ui has the French line", 0)
    end
  end,
}
