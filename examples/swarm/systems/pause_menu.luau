-- The pause menu's buttons (phase 5 item 2): Resume closes it; Restart
-- closes it and starts the run again from wave 1 (the swarm and the
-- bullets gone, the player whole again where every scene starts it);
-- Controls and Settings show their page in its place, and Back (or the
-- cancel action) brings it back. The settings page cycles the text size
-- and turns high contrast and fullscreen on and off, in the UiSettings of
-- @pause/settings; its words come from the string table. Closed, it looks
-- at one flag and goes.
local SIZES = { 1.0, 1.25, 1.5 }

-- The run again from the start: what the scene's files and the first tick
-- would make of it.
local function restart(q)
  local pe = q:entity("@player")
  local pl = pe:get("Player")
  pl.kills = 0
  pl.dead = false
  pl.cooldown = 0
  pl.aim = 0
  pl.iframes = 0
  local hp = pe:get("Health")
  hp.cur = hp.max
  pe:get("Transform").pos = vector.create(0, 0, 0)
  local doomed = {}
  for e in q:each("Enemy") do table.insert(doomed, e:id()) end
  for e in q:each("Bullet") do table.insert(doomed, e:id()) end
  for _, id in ipairs(doomed) do q:entity(id):despawn() end
  -- No wave yet: sim.waves spawns the first on the next tick.
  local d = q:entity("@director"):get("Director")
  d.wave = 0
  d.timer = 0
  d.alive = 0
  d.cleared = false
  q:emit("run.restarted", {})
end

return system "sim.pause_menu" {
  stage = "sim",
  after = { "ui.input" },
  unfrozen = true,
  reads = { "UiNode", "UiSettings", "UiText", "Player", "Health", "Transform", "Director", "Enemy", "Bullet" },
  writes = { "UiNode", "UiSettings", "UiText", "Player", "Health", "Transform", "Director", "Enemy", "Bullet" },
  doc = "The pause menu's buttons: Resume closes it; Restart closes it and starts the run again from wave 1 (enemies and bullets despawned, the player whole at the start); Controls and Settings swap their page in, and Back or cancel swaps it out; the settings page's buttons cycle the text size and turn high contrast and fullscreen on and off. Nothing while the menu is closed.",
  run = function(q)
    local menu = q:entity("@pause"):get("UiNode")
    if not menu.visible then return end
    local panel = q:entity("@pause/panel"):get("UiNode")
    local pages = {
      controls = q:entity("@pause/controls"):get("UiNode"),
      settings = q:entity("@pause/settings"):get("UiNode"),
    }
    local resume = #q:events("pause.resume") > 0
    local again = #q:events("pause.restart") > 0
    if resume or again then
      menu.visible = false
      q:emit("pause.closed", {})
      if again then restart(q) end
      return
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

    -- The settings.
    local s = q:entity("@pause/settings"):get("UiSettings")
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
      ["@pause/settings/text"] = q:text("settings.text", { percent = math.floor(s.text_scale * 100 + 0.5) }),
      ["@pause/settings/contrast"] = q:text("settings.contrast", { state = on(s.high_contrast) }),
      ["@pause/settings/fullscreen"] = q:text("settings.fullscreen", { state = on(s.fullscreen) }),
    }
    for id, text in pairs(words) do
      local t = q:entity(id):get("UiText")
      if t.text ~= text then
        t.text = text
      end
    end
  end,
}
