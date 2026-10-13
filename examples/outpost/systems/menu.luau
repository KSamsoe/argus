-- The menu (phase 5 item 2; see components/menu.luau). Escape opens it in
-- the middle of the screen and the game stands still, the colony's clock
-- and its speed and pause keys too; the view stays where it is. Escape (or
-- Backspace, or Back) backs out a page at a time, and from the first page
-- closes it, as Resume does.
--
-- One panel at a time with the inspector (systems/inspector.luau): with
-- the inspector open, Escape is the inspector's and backs out of it, and
-- the menu opens once it is closed; with the menu open, Tab does nothing.
--
-- The menu is made of UI entities like any other tree, built here the
-- first tick there is none (on load, before the player's settings are put
-- into its UiSettings): a save from before it (the first week's) gets one
-- too. Closed, its systems look at one flag a tick.

local CONTROLS = { "speed_1", "speed_3", "speed_10", "pause", "inspect", "menu" }
local SIZES = { 1.0, 1.25, 1.5 }

local ui = require("ui")
local spawn = ui.spawn

local function button(q, parent, event, key, align)
  return ui.button(q, parent, event, key, vector.create(18, 7, 0), align)
end

local function page(q, root, name)
  local p = spawn(q, root, { "UiPanel", "MenuPage" })
  local n = p:get("UiNode")
  n.pad = vector.create(28, 22, 0)
  n.gap = 8
  n.min = vector.create(320, 0, 0)
  n.visible = name == "main"
  local panel = p:get("UiPanel")
  panel.color = "panel"
  panel.border = 2
  p:get("MenuPage").name = name
  return p
end

local function title(q, parent, key)
  local e = spawn(q, parent, { "UiText" })
  e:get("UiNode").margin = vector.create(0, 6, 0)
  local t = e:get("UiText")
  t.key = key
  t.size = 18
  t.align = "center"
end

system "sim.colony.menu.build" {
  stage = "sim",
  on_load = true,
  reads = { "Menu" },
  writes = { "UiRoot", "UiNode", "UiPanel", "UiText", "UiButton", "UiSettings", "InputBinding", "Parent", "Menu", "MenuPage", "MenuSetting" },
  doc = "Builds the menu once (while there is no Menu): a root over the whole screen, hidden; its first page (a title, Resume, Controls, Settings); a controls page with a button per action that shows its key and rebinds it; and a settings page, which holds the game's UiSettings, with text size, high contrast, fullscreen, and Back.",
  run = function(q)
    if q:count("Menu") > 0 then return end
    local root = q:spawn({ "UiRoot", "UiNode", "UiPanel", "Menu" })
    -- Over the HUD and the inspector.
    root:get("UiRoot").layer = 30
    local n = root:get("UiNode")
    n.justify = "center"
    n.align = "center"
    n.visible = false
    root:get("UiPanel").color = "#05070ACC"

    local main = page(q, root, "main")
    title(q, main, "menu.title")
    button(q, main, "menu.resume", "menu.resume")
    button(q, main, "menu.controls", "menu.controls")
    button(q, main, "menu.settings", "menu.settings")

    local controls = page(q, root, "controls")
    title(q, controls, "controls.title")
    for _, action in ipairs(CONTROLS) do
      local b = button(q, controls, "", "controls." .. action, "left")
      b:insert("InputBinding", { action = action })
    end
    button(q, controls, "menu.back", "menu.back")

    local settings = page(q, root, "settings")
    settings:insert("UiSettings")
    title(q, settings, "settings.title")
    for _, name in ipairs({ "text", "contrast", "fullscreen" }) do
      local b = button(q, settings, "menu." .. name, nil, "left")
      b:insert("MenuSetting", { name = name })
    end
    button(q, settings, "menu.back", "menu.back")
  end,
}

-- The menu's root, or nil before it is built.
local function menu_of(q)
  local id = nil
  for e in q:each("Menu") do id = e:id() end
  return if id ~= nil then q:entity(id) else nil
end

-- One page shown, the others hidden.
local function show(q, name)
  for e, pg in q:each("MenuPage") do
    e:get("UiNode").visible = pg.name == name
  end
end

-- The player's view (not a vignette's camera) is still while the menu is
-- open: the arrows are the menu's.
local function view(q, open)
  for e, cam in q:each("Camera") do
    if not e:has("Vignette") then
      cam.pan_action = if open then "" else "pan"
    end
  end
end

local function close(q, node)
  node.visible = false
  view(q, false)
  q:emit("menu.closed", {})
end

system "input.colony.menu" {
  stage = "input",
  after = { "input.actions" },
  unfrozen = true,
  reads = { "Actions", "Menu", "MenuPage", "Inspector", "Vignette" },
  writes = { "UiNode", "Camera" },
  doc = "Opens the menu on the menu action (Escape) unless the inspector is open, on its first page; while it is open, the menu or cancel action backs out a page, and from the first page closes it; freezes the rest of each tick while it is open (the UI still runs), and stills the view's panning.",
  run = function(q)
    local menu = menu_of(q)
    if menu == nil then return end
    local colony = q:entity("@colony")
    if not colony:has("Actions") then return end
    local a = colony:get("Actions")
    local node = menu:get("UiNode")
    if not node.visible then
      if not a.menu_pressed then return end
      -- The inspector open keeps Escape: it backs out of it first.
      for _, ins in q:each("Inspector") do
        if ins.open then return end
      end
      node.visible = true
      show(q, "main")
      view(q, true)
      q:emit("menu.opened", {})
    elseif a.menu_pressed or a.ui_cancel_pressed then
      local first = true
      for e, pg in q:each("MenuPage") do
        if pg.name ~= "main" and e:get("UiNode").visible then first = false end
      end
      if first then
        close(q, node)
      else
        show(q, "main")
      end
    end
    if node.visible then
      q:freeze()
    end
  end,
}

return system "sim.colony.menu" {
  stage = "sim",
  after = { "ui.input" },
  unfrozen = true,
  reads = { "Menu", "MenuPage", "MenuSetting", "Vignette" },
  writes = { "UiNode", "UiSettings", "UiText", "Camera" },
  doc = "The menu's buttons: Resume closes it; Controls and Settings show their page, Back the first page again; the settings page's buttons cycle the text size and turn high contrast and fullscreen on and off in the menu's UiSettings, and their words say so. Nothing while the menu is closed.",
  run = function(q)
    local menu = menu_of(q)
    if menu == nil then return end
    local node = menu:get("UiNode")
    if not node.visible then return end
    if #q:events("menu.resume") > 0 then
      close(q, node)
      return
    end
    if #q:events("menu.controls") > 0 then show(q, "controls") end
    if #q:events("menu.settings") > 0 then show(q, "settings") end
    if #q:events("menu.back") > 0 then show(q, "main") end

    -- The settings, in the settings page's UiSettings.
    local s = nil
    for e, pg in q:each("MenuPage") do
      if pg.name == "settings" then s = e:get("UiSettings") end
    end
    if #q:events("menu.text") > 0 then
      local next = SIZES[1]
      for i, v in ipairs(SIZES) do
        if math.abs(s.text_scale - v) < 0.01 then
          next = SIZES[i % #SIZES + 1]
        end
      end
      s.text_scale = next
    end
    if #q:events("menu.contrast") > 0 then
      s.high_contrast = not s.high_contrast
    end
    if #q:events("menu.fullscreen") > 0 then
      s.fullscreen = not s.fullscreen
    end
    local function on(b)
      return q:text(b and "settings.on" or "settings.off")
    end
    local words = {
      text = q:text("settings.text", { percent = math.floor(s.text_scale * 100 + 0.5) }),
      contrast = q:text("settings.contrast", { state = on(s.high_contrast) }),
      fullscreen = q:text("settings.fullscreen", { state = on(s.fullscreen) }),
    }
    for e, ms in q:each("MenuSetting") do
      local t = e:get("UiText")
      if t.text ~= words[ms.name] then
        t.text = words[ms.name]
      end
    end
  end,
}
