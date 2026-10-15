-- Puppet's icon (argus.toml's [game] icon): a wooden puppet's face, 16 x
-- 16, drawn as text (one character a pixel). Change a row or a colour and
-- the next build's icon follows.

local key = {
  o = "#2A1B14",                   -- the outline
  W = "#D9A066", w = "#B07A45",    -- the wood, and its shade
  E = "#2A1B14", e = "#FFFFFF",    -- the eyes and their light
  C = "#E0605A",                   -- the cheeks
  M = "#7A3B2E",                   -- the mouth's hinge
}

local face = {
  "....oooooooo....",
  "...oWWWWWWWWo...",
  "..oWWWWWWWWWWo..",
  ".oWWWWWWWWWWWWo.",
  ".oWWEeWWWWEeWWo.",
  ".oWWEEWWWWEEWWo.",
  ".oWWWWWWWWWWWWo.",
  ".oWCCWWWWWWCCWo.",
  ".oWWWWWWWWWWWWo.",
  ".owMMMMMMMMMMwo.",
  ".owwWWWWWWWWwwo.",
  "..owWWWWWWWWwo..",
  "..owwWWWWWWwwo..",
  "...owwWWWWwwo...",
  "....oowwwwoo....",
  "......oooo......",
}

local c = art.canvas(16, 16)
c:rows(0, 0, face, key)
return art.sprite{ frames = { c } }
