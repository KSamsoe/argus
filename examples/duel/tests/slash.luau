-- A swing's slash (prefabs/slash.toml) is drawn apart from its hitbox
-- (prefabs/swing.toml): it sweeps while the hitbox is out, then stays where
-- it was, fading, for 16 ticks more. Online, the other player's swing can be
-- learned of several ticks late, after a rollback, when its hitbox's few
-- ticks are nearly over; the fade keeps it in sight. The slash is
-- presentation: it never scores.
local duel = require("duel")

local FADE = 16 -- SLASH_FADE in systems/looks.luau

return {
  scenario "a slash sweeps while its swing's hitbox is out, then stays, fading, and goes" {
    scene = "arena", seed = 1,
    run = function(t)
      duel.start(t)
      local life = duel.get(t, "@one").Fighter.swing_ticks
      duel.face_off(t, -3, 5) -- out of reach: the swing hits nothing
      t.input:tap("attack", true)
      local r = t:run_until { event = "swing", max_frames = 5 }
      assert(r.hit, "no swing")
      t.expect:count({ has = { "Swing" } }):eq(1)
      t.expect:count({ has = { "Slash" } }):eq(1)
      t.expect:count({ has = { "Swing", "Sprite" } }):eq(0) -- the hitbox draws nothing

      -- The hitbox goes after its ticks; the slash stays, fading.
      t:step(life)
      t.expect:count({ has = { "Swing" } }):eq(0)
      t.expect:count({ has = { "Slash" } }):eq(1)
      t:step(FADE - 1)
      t.expect:count({ has = { "Slash" } }):eq(1)
      t:step(1)
      t.expect:count({ has = { "Slash" } }):eq(0)
      t.expect:events("point"):count():eq(0)
    end,
  },

  scenario "a swing that scores keeps its slash in sight, and a new round clears it" {
    scene = "arena", seed = 1,
    run = function(t)
      duel.start(t)
      duel.face_off(t, -1, 1.5)
      t.input:tap("attack", true)
      t:step(1)
      t.expect:events("point", { by = 1 }):count():eq(1)
      t.expect:count({ has = { "Swing" } }):eq(0) -- a point takes the hitbox
      t.expect:count({ has = { "Slash" } }):eq(1) -- not the slash
      local pause = duel.get(t, "@arena").Match.pause
      local r = t:run_until { event = "round.started", max_frames = pause + 10 }
      assert(r.hit, "no round.started after the pause")
      t.expect:count({ has = { "Slash" } }):eq(0)
    end,
  },
}
