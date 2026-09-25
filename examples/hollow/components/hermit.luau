-- The hermit at the cave mouth (phase 3 item 4): someone to talk to. His
-- conversation is data (defs/dialogue.toml); these are the facts about
-- him it reads and writes.
return component "Hermit" {
  doc = "The hermit at the cave mouth: what he knows of the hero, which his conversation (defs/dialogue.toml) reads and writes",
  met = field.bool { default = false, doc = "Has talked with the hero" },
  has_gem = field.bool { default = false, doc = "The hero gave him the gem" },
}
