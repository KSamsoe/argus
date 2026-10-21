-- A whole match on this machine: first to three wins, the win screen says
-- who, nothing moves under it, and after its pause either player's attack
-- starts a new match from 0 - 0.
local duel = require("duel")

-- A point for player 1 or 2: the two face to face, and that one swings.
local function point(t, slot)
  duel.face_off(t, -1, 1.5)
  t.input:slot(slot):tap("attack", true)
  t:step(1)
end

return scenario "first to three wins: the win screen names the winner, and attack starts a new match" {
  scene = "arena", seed = 1,
  run = function(t)
    duel.start(t)
    local m = duel.get(t, "@arena").Match
    for i, slot in { 1, 2, 1 } do
      point(t, slot)
      local r = t:run_until { event = "round.started", max_frames = m.pause + 10 }
      assert(r.hit, "no new round after point " .. i)
    end
    t.expect:field("@arena", "Match.one"):eq(2)
    t.expect:field("@arena", "Match.two"):eq(1)
    local hud = duel.field(t, "@hud", "UiText.shown")
    assert(string.find(hud, "2 - 1", 1, true), "the HUD reads " .. hud)

    -- The third point for player 1: the match is won once its pause is over.
    point(t, 1)
    t.expect:events("match.won"):count():eq(0)
    local r = t:run_until { event = "match.won", max_frames = m.pause + 10 }
    assert(r.hit, "no match.won")
    t.expect:events("match.won", { winner = 1, one = 3, two = 1 }):count():eq(1)
    t:step(1)
    t.expect:field("@win", "UiRect.shown"):eq(true)
    t.expect:field("@win/panel/title", "UiText.shown"):eq("Player 1 wins")

    -- Nothing moves under it, and a press before its pause is over is nothing.
    local x = duel.field(t, "@one", "Transform.pos.x")
    t.input:hold("move", { 1, 0 }, 20)
    t:step(20)
    t.expect:field("@one", "Transform.pos.x"):eq(x)
    t.input:slot(2):tap("attack", true)
    t:step(1)
    t.expect:events("match.started"):count():eq(1)

    -- After it, player 2's attack starts a new match.
    t:step(m.pause)
    t.input:slot(2):tap("attack", true)
    t:step(1)
    t.expect:events("match.started"):count():eq(2)
    t.expect:field("@arena", "Match.one"):eq(0)
    t.expect:field("@arena", "Match.two"):eq(0)
    t.expect:field("@arena", "Match.winner"):eq(0)
    t.expect:field("@arena", "Match.state"):eq("fight")
    t:step(1)
    t.expect:field("@win", "UiRect.shown"):eq(false)
  end,
}
