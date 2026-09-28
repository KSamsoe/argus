-- What the lanterns mod remembers wherever the hero is: on an entity it
-- adds to the game's always-loaded room (persistent/lanterns.state.toml),
-- since the keeper's room streams out when the hero leaves it.
return component "lanterns_Vigil" {
  doc = "The lanterns mod's state: whether the keeper has lit the lanterns",
  kindled = field.bool { default = false, doc = "The keeper lit the lanterns" },
  shown = field.bool { default = false, doc = "The kindling has played (its timeline started)" },
}
