//! A match in a scenario (docs/multiplayer-checklist.md item 3): `t:net`
//! plays the rest of a run as every player of a match, the others' worlds
//! forks over a bad simulated network. Each step ends with every world the
//! same; a world that drifts fails the run, naming the field.

mod common;

use common::*;
use serde_json::json;

const COMPONENTS: &str = r#"
component "Fighter" {
  doc = "A player's ship: turns and thrusts by move, fires by fire",
  speed = field.f32 { default = 6.0, doc = "Units per second" },
  heading = field.f32 { default = 0.0, doc = "Radians" },
  shots = field.u32 { default = 0, doc = "Shots fired" },
}

component "Shot" {
  doc = "Flies along vel until its life runs out",
  vel = field.vec2 { doc = "Units per second" },
  life = field.u8 { default = 40, doc = "Ticks left" },
}
"#;

const FLY: &str = r#"
return system "sim.fly" {
  stage = "sim",
  rng = "spread",
  reads = { "Actions" },
  writes = { "Fighter", "Transform", "Shot" },
  spawns = { "Shot", "Transform" },
  doc = "Each fighter turns by move.x, thrusts by move.y, and fires a shot, a little spread, on fire.",
  run = function(q)
    for _, f, a, t in q:each("Fighter", "Actions", "Transform") do
      local m = a.move
      f.heading += m.x * 3 * q.dt
      local dir = vector.create(math.cos(f.heading), math.sin(f.heading), 0)
      t.pos = t.pos + dir * (m.y * f.speed * q.dt)
      if a.fire_pressed then
        f.shots += 1
        local h = f.heading + (q:rng():next() - 0.5) * 0.3
        local s = q:spawn({ "Shot", "Transform" })
        s:get("Transform").pos = t.pos
        s:get("Shot").vel = vector.create(math.cos(h), math.sin(h), 0) * 12
      end
    end
  end,
}
"#;

const SHOTS: &str = r#"
return system "sim.shots" {
  stage = "sim",
  after = { "sim.fly" },
  writes = { "Shot", "Transform" },
  spawns = { "Shot", "Transform" },
  doc = "Moves each shot and takes it away when its life is spent.",
  run = function(q)
    for e, s, t in q:each("Shot", "Transform") do
      t.pos = t.pos + s.vel * q.dt
      s.life -= 1
      if s.life == 0 then e:despawn() end
    end
  end,
}
"#;

const INPUT: &str = r#"
[actions.move]
type = "axis2"
doc = "Turn (x) and thrust (y)"
[actions.move.bindings]
keyboard = ["wasd"]

[actions.fire]
type = "button"
doc = "Fire a shot"
[actions.fire.bindings]
keyboard = ["space"]

[slots.1]
devices = ["keyboard"]

[slots.2]
devices = ["keyboard"]

[slots.2.keys]
move = ["arrows"]
fire = ["period"]
"#;

const MATCH: &str = r#"
return scenario "two players over a bad link play one run" {
  scene = "main", seed = 1,
  run = function(t)
    t:net { players = 2, delay = 2, latency = 70, jitter = 40, loss = 0.05 }
    local two = t.input:slot(2)
    t.input:hold("move", { 1, 1 }, 40)
    two:hold("move", { -1, 1 }, 30)
    t:step(12)
    two:tap("fire", true)
    t:step(4)
    t.input:tap("fire", true)
    t:step(90)
    two:hold("move", { 0, 1 }, 25)
    t.input:tap("fire", true)
    t:step(60)
    t.expect:field("@one", "Fighter.shots"):eq(2)
    t.expect:field("@two", "Fighter.shots"):eq(1)
    local s = t:net_status()
    t:log("rollbacks " .. s.worlds[1].rollbacks .. " and " .. s.worlds[2].rollbacks .. ", lost " .. s.lost .. " of " .. s.messages)
    if s.worlds[2].rollbacks == 0 then error("player 2 never rolled back on a 70 ms link", 0) end
    if s.worlds[1].checks < 2 then error("too few checks: " .. s.worlds[1].checks, 0) end
  end,
}
"#;

const DRIFT: &str = r#"
return scenario "a world that drifts fails the run" {
  scene = "main", seed = 1,
  run = function(t)
    t:net { players = 2 }
    t.input:hold("move", { 0, 1 }, 200)
    t:step(30)
    -- Player 2's world goes its own way.
    t:call("sim.set", { fork = "net.2", entity = "@two", path = "Fighter.speed", value = 9 })
    t:step(120)
  end,
}
"#;

fn fighter(slot: u8, x: f32, heading: f32) -> String {
    format!(
        "[PlayerSlot]\nslot = {slot}\n\n[Actions]\n\n[Transform]\npos = [{x:.1}, 0.0]\n\n[Fighter]\nheading = {heading}\n"
    )
}

fn duel(root: &std::path::Path) {
    write(
        root,
        "argus.toml",
        "name = \"duel\"\nclock = \"continuous@60\"\ndefault_scene = \"main\"\nplugins = []\n",
    );
    write(root, "input.toml", INPUT);
    write(root, "components/duel.luau", COMPONENTS);
    write(root, "systems/fly.luau", FLY);
    write(root, "systems/shots.luau", SHOTS);
    write(
        root,
        "scenes/main/scene.toml",
        "clock = \"continuous@60\"\nseed = 1\n",
    );
    write(root, "scenes/main/one.toml", &fighter(1, -4.0, 0.0));
    write(root, "scenes/main/two.toml", &fighter(2, 4.0, 3.0));
}

#[test]
fn a_scenario_plays_a_match_over_a_bad_link_and_the_players_agree() {
    let t = temp("net-sim-match");
    let root = t.path();
    duel(root);
    write(root, "tests/match.luau", MATCH);
    let mut d = open(root);
    let r = ok(&mut d, "test.run", json!({}));
    assert_eq!(r["ok"], true, "{r:#}");
    let logged = r.to_string();
    assert!(logged.contains("rollbacks"), "{r:#}");
    // The match ended with the run: the world is the scene's, alone.
    assert_eq!(ok(&mut d, "net.status", json!({}))["state"], "alone");
}

#[test]
fn a_scenario_whose_worlds_drift_fails_and_names_the_field() {
    let t = temp("net-sim-drift");
    let root = t.path();
    duel(root);
    write(root, "tests/drift.luau", DRIFT);
    let mut d = open(root);
    let r = ok(&mut d, "test.run", json!({}));
    assert_eq!(r["ok"], false, "{r:#}");
    let text = r.to_string();
    assert!(text.contains("out of step"), "{r:#}");
    assert!(text.contains("Fighter.speed"), "{r:#}");
}

/// The daemon as one player of a real match (net.host): a person's game
/// (here a Player, as a desktop game is) joins by the code, and the agent's
/// injects are the daemon's side. Both end on the same world.
#[test]
fn the_daemon_plays_one_side_through_a_relay() {
    use argus_player::netplay::Netplay;
    use argus_player::Player;
    let t = temp("net-relay-daemon");
    let root = t.path();
    duel(root);
    let relay = argus_relay::Relay::bind("127.0.0.1:0", argus_relay::Limits::default())
        .unwrap()
        .spawn();
    let url = format!("ws://{}", relay.addr());
    let mut d = open(root);
    let r = ok(
        &mut d,
        "net.host",
        json!({ "relay": url, "players": 2, "seed": 5 }),
    );
    assert_eq!(r["state"], "room", "{r}");
    let code = r["code"].as_str().unwrap().to_string();
    assert_eq!(r["slot"], 1);
    // The person's game joins.
    let mut p = Player::open(root, None, None).unwrap();
    let mut net: Netplay<argus_relay::Client> = Netplay::new();
    net.join(argus_relay::Client::connect(&url).unwrap(), &p, &code);
    let mut injected = false;
    for f in 0..600u64 {
        d.frame().unwrap();
        if let Some(start) = net.poll() {
            p.restart(&start.scene, start.seed).unwrap();
            net.begin(&mut p, &start).unwrap();
        }
        if net.playing() && f % 40 == 5 {
            // The person presses fire on their side: online, everyone's own
            // keys are slot 1's, whatever slot the room gave them.
            p.devices(&[argus_input::DeviceInput::Key {
                name: "Space".into(),
                pressed: true,
            }]);
        }
        if net.playing() && f % 40 == 9 {
            p.devices(&[argus_input::DeviceInput::Key {
                name: "Space".into(),
                pressed: false,
            }]);
        }
        net.frame(&mut p, 1).unwrap();
        let s = ok(&mut d, "net.status", json!({}));
        if s["state"] == "playing" && !injected {
            // The agent's side.
            ok(
                &mut d,
                "input.inject",
                json!({ "action": "move", "value": [1, 1], "frames": 60 }),
            );
            ok(
                &mut d,
                "input.inject",
                json!({ "action": "fire", "frames": 2 }),
            );
            injected = true;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(injected);
    // Both run to the same tick, every tick final.
    for _ in 0..1000 {
        let lead = d.call(0, "sim.status", &json!({})).unwrap()["tick"]
            .as_u64()
            .unwrap();
        let s = ok(&mut d, "net.status", json!({}));
        if p.tick() == lead
            && s["match"]["final_to"] == lead
            && net.stats().is_some_and(|x| x.checks > 2)
        {
            break;
        }
        if p.tick() < lead {
            net.frame(&mut p, 1).unwrap();
        } else {
            d.frame().unwrap();
            net.frame(&mut p, 0).unwrap();
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let s = ok(&mut d, "net.status", json!({}));
    assert_eq!(s["state"], "playing", "{s}");
    let mine = ok(&mut d, "world.get", json!({ "entity": "@one" }));
    let theirs = p.world.entity("@one").unwrap();
    assert_eq!(
        mine["components"]["Fighter"]["shots"],
        json!(1),
        "the agent fired once: {mine}"
    );
    assert_eq!(
        p.world.get_field(theirs, "Fighter.shots").unwrap(),
        argus_ecs::Value::UInt(1)
    );
    let two = p.world.entity("@two").unwrap();
    let shots = match p.world.get_field(two, "Fighter.shots").unwrap() {
        argus_ecs::Value::UInt(n) => n,
        v => panic!("{v:?}"),
    };
    assert!(shots >= 10, "the person fired: {shots}");
    let dtwo = ok(&mut d, "world.get", json!({ "entity": "@two" }));
    assert_eq!(dtwo["components"]["Fighter"]["shots"], json!(shots));
    // Leaving ends it for the other side.
    ok(&mut d, "net.leave", json!({}));
    for _ in 0..200 {
        net.frame(&mut p, 0).unwrap();
        if !net.playing() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(!net.playing());
    assert!(net.view().why.contains("left"), "{:?}", net.view());
}
