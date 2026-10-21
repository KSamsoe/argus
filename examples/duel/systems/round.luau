-- The round: where the match is, in @arena's Match.state, and what moves
-- it on.
--
--   menu   the title menu (systems/menu.luau); its buttons, or an online
--          match starting, make it start
--   start  a new match: no points, both fighters at home        -> fight
--   fight  the round is on; a swing that lands (sim.attack)      -> point
--   point  Match.pause ticks standing still, then a new round    -> fight,
--          or, with Match.to points,                             -> won
--   won    the win screen; after Match.pause ticks, either player's
--          attack starts a new match                             -> fight
--
-- If you add state to a round (a power-up, a hazard), reset it in
-- new_round too, or it carries over.

-- Both fighters back at home, facing each other, nothing cooling down, no
-- swing out.
local function new_round(q)
  local homes = {}
  for e, f in q:each("Fighter") do
    homes[e:id()] = f.home
  end
  for e, f, t in q:each("Fighter", "Transform") do
    t.pos = f.home
    for id, other in homes do
      local d = other - f.home
      if id ~= e:id() and vector.magnitude(d) > 0 then
        f.facing = vector.normalize(d)
      end
    end
    f.dash, f.dash_wait, f.swing_wait, f.flash = 0, 0, 0, 0
    f.push = vector.create(0, 0, 0)
  end
  local swings = {}
  for e in q:each("Swing") do
    table.insert(swings, e:id())
  end
  for _, id in swings do
    q:entity(id):despawn()
  end
end

local function new_match(q, m)
  m.one, m.two, m.winner = 0, 0, 0
  new_round(q)
  m.state = "fight"
  q:emit("match.started", { online = m.online })
end

return system "sim.round" {
  stage = "sim",
  reads = { "Actions" },
  writes = { "Match", "Fighter", "Transform", "Swing" },
  doc = "Moves the match on (Match.state): start begins a new match (match.started {online}); point waits Match.pause ticks, then starts a new round (round.started {one, two}), or wins the match at Match.to points (match.won {winner, one, two}); won waits Match.pause ticks, then either player's attack begins a new match.",
  run = function(q)
    local m = q:entity("@arena"):get("Match")
    if m.state == "start" then
      new_match(q, m)
    elseif m.state == "point" then
      if m.wait > 0 then
        m.wait -= 1
      elseif math.max(m.one, m.two) >= m.to then
        m.winner = if m.one >= m.to then 1 else 2
        m.state = "won"
        m.wait = m.pause
        q:emit("match.won", { winner = m.winner, one = m.one, two = m.two })
      else
        new_round(q)
        m.state = "fight"
        q:emit("round.started", { one = m.one, two = m.two })
      end
    elseif m.state == "won" then
      if m.wait > 0 then
        m.wait -= 1
      else
        local again = false
        for _, a in q:each("Actions") do
          again = again or a.attack_pressed
        end
        if again then
          new_match(q, m)
        end
      end
    end
  end,
}
