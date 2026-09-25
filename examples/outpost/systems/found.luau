-- The colony's founding: six colonists, fed, before the first tick.

local NAMES = { "Ada", "Bram", "Cleo", "Dov", "Esme", "Finn" }

return system "sim.colony.found" {
  stage = "sim",
  on_load = true, -- the colony is there before the first tick
  reads = { "Colonist" },
  writes = { "Colonist", "Needs" },
  doc = "Founds the colony at the load: six colonists, fed.",
  run = function(q)
    if q:count("Colonist") > 0 then return end
    for _, name in ipairs(NAMES) do
      local c = q:spawn({ "Colonist", "Needs" })
      c:get("Colonist").name = name
    end
  end,
}
