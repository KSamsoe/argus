-- The menu's buttons (phase 3 item 1): Resume closes it; New run closes it
-- and starts again on a new first floor, drawn on this same turn. Controls
-- and Settings (phase 5 item 2) show their page in its place, and Back (or
-- the cancel action) brings it back. The settings page cycles the text
-- size and turns high contrast and fullscreen on and off, in @settings's
-- UiSettings; its words come from the string table. The saves page (2D
-- staples item 4) saves the run to a slot with what it holds, loads one,
-- and deletes one, and says what each slot holds (q:saves).
local SLOTS = 3
local SIZES = { 1.0, 1.25, 1.5 }

return system "sim.menu_buttons" {
  stage = "sim",
  after = { "ui.input" },
  unfrozen = true,
  reads = { "UiNode", "Player", "Health", "Floor", "Turn", "UiSettings", "UiText", "UiButton" },
  writes = { "UiNode", "Player", "Health", "Floor", "Turn", "UiSettings", "UiText", "UiButton" },
  doc = "The menu's buttons: Resume closes it; New run also resets the hero and the floor count, so sim.floor makes a new first floor; Controls, Settings, and Saves swap their page in, and Back or cancel swaps it out; the settings page's buttons cycle the text size and turn high contrast and fullscreen on and off; the saves page's save to, load, and delete a slot, and show what each holds.",
  run = function(q)
    local menu = q:entity("@menu"):get("UiNode")
    local panel = q:entity("@menu.panel"):get("UiNode")
    local pages = {
      controls = q:entity("@controls"):get("UiNode"),
      settings = q:entity("@settings"):get("UiNode"),
      saves = q:entity("@saves"):get("UiNode"),
    }
    local resume = #q:events("menu.resume") > 0
    local new_run = #q:events("menu.new_run") > 0
    -- A load puts back the menu as it was saved; the game goes on closed.
    local loaded = #q:events("game.loaded") > 0
    if resume or new_run or loaded then
      menu.visible = false
      q:emit("menu.closed", {})
    end
    if new_run then
      local pe = q:entity("@player")
      local pl = pe:get("Player")
      pl.floor = 0
      pl.kills = 0
      pl.dead = false
      pl.descend = false
      pl.potions = 0
      local hp = pe:get("Health")
      hp.cur = hp.max
      q:entity("@floor"):get("Floor").n = 0
      q:entity("@turn"):get("Turn").n = 0
      q:emit("run.new", {})
    end
    for name, page in pairs(pages) do
      if #q:events("menu." .. name) > 0 then
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
      return
    end

    -- The saves: a slot saved with what it holds, loaded, or deleted.
    -- A save is written, and a delete done, after this step: what this
    -- tick asked for shows at once, and q:saves() has it from the next.
    local pe = q:entity("@player")
    local pl = pe:get("Player")
    local asked = {}
    for n = 1, SLOTS do
      local slot = "slot" .. n
      if #q:events("saves.save" .. n) > 0 then
        local summary = {
          floor = pl.floor,
          turn = q:entity("@turn"):get("Turn").n,
          kills = pl.kills,
          hp = pe:get("Health").cur,
        }
        q:save(slot, summary)
        asked[slot] = { name = slot, summary = summary }
      end
      if #q:events("saves.load" .. n) > 0 then q:load(slot) end
      if #q:events("saves.delete" .. n) > 0 then
        q:delete_save(slot)
        asked[slot] = false
      end
    end
    if pages.saves.visible then
      local held = {}
      for _, s in ipairs(q:saves()) do held[s.name] = s end
      for slot, s in pairs(asked) do held[slot] = s or nil end
      for n = 1, SLOTS do
        local s = held["slot" .. n]
        local what = s and q:text("saves.held", {
          floor = s.summary.floor or 0,
          turn = s.summary.turn or 0,
          kills = s.summary.kills or 0,
        }) or q:text("saves.empty")
        local text = q:text("saves.slot", { n = n, what = what })
        local t = q:entity("@saves.slot" .. n .. ".text"):get("UiText")
        if t.text ~= text then t.text = text end
        for _, act in ipairs({ "load", "delete" }) do
          local b = q:entity("@saves.slot" .. n .. "." .. act):get("UiButton")
          if b.disabled ~= (s == nil) then b.disabled = (s == nil) end
        end
      end
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
    local function on(b)
      return q:text(b and "settings.on" or "settings.off")
    end
    local words = {
      ["@settings.text"] = q:text("settings.text", { percent = math.floor(s.text_scale * 100 + 0.5) }),
      ["@settings.contrast"] = q:text("settings.contrast", { state = on(s.high_contrast) }),
      ["@settings.fullscreen"] = q:text("settings.fullscreen", { state = on(s.fullscreen) }),
    }
    for id, text in pairs(words) do
      local t = q:entity(id):get("UiText")
      if t.text ~= text then
        t.text = text
      end
    end
  end,
}
