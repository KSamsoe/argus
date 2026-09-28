-- A door (the lanterns mod): stepping into it takes the hero to `to`, the
-- far side, in another room.
return component "lanterns_Door" {
  doc = "A door to another room: the hero touching it is set down at `to`",
  to = field.vec2 { doc = "Where the hero comes out, in world units" },
}
