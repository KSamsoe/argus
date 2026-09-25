-- The pause menu's buttons (phase 3 item 1): Resume closes it; Restart the
-- run closes it and asks sim.run to start again; Controls and Settings
-- show their page in its place, and Back (or the cancel action) brings it
-- back. The settings page cycles the text size and the language, and
-- turns high contrast and fullscreen on and off, in @settings's
-- UiSettings. Its words come from the string tables (phase 3 item 4), in
-- the language chosen.
local SIZES = { 1.0, 1.25, 1.5 }

return system "sim.pause_menu" {
  stage = "sim",
  after = { "ui.input" },
  unfrozen = true,
  reads = { "UiNode", "UiSettings", "UiText" },
  writes = { "UiNode", "Run", "UiSettings", "UiText" },
  doc = "The pause menu's buttons: Resume closes it; Restart the run closes it and asks for a new run; Controls and Settings swap their page in, and Back or cancel swaps it out; the settings page's buttons cycle the text size and the language (every string table there is) and turn high contrast and fullscreen on and off.",
  run = function(q)
    local menu = q:entity("@pause"):get("UiNode")
    local panel = q:entity("@pause.panel"):get("UiNode")
    local pages = {
      controls = q:entity("@controls"):get("UiNode"),
      settings = q:entity("@settings"):get("UiNode"),
    }
    local resume = #q:events("pause.resume") > 0
    local restart = #q:events("pause.restart") > 0
    if resume or restart then
      menu.visible = false
      q:emit("pause.closed", {})
    end
    if restart then
      q:entity("@hud"):get("Run").restart = true
    end
    for name, page in pairs(pages) do
      if #q:events("pause." .. name) > 0 then
        panel.visible = false
        page.visible = true
      elseif page.visible and (#q:events(name .. ".back") > 0 or #q:events("ui.cancel") > 0) then
        page.visible = false
        panel.visible = true
      end
    end
    -- A closed menu opens on its first page next time.
    if not menu.visible then
      for _, page in pairs(pages) do
        page.visible = false
      end
      panel.visible = true
    end

    -- The settings.
    local s = q:entity("@settings"):get("UiSettings")
    if #q:events("settings.text") > 0 then
      local next = SIZES[1]
      for i, v in ipairs(SIZES) do
        if math.abs(s.text_scale - v) < 0.01 then
          next = SIZES[i % #SIZES + 1]
        end
      end
      s.text_scale = next
    end
    if #q:events("settings.contrast") > 0 then
      s.high_contrast = not s.high_contrast
    end
    if #q:events("settings.fullscreen") > 0 then
      s.fullscreen = not s.fullscreen
    end
    if #q:events("settings.language") > 0 then
      local langs = q:languages()
      local at = 1
      for i, l in ipairs(langs) do
        if l == s.language or (s.language == "" and i == 1) then at = i end
      end
      s.language = langs[at % #langs + 1]
    end
    local function on(b)
      return q:text(b and "settings.on" or "settings.off")
    end
    local words = {
      ["@settings.text"] = q:text("settings.text", { percent = math.floor(s.text_scale * 100 + 0.5) }),
      ["@settings.contrast"] = q:text("settings.contrast", { state = on(s.high_contrast) }),
      ["@settings.fullscreen"] = q:text("settings.fullscreen", { state = on(s.fullscreen) }),
      ["@settings.language"] = q:text("settings.language", { name = q:text("language.name") }),
    }
    for id, text in pairs(words) do
      local t = q:entity(id):get("UiText")
      if t.text ~= text then
        t.text = text
      end
    end
  end,
}
