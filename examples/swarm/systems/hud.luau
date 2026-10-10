-- The HUD (prefabs/hud.toml): the wave and the enemies left, the hero's HP,
-- and the kills, the words filled from the world each layout by their
-- string's holes (strings/en.toml, hud.*). This system does what holes
-- cannot: lights the HP pips, shows the wave's banner for a moment when a
-- wave spawns (wave.spawned), and shows the panel that says the run is over
-- while the hero is dead. It reads gameplay and writes only UI (and its own
-- Hud), a handful of fields a tick, and nothing while they stand.

local PIPS = 10
local BANNER = 2.4 -- seconds the wave banner shows
local FADE = 0.6 -- of them, the last fading out

return system "late.hud" {
  stage = "late",
  reads = { "Hud", "Player", "Health", "UiNode", "UiPanel", "UiText" },
  writes = { "Hud", "UiNode", "UiPanel", "UiText" },
  doc = "The HUD's moving parts: the HP pips lit as Health.cur is of Health.max, the wave banner shown for 2.4 s from each wave.spawned (its line saying how many, and how many dash), and the run-over panel shown while Player.dead. Writes only UI and Hud.",
  run = function(q)
    local hud = q:entity("@hud"):get("Hud")
    local pe = q:entity("@player")
    local hp = pe:get("Health")

    local lit = if hp.max > 0 then math.ceil(PIPS * math.max(hp.cur, 0) / hp.max) else 0
    if lit ~= hud.lit then
      hud.lit = lit
      for i = 1, PIPS do
        q:entity("@hud/hp/pips/p" .. i):get("UiPanel").color = if i <= lit then "hp" else "hp_off"
      end
    end

    local spawned = q:events("wave.spawned")
    if #spawned > 0 then
      local ev = spawned[#spawned]
      hud.banner = BANNER
      local key = if ev.dashers > 0 then "hud.banner_dashers" else "hud.banner_swarm"
      q:entity("@hud/banner/line"):get("UiText").text = q:text(key, { count = ev.count, dashers = ev.dashers })
    end
    local banner = q:entity("@hud/banner"):get("UiNode")
    if hud.banner > 0 then
      hud.banner = math.max(0, hud.banner - q.dt)
      banner.visible = hud.banner > 0
      banner.opacity = math.min(1, hud.banner / FADE)
    end

    local over = q:entity("@hud/over"):get("UiNode")
    local dead = pe:get("Player").dead
    if over.visible ~= dead then
      over.visible = dead
    end
  end,
}
