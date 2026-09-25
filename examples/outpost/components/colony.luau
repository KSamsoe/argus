-- Outpost, the colony sim (§0's fourth reference game): its first skeleton,
-- phase 3 item 0. Colonists eat from a stockpile the fields fill; a colonist
-- who goes too long without food dies. Jobs, stockpiles of more than food,
-- and the day and night come in item 3.

component "Colony" {
  doc = "The colony as a whole: the day it is on, and the rules of its economy (tune fields: a slider is a hypothesis about balance, section 10)",
  day = field.u32 { doc = "Days since the colony was founded; the game saves itself when one turns" },
  hunger_per_day = field.f32 { default = 1, range = {0, 10}, tune = true, doc = "Meals a colonist needs a day" },
  farm_per_day = field.f32 { default = 8, range = {0, 100}, tune = true, doc = "Meals the fields yield a day, before the weather" },
  weather = field.f32 { default = 0.25, range = {0, 1}, tune = true, doc = "How much a day's harvest varies: 0.25 is anywhere from 75 to 125 percent" },
  eat_below = field.f32 { default = 0.5, range = {0, 1}, tune = true, doc = "A colonist eats once fed drops below this, filling up" },
}

component "Stockpile" {
  doc = "What the colony has stored",
  food = field.f32 { default = 0, doc = "Meals stored" },
}

component "Colonist" {
  doc = "A person of the colony",
  name = field.str { doc = "Their name" },
}

return component "Needs" {
  doc = "What a colonist lacks",
  fed = field.f32 { default = 1, range = {0, 1}, doc = "How fed: 1 full, 0 starving (death)" },
}
