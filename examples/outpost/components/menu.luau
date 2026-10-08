-- The menu (phase 5 item 2; section 12: "every game's pause menu"):
-- Escape opens it over the valley and the game stands still. Resume,
-- Controls (each action's key, rebound to the next key pressed), and
-- Settings (text size, high contrast, fullscreen). Built from UI entities
-- by systems/menu.luau, as the inspector is.

component "Menu" {
  doc = "The menu's root: shown while the menu is open (UiNode.visible); the game is frozen while it is",
}

component "MenuPage" {
  doc = "One page of the menu: main (Resume, Controls, Settings), controls, or settings; one is shown at a time",
  name = field.str { doc = "main, controls, or settings" },
}

return component "MenuSetting" {
  doc = "A settings button whose words say the setting's state, written by systems/menu.luau",
  name = field.str { doc = "text, contrast, or fullscreen" },
}
