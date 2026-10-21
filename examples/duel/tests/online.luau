-- A whole match online, over a bad link: t:net makes player 2's world, a
-- copy of this one, and sends every message between the two through a
-- network with 60 ms each way, up to 30 ms more, 5% of messages lost, and
-- 2 ticks of input delay. Each t:step runs both worlds and ends with every
-- tick final and both worlds the same: if they ever differ, the run fails,
-- naming the fields. Both players' input goes through the match, each from
-- its own machine (t.input is player 1's, t.input:slot(2) player 2's), so
-- each sees the other's late and the match rolls back to correct its
-- guesses. The expectations read player 1's world, which is both.
local duel = require("duel")

-- Walks the two at each other until they are close, and stops them (the
-- input delay carries them a little closer).
local function close_in(t, two)
  t.input:hold("move", { 1, 0 })
  two:hold("move", { -1, 0 })
  for _ = 1, 60 do
    t:step(2)
    local gap = duel.field(t, "@two", "Transform.pos.x") - duel.field(t, "@one", "Transform.pos.x")
    if gap < 2 then
      break
    end
  end
  t.input:release("move")
  two:release("move")
  t:step(4)
end

return scenario "two players over a bad link play a whole match, and their worlds agree" {
  scene = "arena", seed = 1,
  run = function(t)
    -- In the match q:net() says playing from its first tick on both
    -- machines, so the menu starts the fight itself.
    t:net { players = 2, delay = 2, latency = 60, jitter = 30, loss = 0.05 }
    local two = t.input:slot(2)
    local pause = duel.get(t, "@arena").Match.pause

    -- The fight is on at once, with no menu.
    t:step(2)
    t.expect:field("@arena", "Match.state"):eq("fight")
    t.expect:field("@menu", "UiRect.shown"):eq(false)

    -- Five rounds: 1, 2, 1, 2, then 1 again, who swings each time.
    local points = 0
    for i, slot in { 1, 2, 1, 2, 1 } do
      close_in(t, two)
      t.input:slot(slot):tap("attack", true)
      t:step(6)
      points += 1
      t.expect:events("point"):count():eq(points)
      t.expect:events("point", { by = slot }):count():eq(if slot == 1 then (i + 1) // 2 else i // 2)
      t:step(pause + 4)
    end
    t.expect:field("@arena", "Match.one"):eq(3)
    t.expect:field("@arena", "Match.two"):eq(2)
    t.expect:events("match.won", { winner = 1, one = 3, two = 2 }):count():eq(1)
    t.expect:field("@win", "UiRect.shown"):eq(true)

    -- The link was bad enough to make both players guess wrong, and the
    -- worlds were compared along the way.
    local s = t:net_status()
    t:log("rollbacks " .. s.worlds[1].rollbacks .. " and " .. s.worlds[2].rollbacks .. ", lost " .. s.lost .. " of " .. s.messages)
    if s.worlds[1].rollbacks == 0 or s.worlds[2].rollbacks == 0 then
      error("a player never rolled back over a 60 ms link", 0)
    end
    if s.worlds[1].checks < 5 then
      error("too few checks of the worlds: " .. s.worlds[1].checks, 0)
    end
  end,
}
