-- The rules of a round: a swing that lands is a point, a dash goes through
-- a swing, the swing and the dash each wait their cooldown, and the walls
-- hold. Each starts a match on this machine from the title menu, sets the
-- fighters where it wants them (sim.set changes the running world, never
-- the files), and drives both slots: t.input is player 1, t.input:slot(2)
-- player 2. The numbers (reach, speeds, cooldowns) are read from the
-- world, so tuning them keeps the tests green while the rules hold.
local duel = require("duel")

return {
  scenario "a swing that touches the other fighter scores, knocks it back, and the round starts again" {
    scene = "arena", seed = 1,
    run = function(t)
      duel.start(t)
      local one, two = duel.get(t, "@one"), duel.get(t, "@two")
      local pause = duel.get(t, "@arena").Match.pause
      duel.face_off(t, -1, 1.5)
      t.input:tap("attack", true)
      t:step(1)
      t.expect:events("swing", { by = "@one" }):count():eq(1)
      t.expect:events("point", { by = 1 }):count():eq(1)
      t.expect:field("@arena", "Match.one"):eq(1)
      t.expect:field("@arena", "Match.two"):eq(0)
      t.expect:field("@arena", "Match.state"):eq("point")
      t.expect:field("@two", "Fighter.flash"):gt(0)
      t.expect:count({ has = { "Swing" } }):eq(0)

      -- Knocked back, the way the swing went; the round stands still, so
      -- player 2 walking back does nothing.
      t.input:slot(2):hold("move", { -1, 0 }, 30)
      t:step(30)
      t.expect:field("@two", "Transform.pos.x"):gt(0.5 + 1)
      t.expect:field("@two", "Fighter.push.x"):eq(0)

      -- After the pause, both are back at home, facing each other, and the
      -- round is on again with the score kept.
      local r = t:run_until { event = "round.started", max_frames = pause + 10 }
      assert(r.hit, "no round.started after the pause")
      t.expect:field("@one", "Transform.pos.x"):near(one.Fighter.home[1], 1e-4)
      t.expect:field("@two", "Transform.pos.x"):near(two.Fighter.home[1], 1e-4)
      t.expect:field("@one", "Fighter.facing.x"):near(1, 1e-6)
      t.expect:field("@two", "Fighter.facing.x"):near(-1, 1e-6)
      t.expect:field("@arena", "Match.state"):eq("fight")
      t.expect:field("@arena", "Match.one"):eq(1)
    end,
  },

  scenario "a fighter dashing through a swing is not hit; standing in it, it is" {
    scene = "arena", seed = 1,
    run = function(t)
      duel.start(t)
      local two = t.input:slot(2)
      -- Player 2 three units away, out of reach, dashes at player 1 and
      -- through it (4 units in 10 ticks) while player 1's swing is out.
      duel.face_off(t, -1, 3)
      two:tap("dash", true)
      t.input:tap("attack", true)
      t:step(12)
      t.expect:events("dash", { by = "@two" }):count():eq(1)
      t.expect:events("swing", { by = "@one" }):count():eq(1)
      t.expect:events("point"):count():eq(0)
      t.expect:field("@two", "Transform.pos.x"):lt(-1.5)
      t.expect:field("@arena", "Match.state"):eq("fight")

      -- The same swing at a fighter standing in it is a point.
      t:step(duel.get(t, "@one").Fighter.swing_every)
      duel.face_off(t, -1, 1.5)
      t.input:tap("attack", true)
      t:step(1)
      t.expect:events("point", { by = 1 }):count():eq(1)
    end,
  },

  scenario "two swings that land on the same tick are a clash: both knocked back, no point" {
    scene = "arena", seed = 1,
    run = function(t)
      duel.start(t)
      duel.face_off(t, -1, 1.5)
      t.input:tap("attack", true)
      t.input:slot(2):tap("attack", true)
      t:step(1)
      t.expect:events("clash"):count():eq(1)
      t.expect:events("point"):count():eq(0)
      t.expect:field("@arena", "Match.state"):eq("point")
      t:step(20)
      t.expect:field("@one", "Transform.pos.x"):lt(-1 - 1)
      t.expect:field("@two", "Transform.pos.x"):gt(0.5 + 1)
      local r = t:run_until { event = "round.started", max_frames = 90 }
      assert(r.hit, "no new round after the clash")
      t.expect:field("@arena", "Match.one"):eq(0)
      t.expect:field("@arena", "Match.two"):eq(0)
    end,
  },

  scenario "the swing and the dash each wait their cooldown" {
    scene = "arena", seed = 1,
    run = function(t)
      duel.start(t)
      local f = duel.get(t, "@one").Fighter
      -- A swing; a second press too soon is nothing; the press on the tick
      -- the wait runs out swings.
      t.input:tap("attack", true)
      t:step(1)
      t.expect:events("swing"):count():eq(1)
      t:step(5)
      t.input:tap("attack", true)
      t:step(1)
      t.expect:events("swing"):count():eq(1)
      t:step(f.swing_every - 7)
      t.input:tap("attack", true)
      t:step(1)
      t.expect:events("swing"):count():eq(2)

      -- A dash goes dash_speed for dash_ticks along the facing; a press
      -- before dash_every ticks is nothing; the one on the tick after is.
      local x = duel.field(t, "@one", "Transform.pos.x")
      t.input:tap("dash", true)
      t:step(f.dash_ticks)
      t.expect:events("dash"):count():eq(1)
      t.expect:field("@one", "Transform.pos.x"):near(x + f.dash_speed * f.dash_ticks / 60, 1e-3)
      t.input:tap("dash", true)
      t:step(1)
      t.expect:events("dash"):count():eq(1)
      t:step(f.dash_every - f.dash_ticks - 2)
      t.input:tap("dash", true)
      t:step(1)
      t.expect:events("dash"):count():eq(1)
      t.input:tap("dash", true)
      t:step(1)
      t.expect:events("dash"):count():eq(2)
    end,
  },

  scenario "the fighters stay inside the arena, walking, dashing, and knocked back" {
    scene = "arena", seed = 1,
    run = function(t)
      duel.start(t)
      local arena = duel.get(t, "@arena")
      local body = duel.get(t, "@one").Sprite.size
      local wall_x = arena.Sprite.size[1] / 2 - body[1] / 2
      local wall_y = arena.Sprite.size[2] / 2 - body[2] / 2
      -- Player 1 walks into the top left corner and dashes on into it,
      -- player 2 into the bottom right.
      local d = 0.70710678
      t.input:hold("move", { -d, d }, 240)
      t.input:slot(2):hold("move", { d, -d }, 240)
      t:step(200)
      t.input:tap("dash", true)
      t.input:slot(2):tap("dash", true)
      t:step(40)
      t.expect:events("dash"):count():eq(2)
      t.expect:field("@one", "Transform.pos.x"):near(-wall_x, 1e-4)
      t.expect:field("@one", "Transform.pos.y"):near(wall_y, 1e-4)
      t.expect:field("@two", "Transform.pos.x"):near(wall_x, 1e-4)
      t.expect:field("@two", "Transform.pos.y"):near(-wall_y, 1e-4)

      -- Knocked back into the right wall, it stops there.
      duel.place(t, "@one", wall_x - 1.5, 0, 1, 0)
      duel.place(t, "@two", wall_x, 0, -1, 0)
      t:step(10)
      t.input:tap("attack", true)
      t:step(30)
      t.expect:events("point", { by = 1 }):count():eq(1)
      t.expect:field("@two", "Transform.pos.x"):near(wall_x, 1e-4)
    end,
  },

  scenario "two players share the keyboard: WASD, F, and Space, and the arrows, period, and slash" {
    scene = "arena", seed = 1,
    run = function(t)
      duel.start(t)
      -- Keys as the window sends them (input.device), through input.toml:
      -- slot 2's own keys ([slots.2.keys]) in place of the actions'.
      local function key(name, down)
        t:call("input.device", { events = { { key = name, pressed = down } } })
      end
      local function press(name)
        key(name, true)
        t:step(1)
        key(name, false)
        t:step(1)
      end
      key("KeyD", true)
      key("ArrowLeft", true)
      t:step(30)
      key("KeyD", false)
      key("ArrowLeft", false)
      t:step(1)
      t.expect:field("@one", "Transform.pos.x"):gt(-6 + 3)
      t.expect:field("@two", "Transform.pos.x"):lt(6 - 3)

      -- The arrows move only player 2, WASD only player 1.
      key("ArrowUp", true)
      t:step(20)
      key("ArrowUp", false)
      key("KeyS", true)
      t:step(20)
      key("KeyS", false)
      t:step(1)
      t.expect:field("@two", "Transform.pos.y"):gt(2)
      t.expect:field("@one", "Transform.pos.y"):lt(-2)

      -- Period swings and slash dashes for player 2; F and Space for 1.
      -- (A swing first: there is no swinging while dashing.)
      press("Period")
      t.expect:events("swing", { by = "@two" }):count():eq(1)
      t.expect:events("swing"):count():eq(1)
      press("Slash")
      t.expect:events("dash", { by = "@two" }):count():eq(1)
      t.expect:events("dash"):count():eq(1)
      press("KeyF")
      t.expect:events("swing", { by = "@one" }):count():eq(1)
      t.expect:events("swing"):count():eq(2)
      press("Space")
      t.expect:events("dash", { by = "@one" }):count():eq(1)
      t.expect:events("dash"):count():eq(2)
    end,
  },
}
