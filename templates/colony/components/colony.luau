-- The game's own components. Each field has a type, a default, and a doc;
-- `tune = true` puts it in the editor's tuning panel, so it can be changed
-- while the colony runs. The engine brings the rest: Worker and Job (the
-- job system), NavAgent and NavGrid (walking), Sprite, Camera, the UI.
--
-- To change first: Colony's rules (how fast colonists get hungry) and a
-- Source's yield and regrowth (prefabs/bush.toml, prefabs/tree.toml).

component "Colony" {
  doc = "The colony as a whole (the @colony entity, which is also the stockpile): its calendar and the rules everyone lives by",
  day = field.u32 { default = 1, doc = "The day it is, from 1 (systems/day.luau turns it)" },
  clock = field.f64 { doc = "The colony's own seconds since it was founded: they stand still while the game is paused, and the day and the hour follow them" },
  hunger_per_day = field.f32 { default = 1, range = {0, 10}, tune = true, doc = "Food a colonist eats in a day" },
  eat_below = field.f32 { default = 0.5, range = {0, 1}, tune = true, doc = "A colonist goes to eat once Colonist.fed drops below this, and eats their fill" },
  dusk = field.f32 { default = 0.625, range = {0.1, 1}, tune = true, doc = "How far into a day night falls: the day starts at dawn, 06:00, so 0.625 is 21:00. The meadow darkens until the day turns; nobody sleeps yet" },
  paused = field.bool { doc = "Paused by the player: the colony stands still while the camera and the HUD still answer" },
}

component "Stockpile" {
  doc = "What the colony has stored. Haulers bring goods in; the hungry eat from it. A field per kind of goods, named as Source.gives names it",
  food = field.f32 { default = 0, doc = "Food stored: a colonist eats Colony.hunger_per_day a day" },
  wood = field.f32 { default = 0, doc = "Wood stored (nothing uses it yet: building is a good next step)" },
}

component "Colonist" {
  doc = "A person of the colony. Their Worker (an engine component) holds their skills and the job they have",
  name = field.str { doc = "Their name" },
  fed = field.f32 { default = 1, range = {0, 1}, doc = "How fed: 1 full, 0 starved (they die)" },
  meal = field.entity { doc = "Their eat job, while there is one (systems/needs.luau posts it)" },
}

component "Source" {
  doc = "Something that gives goods when worked: a berry bush (food) or a tree (wood). Ripe, it asks for a worker (a job); worked, it drops its goods on the ground and grows back",
  job_kind = field.str { default = "gather", doc = "The kind of job that works it, a JobKind of scenes/main/jobs.toml (gather, chop)" },
  gives = field.str { default = "food", doc = "What it drops: a Stockpile field's name (food, wood)" },
  yield = field.f32 { default = 2, range = {0, 100}, tune = true, doc = "How much it drops, on average, each time it is worked" },
  vary = field.f32 { default = 0.25, range = {0, 1}, tune = true, doc = "How much a drop varies, drawn from the seed: 0.25 is 75 to 125 percent of yield" },
  regrow_days = field.f32 { default = 1, range = {0.05, 30}, tune = true, doc = "Days it takes to grow back once worked" },
  growth = field.f32 { default = 1, range = {0, 1}, doc = "1 ripe; below 1, growing back" },
  ripe_color = field.str { default = "#C2405A", doc = "Its Sprite's colour while ripe" },
  bare_color = field.str { default = "#4E7A3A", doc = "Its Sprite's colour while it grows back" },
  drop_color = field.str { default = "#D9604A", doc = "The colour of the goods it drops" },
  job = field.entity { doc = "Its job, while there is one" },
}

return component "Item" {
  doc = "Goods lying on the ground until a hauler carries them to the stockpile (systems/haul.luau)",
  gives = field.str { default = "food", doc = "What it is: a Stockpile field's name (food, wood)" },
  count = field.f32 { default = 1, doc = "How much" },
  job = field.entity { doc = "Its haul job, while there is one" },
}
