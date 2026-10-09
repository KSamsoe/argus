-- What Hollow tells its platform (phase 5 item 9): the deepest row of the
-- deep the hero has reached, for the deep diver's progress, and the room
-- its rich presence names. On the HUD entity, with the run.
return component "Feats" {
  doc = "What the platform has been told: the deepest row of the deep reached (the deep diver's progress) and the room the rich presence names",
  deepest = field.u32 { default = 0, doc = "The deepest row of the deep the hero has reached, 0 to 9" },
  room = field.str { default = "", doc = "The room the rich presence last named" },
}
