-- The lantern keeper (the lanterns mod): who systems/keeper.luau lets the
-- hero talk to.
return component "lanterns_Keeper" {
  doc = "The lantern keeper in the vigil, who lights the lanterns",
  met = field.bool { default = false, doc = "Has talked with the hero" },
}
