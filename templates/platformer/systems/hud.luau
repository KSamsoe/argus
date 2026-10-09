-- The line at the top of the screen (scenes/level/hud.toml): the coins and
-- the falls while the hero plays, and how the run went once it reaches the
-- flag. It runs in the late stage, after the sim has settled this tick.
return system "late.hud" {
  stage = "late",
  reads = { "Hero", "Coin" },
  writes = { "UiText" },
  doc = "Writes the HUD line (@hud's UiText) from the hero's Hero: coins and falls while running, the result and how to play again once the flag is reached.",
  run = function(q)
    local h = q:entity("@player"):get("Hero")
    local total = q:count("Coin")
    local text
    if h.done then
      text = string.format("You reached the flag! Coins %d/%d, falls %d. Press R to play again.", h.coins, total, h.falls)
    else
      text = string.format("Coins %d/%d    Falls %d", h.coins, total, h.falls)
    end
    local ui = q:entity("@hud"):get("UiText")
    -- Written only when it changes, so the change shows in obs.diff once.
    if ui.text ~= text then
      ui.text = text
    end
  end,
}
