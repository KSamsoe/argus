-- Outpost, the colony sim (section 0's fourth reference game). Phase 3
-- item 0 was its first skeleton (colonists eating from a stockpile the
-- fields filled by themselves); item 3 is its core: a valley laid out from
-- the seed, fields that grow by day, jobs (harvest, haul, cook, eat,
-- sleep) done by colonists who walk to them, stockpiles, needs, and the
-- day and night.
--
-- The first week's save (tests/saves/week1.save) was written by the
-- skeleton, so every change here says how an older row becomes a newer
-- one (section 6): Colony and Stockpile carry migrations.

component "Colony" {
  doc = "The colony as a whole: its calendar and the rules of its economy (tune fields: a slider is a hypothesis about balance, section 10)",
  version = 2,
  day = field.u32 { doc = "Days since the colony was founded; the game saves itself when one turns" },
  clock = field.f64 { doc = "The colony's own seconds, which stand still while the game is paused (day and night follow it)" },
  hunger_per_day = field.f32 { default = 1, range = {0, 10}, tune = true, doc = "Food a colonist needs a day (a meal is 1)" },
  eat_below = field.f32 { default = 0.5, range = {0, 1}, tune = true, doc = "A colonist goes to eat once fed drops below this, and eats their fill" },
  rest_per_day = field.f32 { default = 1.3, range = {0, 10}, tune = true, doc = "Rest a colonist loses in a waking day; a night's sleep gives it back" },
  grow_days = field.f32 { default = 2, range = {0.1, 30}, tune = true, doc = "Days of daylight a field takes to ripen: the growth curve's length" },
  crop_yield = field.f32 { default = 5, range = {0, 100}, tune = true, doc = "Crops a ripe field gives" },
  weather = field.f32 { default = 0.25, range = {0, 1}, tune = true, doc = "How much a day's growth varies: 0.25 is anywhere from 75 to 125 percent" },
  growth = field.f32 { default = 1, doc = "Today's growth, the weather's (written when a day turns)" },
  meals_wanted = field.f32 { default = 24, range = {0, 1000}, tune = true, doc = "Cooks stop once the stockpiles hold this many meals" },
  dusk = field.f32 { default = 0.65, range = {0.3, 1}, tune = true, doc = "How far into a day night falls; colonists sleep until the day turns" },
  haul_open = field.u32 { default = 12, range = {0, 1000}, doc = "Haul jobs posted at a time for goods lying about" },
  laid_out = field.bool { doc = "The valley is laid out: the ground, fields, stockpile, stove, beds (a save from before it was gets one)" },
  size = field.vec2 { default = {40, 24}, doc = "The valley's size in cells, when it is laid out" },
  colonists = field.u32 { default = 0, doc = "Colonists to found it with; 0: the founders of defs/founders.toml (the stress scene founds 200)" },
  stones = field.u32 { default = 0, doc = "Stones lying about when it is laid out, for haulers (the stress scene has 20,000)" },
  speed = field.f32 { default = 1, doc = "The speed the player chose: 1, 3, or 10 ticks a frame" },
  paused = field.bool { doc = "Paused by the player: nothing but the controls runs" },
  migrate = {
    -- v1 (the skeleton): the fields yielded farm_per_day by themselves, and
    -- the colony's calendar was the sim's.
    [2] = { drop = { "farm_per_day" } },
  },
}

component "Stockpile" {
  doc = "A stockpile: what the colony has stored there. Haulers bring goods in; cooks and the hungry take them out",
  version = 2,
  food = field.f32 { default = 0, doc = "Food stored, in meals' worth: its meals and crops together (section 9 sums it)" },
  meal = field.f32 { default = 0, doc = "Meals stored" },
  crop = field.f32 { default = 0, doc = "Crops stored, to be cooked or eaten raw" },
  goods = field.f32 { default = 0, doc = "Other goods stored (stone, wood)" },
  migrate = {
    -- v1 counted meals as food and stored nothing else.
    [2] = function(row)
      row.meal = row.food
      return row
    end,
  },
}

component "Colonist" {
  doc = "A person of the colony",
  version = 2,
  name = field.str { doc = "Their name" },
  bed = field.entity { doc = "Where they sleep" },
}

component "Needs" {
  doc = "What a colonist lacks",
  version = 2,
  fed = field.f32 { default = 1, range = {0, 1}, doc = "How fed: 1 full, 0 starving (death)" },
  rest = field.f32 { default = 1, range = {0, 1}, doc = "How rested: they work at 40 percent tired, full speed rested" },
  meal = field.entity { doc = "The eat job posted for them, while there is one" },
  sleep = field.entity { doc = "The night's sleep job, while there is one" },
}

component "Field" {
  doc = "A field: grows by day (Colony.grow_days, the weather), and once ripe a farmer harvests it",
  growth = field.f32 { default = 0, range = {0, 1}, doc = "0 sown, 1 ripe" },
  crop = field.ref { default = "item:crop", doc = "What it grows" },
  job = field.entity { doc = "Its harvest job, while there is one" },
}

component "Item" {
  doc = "Goods lying on the ground until a hauler takes them to a stockpile",
  kind = field.ref { default = "item:crop", doc = "What it is (defs/items.toml)" },
  count = field.f32 { default = 1, doc = "How many" },
  job = field.entity { doc = "Its haul job, while there is one" },
}

component "Station" {
  doc = "A work station: a cook makes its recipe here from what the stockpiles hold",
  recipe = field.ref { default = "recipe:meal", doc = "What is made here (defs/recipes.toml)" },
  job = field.entity { doc = "Its job, while there is one" },
}

component "Hud" {
  doc = "A line of the HUD (systems/hud.luau writes its words)",
  kind = field.str { doc = "line: the day, the food, the colonists; notice: the colony's news" },
}

component "Grave" {
  doc = "Where a colonist was lost (systems/vignette.luau)",
  name = field.str { doc = "Whose" },
  day = field.u32 { default = 0, doc = "The day they were lost" },
}

component "Vignette" {
  doc = "The camera a vignette cuts to: it follows what the vignette is about (systems/vignette.luau)",
}

return component "Bed" {
  doc = "A bed",
  owner = field.entity { doc = "Whose it is" },
}
