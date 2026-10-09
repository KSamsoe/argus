-- Hollow's achievements and rich presence (phase 5 item 9). Each is a
-- request to the platform (Steam in a release with Steam, the null
-- platform in the daemon and the tests): out of the hollow and sure-footed
-- when the run finishes, the gem finder when the gem is taken, the deep
-- diver at the deep's bottom row with progress on the way down, and the
-- room the hero is in for friends to see.
local function where(room)
  if room == "a" then
    return "In the cave"
  elseif room == "b" then
    return "Climbing the shaft"
  elseif room == "c" then
    return "In the hall"
  end
  local row = string.match(room, "deep%-%d+%-(%d+)")
  if row then
    return "In the deep, " .. row .. " rooms down"
  end
  return "In the hollow"
end

return system "late.feats" {
  stage = "late",
  reads = { "Feats", "InRoom" },
  writes = { "Feats" },
  doc = "Asks the platform for Hollow's achievements (out_of_the_hollow and sure_footed on run.finished, gem_finder on gem.taken, deep_diver at the deep's bottom row with progress on the way down) and sets the rich presence status to the hero's room.",
  run = function(q)
    for _, ev in ipairs(q:events("run.finished")) do
      q:achieve("out_of_the_hollow")
      if ev.falls == 0 then
        q:achieve("sure_footed")
      end
    end
    if #q:events("gem.taken") > 0 then
      q:achieve("gem_finder")
    end
    local hero = q:entity("@player")
    if not hero:has("InRoom") then
      return
    end
    local f = q:entity("@hud"):get("Feats")
    local room = hero:get("InRoom").room
    if room ~= f.room then
      f.room = room
      q:presence("status", where(room))
    end
    local row = tonumber(string.match(room, "deep%-%d+%-(%d+)") or "")
    if row and row > f.deepest then
      f.deepest = row
      if row >= 9 then
        q:achieve("deep_diver")
      else
        q:achieve("deep_diver", row, 9)
      end
    end
  end,
}
