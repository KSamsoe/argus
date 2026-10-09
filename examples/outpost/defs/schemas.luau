-- Outpost's data tables (section 4: "Outpost is mostly defs; an agent
-- balancing its economy edits tables and runs seed sweeps, never touching
-- a scene"). Rows are in the .toml files beside this one; scripts read them
-- with q:def("item:meal") and q:defs("jobs").

def "items" {
  doc = "What there is: goods a stockpile holds",
  prefix = "item",
  key = "id",
  id = field.str {},
  name = field.str { doc = "What a player calls it" },
  food = field.f32 { default = 0, range = {0, 10}, tune = true, doc = "Food it is worth eaten, in meals (a meal is 1)" },
  color = field.str { default = "#FFFFFF", doc = "Its placeholder's colour" },
}

def "recipes" {
  doc = "What a station makes from what",
  prefix = "recipe",
  key = "id",
  id = field.str {},
  output = field.ref { doc = "The item made" },
  count = field.f32 { default = 1, range = {0, 100}, doc = "How many are made" },
  input = field.ref { doc = "The item used up" },
  input_count = field.f32 { default = 1, range = {0, 100}, tune = true, doc = "How many are used up" },
  work = field.f32 { default = 1, range = {0, 600}, tune = true, doc = "Seconds of a cook's work" },
}

def "jobs" {
  doc = "The kinds of job and how the colony weighs them at its founding (each becomes a JobKind, whose priority is a tune field while the game runs)",
  prefix = "job",
  key = "id",
  id = field.str {},
  priority = field.f32 { default = 0, range = {-100, 100}, tune = true, doc = "Higher first" },
  skill = field.str { doc = "The skill it needs; empty: anyone" },
  work = field.f32 { default = 1, range = {0, 100000}, tune = true, doc = "Seconds of work" },
}

return def "founders" {
  doc = "The colonists a colony is founded with",
  prefix = "founder",
  key = "id",
  id = field.str {},
  skills = field.str { doc = "What they can do (farm, cook, chop); everyone hauls" },
}
