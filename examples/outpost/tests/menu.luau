-- The menu (phase 5 item 2): Escape opens it and the game stands still,
-- the colony's clock, its speed and pause keys, and the view; the menu
-- actions move and press in it; Escape backs out a page at a time and
-- closes it, as Resume does. The inspector and the menu take turns: with
-- the inspector open Escape is the inspector's, and with the menu open Tab
-- does nothing. Its Settings scale every text and turn high contrast and
-- fullscreen on; its Controls show each action's key and rebind it. It
-- works the same in the stress scene.

local press = require("helpers").press

-- A key pressed and let go, down the window's path: Escape is both the
-- menu action and cancel, as a player's is.
local function key(t, name)
  t:call("input.device", { events = { { key = name, pressed = true } } })
  t:step(1)
  t:call("input.device", { events = { { key = name, pressed = false } } })
  t:step(1)
end

-- Whether the menu shows, and its page shown.
local function menu(t)
  local root = t:call("world.query", { select = { "UiRect.shown" }, where = { has = { "Menu" } } })
  local pages = t:call("world.query", { select = { "MenuPage.name", "UiRect.shown" }, where = { has = { "MenuPage" } } })
  local page = nil
  for _, r in ipairs(pages.rows) do
    if r[2] then page = r[1] end
  end
  return root.rows[1][1], page
end

local function inspector(t)
  local q = t:call("world.query", { select = { "UiRect.shown" }, where = { has = { "Inspector" } } })
  return q.rows[1][1]
end

-- The button pressing which emits `event` has the focus.
local function focused(t, event)
  t.expect:count({ has = { "UiButton" }, ["UiButton.event"] = event, ["UiButton.focused"] = true }):eq(1)
end

-- The open menu's buttons as the semantic frame reads them, and the one
-- with focus.
local function buttons(t)
  local f = t:call("obs.frame", { format = "semantic" })
  local out, focus = {}, nil
  for _, u in ipairs(f.ui) do
    if u.button then
      table.insert(out, u.text)
      if u.focused then focus = u.text end
    end
  end
  return table.concat(out, " | "), focus
end

local function view(t)
  local q = t:call("world.query", { select = { "Transform.pos", "Camera.pan_action" }, where = { has = { "Camera" }, ["not"] = { "Vignette" } } })
  return q.rows[1][1], q.rows[1][2]
end

local function opens_and_stands_still(t)
  t:step(2)
  local open = menu(t)
  if open then error("the menu starts closed", 0) end
  key(t, "Escape")
  local open, page = menu(t)
  if not open or page ~= "main" then error("Escape opens it on its first page: " .. tostring(page), 0) end
  focused(t, "menu.resume")
  t.expect:events("menu.opened"):count():eq(1)
  local list, focus = buttons(t)
  if list ~= "Resume | Controls | Settings" then error("the semantic frame's buttons: " .. list, 0) end
  if focus ~= "Resume" then error("the semantic frame's focus: " .. tostring(focus), 0) end
  -- Frozen: the colony's clock stands still; the speed and pause keys and
  -- the view's arrows do nothing.
  local clock = t.expect:field("@colony", "Colony.clock"):get()
  local at, pan = view(t)
  if pan ~= "" then error("the view's arrows are the menu's", 0) end
  press(t, "speed_10")
  press(t, "pause")
  t.input:hold("pan", { 1, 0 }, 60)
  t:step(120)
  t.expect:field("@colony", "Colony.clock"):near(clock, 1e-9)
  t.expect:field("@colony", "Colony.paused"):eq(false)
  if t:status().speed ~= 1 then error("the speed keys wait: " .. tostring(t:status().speed), 0) end
  local now = view(t)
  if now[1] ~= at[1] or now[2] ~= at[2] then error("the view stands still", 0) end
  return clock
end

scenario "the menu stops the colony and its buttons work" {
  scene = "valley", seed = 1,
  run = function(t)
    local clock = opens_and_stands_still(t)
    -- Escape closes it, and the colony goes on.
    key(t, "Escape")
    local open = menu(t)
    if open then error("Escape closes it", 0) end
    t.expect:events("menu.closed"):count():eq(1)
    local _, pan = view(t)
    if pan ~= "pan" then error("closed, the arrows move the view again", 0) end
    t:step(60)
    t.expect:field("@colony", "Colony.clock"):gt(clock + 0.5)
    -- Again, and Resume closes it.
    key(t, "Escape")
    press(t, "ui_confirm")
    t.expect:events("menu.resume"):count():eq(1)
    local open = menu(t)
    if open then error("Resume closes it", 0) end
  end,
}

scenario "the menu and the inspector take turns with Escape and Tab" {
  scene = "valley", seed = 1,
  run = function(t)
    t:step(2)
    -- The inspector open: Escape closes it, and opens no menu.
    key(t, "Tab")
    if not inspector(t) then error("Tab opens the inspector", 0) end
    key(t, "Escape")
    local open = menu(t)
    if open or inspector(t) then error("Escape closes the inspector first, and only it", 0) end
    -- Closed: Escape opens the menu, and Tab then does nothing.
    key(t, "Escape")
    local open = menu(t)
    if not open then error("then Escape opens the menu", 0) end
    key(t, "Tab")
    if inspector(t) then error("Tab does nothing while the menu is open", 0) end
    local open = menu(t)
    if not open then error("and the menu stays", 0) end
    -- Backspace (cancel) closes the menu too; then Tab opens the inspector.
    key(t, "Backspace")
    local open = menu(t)
    if open then error("cancel closes the menu from its first page", 0) end
    key(t, "Tab")
    if not inspector(t) then error("closed, Tab opens the inspector again", 0) end
  end,
}

scenario "the menu's settings scale the text and turn contrast and fullscreen on" {
  scene = "valley", seed = 1,
  run = function(t)
    -- The HUD line is written twice a second.
    t:step(31)
    local hud = t:call("world.query", { select = { "UiRect.size" }, where = { has = { "Hud" }, ["Hud.kind"] = "line" } }).rows[1][1]
    key(t, "Escape")
    -- Down to Settings, the third button ({0, -1} is down).
    press(t, "ui_move", { 0, -1 })
    press(t, "ui_move", { 0, -1 })
    focused(t, "menu.settings")
    press(t, "ui_confirm")
    local _, page = menu(t)
    if page ~= "settings" then error("Settings: " .. tostring(page), 0) end
    focused(t, "menu.text")
    local function says(name)
      return t:call("world.query", { select = { "UiText.shown" }, where = { has = { "MenuSetting" }, ["MenuSetting.name"] = name } }).rows[1][1]
    end
    local function setting(path)
      return t:call("world.query", { select = { path }, where = { has = { "UiSettings" } } }).rows[1][1]
    end
    if says("text") ~= "Text size: 100%" then error("the text size: " .. tostring(says("text")), 0) end
    -- 125%: every text grows, the HUD line's too.
    press(t, "ui_confirm")
    if math.abs(setting("UiSettings.text_scale") - 1.25) > 0.001 then error("125%", 0) end
    if says("text") ~= "Text size: 125%" then error("the words say so: " .. tostring(says("text")), 0) end
    local now = t:call("world.query", { select = { "UiRect.size" }, where = { has = { "Hud" }, ["Hud.kind"] = "line" } }).rows[1][1]
    if not (now[2] > hud[2]) then error(string.format("the HUD line grows: %.1f to %.1f", hud[2], now[2]), 0) end
    -- High contrast, then fullscreen.
    press(t, "ui_move", { 0, -1 })
    press(t, "ui_confirm")
    if setting("UiSettings.high_contrast") ~= true then error("high contrast", 0) end
    if says("contrast") ~= "High contrast: on" then error("the words: " .. tostring(says("contrast")), 0) end
    press(t, "ui_move", { 0, -1 })
    press(t, "ui_confirm")
    if setting("UiSettings.fullscreen") ~= true then error("fullscreen", 0) end
    if says("fullscreen") ~= "Fullscreen: on" then error("the words: " .. tostring(says("fullscreen")), 0) end
    local list, focus = buttons(t)
    if list ~= "Text size: 125% | High contrast: on | Fullscreen: on | Back" then error("the semantic frame's buttons: " .. list, 0) end
    if focus ~= "Fullscreen: on" then error("the semantic frame's focus: " .. tostring(focus), 0) end
    -- Around again: 150%, then 100%.
    press(t, "ui_move", { 0, 1 })
    press(t, "ui_move", { 0, 1 })
    focused(t, "menu.text")
    press(t, "ui_confirm")
    if math.abs(setting("UiSettings.text_scale") - 1.5) > 0.001 then error("150%", 0) end
    press(t, "ui_confirm")
    if math.abs(setting("UiSettings.text_scale") - 1.0) > 0.001 then error("back to 100%", 0) end
    -- Escape backs out to the first page, and again closes the menu.
    key(t, "Escape")
    local open, page = menu(t)
    if not open or page ~= "main" then error("Escape backs out a page: " .. tostring(page), 0) end
    key(t, "Escape")
    local open = menu(t)
    if open then error("and then closes it", 0) end
  end,
}

scenario "the menu's controls show the keys and rebind them" {
  scene = "valley", seed = 1,
  run = function(t)
    t:step(2)
    key(t, "Escape")
    press(t, "ui_move", { 0, -1 })
    focused(t, "menu.controls")
    press(t, "ui_confirm")
    local _, page = menu(t)
    if page ~= "controls" then error("Controls: " .. tostring(page), 0) end
    local function binding(action)
      local q = t:call("world.query", {
        select = { "UiText.shown", "InputBinding.listening", "UiButton.focused" },
        where = { has = { "InputBinding" }, ["InputBinding.action"] = action },
      })
      return q.rows[1][1], q.rows[1][2], q.rows[1][3]
    end
    local want = {
      speed_1 = "Play at 1x: 1", speed_3 = "Play at 3x: 2", speed_10 = "Play at 10x: 3",
      pause = "Pause the colony: space", inspect = "Inspector: tab", menu = "Menu: escape",
    }
    for action, words in pairs(want) do
      local shown = binding(action)
      if shown ~= words then error(action .. ": " .. tostring(shown), 0) end
    end
    local _, _, first = binding("speed_1")
    if not first then error("the first key has the focus", 0) end
    local list, focus = buttons(t)
    local all = "Play at 1x: 1 | Play at 3x: 2 | Play at 10x: 3 | Pause the colony: space | Inspector: tab | Menu: escape | Back"
    if list ~= all then error("the semantic frame's buttons: " .. list, 0) end
    if focus ~= "Play at 1x: 1" then error("the semantic frame's focus: " .. tostring(focus), 0) end
    -- Pressed, it waits for a key.
    press(t, "ui_confirm")
    local shown, listening = binding("speed_1")
    if not listening or shown ~= "Play at 1x: press a key" then error("waiting: " .. tostring(shown), 0) end
    -- The key (here through the protocol, as the window hands one over).
    t:call("input.bind", { action = "speed_1", bindings = { "4" } })
    t:step(2)
    local shown, listening = binding("speed_1")
    if listening or shown ~= "Play at 1x: 4" then error("bound: " .. tostring(shown), 0) end
    -- Escape while one waits cancels the wait, and only that: the key
    -- stays, and so does the page.
    press(t, "ui_move", { 0, -1 })
    press(t, "ui_confirm")
    local _, listening = binding("speed_3")
    if not listening then error("3x waits for a key", 0) end
    key(t, "Escape")
    local shown, listening = binding("speed_3")
    if listening or shown ~= "Play at 3x: 2" then error("Escape cancels the wait: " .. tostring(shown), 0) end
    local open, page = menu(t)
    if not open or page ~= "controls" then error("and leaves the page open: " .. tostring(page), 0) end
    t.expect:events("input.rebind_cancelled", { action = "speed_3" }):count():eq(1)
    -- Backspace: the first page again.
    key(t, "Backspace")
    local open, page = menu(t)
    if not open or page ~= "main" then error("cancel backs out a page: " .. tostring(page), 0) end
    -- Put the key back for the other tests' sake (bindings live in the
    -- daemon's map, not the world).
    t:call("input.bind", { action = "speed_1", bindings = { "1" } })
  end,
}

scenario "the stress scene's menu stops its two hundred colonists" {
  scene = "stress", seed = 1,
  run = function(t)
    local clock = opens_and_stands_still(t)
    press(t, "ui_confirm")
    local open = menu(t)
    if open then error("Resume closes it", 0) end
    t:step(30)
    t.expect:field("@colony", "Colony.clock"):gt(clock)
  end,
}
