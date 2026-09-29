-- The inspector (the phase 3 gate: "a menu-driven in-game inspector" for
-- Outpost's core; section 12: "Outpost's inspector panels and overview
-- screens"). A panel on the right of the screen that Tab opens: the colony
-- at a glance, its people one by one, its stores, its jobs. Built from UI
-- entities by systems/inspector.luau, like everything else of Outpost's.

component "Inspector" {
  doc = "The inspector panel: whether it is open, the page it shows, and who it is looking at",
  open = field.bool { doc = "Shown (Tab opens and closes it)" },
  page = field.str { default = "colony", doc = "The page shown: colony, people, person, stores, or jobs" },
  person = field.entity { doc = "The colonist the person page shows" },
  list = field.u32 { default = 0, doc = "Which ten of the colonists the people page lists: 0 the first ten" },
  follow = field.bool { doc = "The camera follows the person shown" },
  refresh = field.u32 { default = 0, doc = "Ticks until the page's words are written again" },
}

component "InspectPage" {
  doc = "One page of the inspector: shown while it is the inspector's page",
  name = field.str { doc = "colony, people, person, stores, or jobs" },
}

component "InspectLine" {
  doc = "A line of words on an inspector page, written by systems/inspector.luau; hidden while it has none",
  page = field.str { doc = "The page it is on" },
  n = field.u32 { doc = "Its place on the page, from 1" },
}

return component "InspectRow" {
  doc = "A button on the people page: one colonist, pressed to see them",
  n = field.u32 { doc = "Its place in the list, from 1" },
  who = field.entity { doc = "The colonist it shows (written as the list moves)" },
}
