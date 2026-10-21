//! Matches (docs/multiplayer-checklist.md item 1): two players in one
//! process, over a network made as bad as a test asks, end on the same
//! world, tick for tick, as each other and as one player replaying either
//! one's input alone. A prediction proven wrong rolls back and agrees; a
//! world that drifts is caught within a second, by the field.

use argus_input::ActionValue;
use argus_net::{Conditions, Config, Match, Peer, SimLink, SimNet, State};
use argus_player::Player;
use std::path::{Path, PathBuf};

/// A temp directory removed on drop, used as its path.
struct Temp(PathBuf);

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl std::ops::Deref for Temp {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

const COMPONENTS: &str = r#"
component "Fighter" {
  doc = "A player's ship: turns and thrusts by move, fires by fire",
  speed = field.f32 { default = 6.0, doc = "Units per second" },
  heading = field.f32 { default = 0.0, doc = "Radians" },
  shots = field.u32 { default = 0, doc = "Shots fired" },
  hum = field.u32 { default = 0, doc = "Ticks thrusting; it hums every eighth" },
}

component "Net" {
  doc = "What q:net() said this tick",
  state = field.str { doc = "alone, asking, room, playing, or ended" },
  players = field.u8 { default = 0, doc = "Players" },
  code = field.str { doc = "The room's code, in a room" },
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
      if m.y > 0 then
        f.hum += 1
        if f.hum % 8 == 1 then q:play("sfx/zap") end
      end
      if a.fire_pressed then
        q:play("sfx/zap")
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

const NET: &str = r#"
return system "sim.net" {
  stage = "sim",
  writes = { "Net" },
  doc = "Writes what q:net() says on the watcher; the menu's asks come from host = true on it.",
  run = function(q)
    local n = q:net()
    for _, w in q:each("Net") do
      w.state = n.state
      w.players = n.players
      w.code = n.code or ""
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
gamepad = ["left_stick"]

[actions.fire]
type = "button"
doc = "Fire a shot"
[actions.fire.bindings]
keyboard = ["space"]
gamepad = ["south"]

[slots.1]
devices = ["keyboard"]

[slots.2]
devices = ["gamepad:0"]
"#;

fn fighter(slot: u8, x: f32, heading: f32) -> String {
    format!(
        "[PlayerSlot]\nslot = {slot}\n\n[Actions]\n\n[Transform]\npos = [{x:.1}, 0.0]\n\n[Fighter]\nheading = {heading}\n"
    )
}

/// A tenth of a second of a falling tone, a 16-bit mono WAV.
fn zap() -> Vec<u8> {
    let rate = 22_050u32;
    let n = rate / 10;
    let samples: Vec<i16> = (0..n)
        .map(|i| {
            let t = i as f32 / rate as f32;
            ((t * (900.0 - 4000.0 * t) * std::f32::consts::TAU).sin() * 9000.0) as i16
        })
        .collect();
    let data = samples.len() as u32 * 2;
    let mut b = Vec::new();
    b.extend(b"RIFF");
    b.extend((36 + data).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(rate.to_le_bytes());
    b.extend((rate * 2).to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend(16u16.to_le_bytes());
    b.extend(b"data");
    b.extend(data.to_le_bytes());
    for s in samples {
        b.extend(s.to_le_bytes());
    }
    b
}

/// A two-player game, real time or in turns.
fn duel(name: &str, clock: &str) -> Temp {
    let dir = std::env::temp_dir().join(format!("argus-net-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let put = |rel: &str, text: &str| {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    };
    put(
        "argus.toml",
        &format!("name = \"duel\"\nclock = \"{clock}\"\ndefault_scene = \"main\"\nplugins = []\n"),
    );
    put("input.toml", INPUT);
    put("components/duel.luau", COMPONENTS);
    put("systems/fly.luau", FLY);
    put("systems/shots.luau", SHOTS);
    put("systems/net.luau", NET);
    put("scenes/main/watch.toml", "[Net]\n");
    put(
        "scenes/main/scene.toml",
        &format!("clock = \"{clock}\"\nseed = 1\n"),
    );
    std::fs::create_dir_all(dir.join("assets/sfx")).unwrap();
    std::fs::write(dir.join("assets/sfx/zap.wav"), zap()).unwrap();
    put("scenes/main/one.toml", &fighter(1, -4.0, 0.0));
    put("scenes/main/two.toml", &fighter(2, 4.0, 3.0));
    Temp(dir)
}

/// What player `slot` does at frame `f`: a seeded walk of turns, thrusts,
/// and shots, changing every few frames.
fn script(slot: u8, f: u64) -> Vec<(String, ActionValue)> {
    let h = |x: u64| {
        let mut z = x.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ (slot as u64 * 0x5bd1_e995);
        z ^= z >> 29;
        z = z.wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z ^ (z >> 32)
    };
    let mut out = Vec::new();
    if f.is_multiple_of(7) {
        let r = h(f);
        let x = ((r % 5) as f32 - 2.0) / 2.0;
        let y = (((r >> 8) % 3) as f32) / 2.0;
        out.push(("move".to_string(), ActionValue::Axis2(x, y)));
    }
    if f % 11 == 3 {
        out.push(("fire".to_string(), ActionValue::Button(h(f + 1) % 2 == 0)));
    }
    out
}

struct Game {
    net: SimNet,
    players: Vec<Player>,
    matches: Vec<Match<SimLink, Player>>,
}

fn game(dir: &Path, cond: Conditions, delay: u8, lockstep: bool) -> Game {
    let net = SimNet::new(2, cond, 42);
    let mut players = Vec::new();
    let mut matches = Vec::new();
    for slot in 1..=2u8 {
        let mut p = Player::open(dir, None, None).unwrap();
        p.join_match();
        p.record();
        let mut cfg = Config::new(2, slot, 0xd0e1);
        cfg.delay = delay;
        cfg.lockstep = lockstep;
        matches.push(Match::new(cfg, net.link(slot as usize - 1), &p).unwrap());
        players.push(p);
    }
    Game {
        net,
        players,
        matches,
    }
}

impl Game {
    /// One frame: the network moves on, each player gives its input and
    /// runs the tick it owes.
    fn frame(&mut self, f: u64, input: bool) {
        self.net.advance(1000.0 / 60.0);
        for i in 0..2 {
            if input {
                self.matches[i].local(&script(i as u8 + 1, f));
            }
            self.matches[i].advance(&mut self.players[i], 1).unwrap();
        }
    }

    /// Frames with nothing new until both players have run to the same
    /// tick and every tick before it is final on both.
    fn settle(&mut self) {
        for _ in 0..600 {
            let t0 = self.players[0].tick();
            let t1 = self.players[1].tick();
            if t0 == t1 && self.matches.iter().all(|m| m.final_to() == t0) {
                return;
            }
            self.net.advance(1000.0 / 60.0);
            let lead = t0.max(t1);
            for i in 0..2 {
                let behind = self.players[i].tick() < lead;
                self.matches[i]
                    .advance(&mut self.players[i], behind as u32)
                    .unwrap();
            }
        }
        panic!(
            "never settled: ticks {} and {}, final {} and {}",
            self.players[0].tick(),
            self.players[1].tick(),
            self.matches[0].final_to(),
            self.matches[1].final_to()
        );
    }
}

/// The match as one player alone would play it: a fresh player given one
/// side's input log from tick 0. Its digest and the sounds it played.
fn replayed(dir: &Path, log: &serde_json::Value, ticks: u64) -> (u64, u64) {
    let mut p = Player::open(dir, None, None).unwrap();
    p.replay_log(log).unwrap();
    p.step(ticks).unwrap();
    (p.digest(), p.sounds)
}

fn play(name: &str, cond: Conditions, frames: u64) -> (Game, Temp) {
    let dir = duel(name, "continuous@60");
    let mut g = game(&dir, cond, 2, false);
    for f in 0..frames {
        g.frame(f, true);
        for m in &g.matches {
            assert!(
                matches!(m.state(), State::Running | State::Waiting),
                "{:?}",
                m.state()
            );
        }
    }
    g.settle();
    (g, dir)
}

fn check_one_run(g: &Game, dir: &Path) {
    let (a, b) = (&g.players[0], &g.players[1]);
    assert_eq!(a.tick(), b.tick());
    assert_eq!(a.digest(), b.digest(), "the two players' worlds");
    let (la, lb) = (a.input_log(), b.input_log());
    assert_eq!(la, lb, "the two players' input logs");
    assert!(la.as_array().is_some_and(|l| l.len() > 50), "{la}");
    let (digest, sounds) = replayed(dir, &la, a.tick());
    assert_eq!(digest, a.digest(), "one player replaying the log alone");
    // A rollback's sounds: none played twice, none left that the run as
    // finally played did not make.
    for p in &g.players {
        let s = p.net_sounds().unwrap();
        assert_eq!(p.sounds - s.stopped, sounds, "{s:?}, {} started", p.sounds);
    }
    // A report from either player is the match.
    for p in &g.players {
        let bytes = p.report("the match", "report", "").unwrap();
        let mut q = Player::open(dir, None, None).unwrap();
        q.play_report(&bytes).unwrap();
        assert_eq!(q.tick(), a.tick());
        assert_eq!(q.digest(), a.digest(), "a report played back");
    }
}

#[test]
fn two_players_on_a_good_link_play_one_run() {
    let (g, dir) = play(
        "good",
        Conditions {
            delay_ms: 10.0,
            ..Conditions::default()
        },
        600,
    );
    check_one_run(&g, &dir);
    for m in &g.matches {
        let s = m.stats();
        assert_eq!(
            s.rollbacks, 0,
            "a link quicker than the delay predicts nothing"
        );
        assert!(s.checks >= 9, "{s:?}");
    }
}

#[test]
fn a_bad_link_rolls_back_and_the_players_still_agree() {
    let (g, dir) = play(
        "bad",
        Conditions {
            delay_ms: 70.0,
            jitter_ms: 40.0,
            loss: 0.1,
        },
        900,
    );
    check_one_run(&g, &dir);
    let s0 = g.matches[0].stats();
    let s1 = g.matches[1].stats();
    assert!(s0.rollbacks > 10 && s1.rollbacks > 10, "{s0:?} {s1:?}");
    assert!(s0.deepest <= 8 && s1.deepest <= 8, "{s0:?} {s1:?}");
    assert!(s0.checks >= 10, "{s0:?}");
    let heard: Vec<_> = g.players.iter().map(|p| p.net_sounds().unwrap()).collect();
    assert!(
        heard
            .iter()
            .all(|s| s.kept > 0 && s.late > 0 && s.stopped > 0),
        "{heard:?}"
    );
    let (sent, lost) = g.net.counts();
    assert!(lost > 0 && lost < sent);
}

#[test]
fn a_world_that_drifts_is_caught_within_a_second_by_the_field() {
    let dir = duel("drift", "continuous@60");
    let mut g = game(
        &dir,
        Conditions {
            delay_ms: 10.0,
            ..Conditions::default()
        },
        2,
        false,
    );
    for f in 0..130 {
        g.frame(f, true);
    }
    assert!(matches!(g.matches[0].state(), State::Running));
    // Player 2's world goes its own way: a field nothing writes back.
    let p2 = &g.players[1];
    let two = p2.world.entity("@two").unwrap();
    p2.world.set_field(two, "Fighter.speed", 6.5).unwrap();
    let at = g.players[1].tick();
    let mut caught = None;
    for f in 130..260 {
        g.frame(f, true);
        if g.matches
            .iter()
            .all(|m| matches!(m.state(), State::OutOfStep(o) if !o.differences.is_empty()))
        {
            caught = Some(f);
            break;
        }
    }
    let caught = caught.expect("never caught");
    assert!(
        caught - 130 <= 60 + 10,
        "caught {} ticks after",
        caught - 130
    );
    for (i, m) in g.matches.iter().enumerate() {
        let State::OutOfStep(o) = m.state() else {
            unreachable!()
        };
        assert!(o.tick >= at, "{o:?}");
        let d: Vec<&str> = o.differences.iter().map(|d| d.field.as_str()).collect();
        assert!(
            d.iter().any(|f| f.ends_with("Fighter.speed")),
            "player {}: {d:?}",
            i + 1
        );
        let words = o.describe();
        assert!(
            words.starts_with(&format!("out of step at the end of tick {}", o.tick))
                && words.contains("Fighter.speed: "),
            "{words}"
        );
    }
    // Stopped: nothing runs.
    let t = g.players[0].tick();
    g.frame(300, true);
    assert_eq!(g.players[0].tick(), t);
}

#[test]
fn a_turn_based_game_waits_for_every_command() {
    let dir = duel("turns", "discrete");
    let mut g = game(
        &dir,
        Conditions {
            delay_ms: 30.0,
            ..Conditions::default()
        },
        2,
        true,
    );
    // Player 1 presses fire on frames 5 and 40; nobody else does a thing.
    let mut turns = Vec::new();
    for f in 0..90u64 {
        g.net.advance(1000.0 / 60.0);
        for i in 0..2 {
            if i == 0 && (f == 5 || f == 40) {
                g.matches[0].local(&[("fire".to_string(), ActionValue::Button(true))]);
            }
            if i == 0 && (f == 6 || f == 41) {
                g.matches[0].local(&[("fire".to_string(), ActionValue::Button(false))]);
            }
            g.matches[i].advance(&mut g.players[i], 16).unwrap();
        }
        turns.push((g.players[0].tick(), g.players[1].tick()));
    }
    let (a, b) = (&g.players[0], &g.players[1]);
    assert_eq!(a.tick(), b.tick());
    assert_eq!(a.digest(), b.digest());
    let one = a.world.entity("@one").unwrap();
    assert_eq!(
        a.world.get_field(one, "Fighter.shots").unwrap(),
        argus_ecs::Value::UInt(2)
    );
    // Player 1's press could not run its turn until player 2's empty one came.
    assert_eq!(turns[5], (0, 0), "{turns:?}");
    assert!(g.matches.iter().all(|m| m.stats().rollbacks == 0));
}

#[test]
fn players_of_other_games_are_refused() {
    let dir = duel("refused", "continuous@60");
    let net = SimNet::new(2, Conditions::default(), 1);
    let mut ps: Vec<Player> = (0..2)
        .map(|_| Player::open(&dir, None, None).unwrap())
        .collect();
    let mut ms: Vec<Match<SimLink, Player>> = (0..2)
        .map(|i| {
            let cfg = Config::new(2, i as u8 + 1, 100 + i as u64);
            Match::new(cfg, net.link(i), &ps[i]).unwrap()
        })
        .collect();
    for _ in 0..5 {
        net.advance(16.0);
        for i in 0..2 {
            ms[i].advance(&mut ps[i], 1).unwrap();
        }
    }
    for m in &ms {
        assert!(
            matches!(m.state(), State::Ended(why) if why.contains("another game")),
            "{:?}",
            m.state()
        );
    }
    assert_eq!(ps[0].tick(), 0);
}

/// What a rollback of the whole window costs, against a 60 Hz frame:
/// Hollow, a snapshot taken, the world put back, and 8 ticks run again.
#[test]
#[ignore = "release timing"]
fn a_rollback_of_eight_ticks_is_a_fraction_of_a_frame() {
    let mut p = Player::open(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hollow"),
        None,
        None,
    )
    .unwrap();
    p.join_match();
    p.step(120).unwrap();
    let runs = 30;
    let t = std::time::Instant::now();
    for _ in 0..runs {
        let s = Peer::save(&p);
        Peer::load(&mut p, &s);
        p.rolled_back(p.tick());
        for _ in 0..8 {
            Peer::step(&mut p, &[], true).unwrap();
        }
        p.caught_up();
    }
    let each = t.elapsed() / runs;
    eprintln!(
        "a rollback of 8 ticks: {each:?} ({:.0}% of a 60 Hz frame), {} entities",
        each.as_secs_f64() * 6000.0,
        p.world.entity_count()
    );
    assert!(each.as_secs_f64() < 0.25 / 60.0, "{each:?}");
}

/// The same match, through a real relay on this machine: each player's
/// link is a WebSocket to it, so the messages cross threads and sockets.
#[test]
fn two_players_play_one_run_through_a_relay() {
    let dir = duel("relay", "continuous@60");
    let relay = argus_relay::Relay::bind("127.0.0.1:0", argus_relay::Limits::default())
        .unwrap()
        .spawn();
    let url = format!("ws://{}", relay.addr());
    let game = 0xd0e1;
    let host = argus_relay::Client::host(&url, 2, game).unwrap();
    let guest = argus_relay::Client::join(&url, host.code(), game).unwrap();
    let mut players = Vec::new();
    let mut matches = Vec::new();
    for link in [host, guest] {
        let mut p = Player::open(&dir, None, None).unwrap();
        p.join_match();
        let cfg = Config::new(2, link.slot(), game);
        matches.push(Match::new(cfg, link, &p).unwrap());
        players.push(p);
    }
    for f in 0..400u64 {
        for i in 0..2 {
            matches[i].local(&script(i as u8 + 1, f));
            matches[i].advance(&mut players[i], 1).unwrap();
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    // Run the one behind up to the other, and wait until both are final.
    for _ in 0..2000 {
        let lead = players[0].tick().max(players[1].tick());
        if players.iter().all(|p| p.tick() == lead) && matches.iter().all(|m| m.final_to() == lead)
        {
            break;
        }
        for i in 0..2 {
            let behind = (players[i].tick() < lead) as u32;
            matches[i].advance(&mut players[i], behind).unwrap();
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let (a, b) = (&players[0], &players[1]);
    assert_eq!(a.tick(), b.tick());
    assert!(a.tick() >= 390, "{}", a.tick());
    assert_eq!(a.digest(), b.digest());
    assert_eq!(a.input_log(), b.input_log());
    assert!(matches.iter().all(|m| matches!(m.state(), State::Running)));
    assert!(
        matches.iter().all(|m| m.stats().checks >= 5),
        "{:?}",
        matches[0].stats()
    );
}

fn watched(p: &Player) -> (String, u64, String) {
    let w = p.world.entity("@watch").unwrap();
    let get = |f: &str| p.world.get_field(w, f).unwrap();
    let text = |v: argus_ecs::Value| match v {
        argus_ecs::Value::Str(s) => s,
        v => panic!("{v:?}"),
    };
    let players = match get("Net.players") {
        argus_ecs::Value::UInt(n) => n,
        v => panic!("{v:?}"),
    };
    (text(get("Net.state")), players, text(get("Net.code")))
}

/// A room on a real relay: the host's menu hosts, the code goes to the
/// other player, who joins; once the room is full both open the host's
/// scene afresh from its seed, and play one run. Scripts saw the room, then
/// only what both share.
#[test]
fn a_room_on_a_relay_starts_one_match_from_the_hosts_seed() {
    use argus_player::netplay::{NetAsk, Netplay};
    use argus_relay::Client;
    let dir = duel("lobby", "continuous@60");
    let relay = argus_relay::Relay::bind("127.0.0.1:0", argus_relay::Limits::default())
        .unwrap()
        .spawn();
    let url = format!("ws://{}", relay.addr());
    let mut players: Vec<Player> = (0..2)
        .map(|_| Player::open(&dir, None, None).unwrap())
        .collect();
    let mut nets: Vec<Netplay<Client>> = (0..2).map(|_| Netplay::new()).collect();
    nets[0].host(Client::connect(&url).unwrap(), &players[0], 2, None, 77, 2);
    let mut code = None;
    let frame = |players: &mut Vec<Player>, nets: &mut Vec<Netplay<Client>>, f: u64| {
        for i in 0..2 {
            if let Some(start) = nets[i].poll() {
                let mut p = Player::open(&dir, Some(&start.scene), Some(start.seed)).unwrap();
                nets[i].begin(&mut p, &start).unwrap();
                players[i] = p;
            }
            nets[i].local(&script(i as u8 + 1, f));
            nets[i].frame(&mut players[i], 1).unwrap();
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    };
    // The host's room, as its scripts see it.
    for f in 0..200 {
        frame(&mut players, &mut nets, f);
        let (state, _, c) = watched(&players[0]);
        if state == "room" && !c.is_empty() {
            code = Some(c);
            break;
        }
    }
    let code = code.expect("the host's scripts saw its room");
    assert_eq!(code.len(), 5);
    nets[1].join(Client::connect(&url).unwrap(), &players[1], &code);
    for f in 0..400 {
        frame(&mut players, &mut nets, f);
    }
    for (i, p) in players.iter().enumerate() {
        assert_eq!(p.seed, 77, "player {} plays the host's seed", i + 1);
        assert_eq!(watched(p).0, "playing");
        assert_eq!(watched(p).1, 2);
        assert!(
            matches!(nets[i].state(), Some(State::Running)),
            "{:?}",
            nets[i].state()
        );
    }
    // Both run to the same tick, every tick final, and agree.
    for _ in 0..2000 {
        let lead = players[0].tick().max(players[1].tick());
        let done = players.iter().all(|p| p.tick() == lead)
            && nets.iter().all(|n| n.stats().is_some_and(|s| s.checks > 0));
        if done {
            break;
        }
        for i in 0..2 {
            let behind = (players[i].tick() < lead) as u32;
            nets[i].frame(&mut players[i], behind).unwrap();
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(players[0].digest(), players[1].digest());
    // A player leaving ends the match for the other, who is told why.
    let mut p0 = std::mem::replace(&mut players[0], Player::open(&dir, None, None).unwrap());
    nets[0].leave(&mut p0);
    for f in 0..200 {
        nets[1].frame(&mut players[1], 1).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        if f > 5 && watched(&players[1]).0 == "ended" {
            break;
        }
    }
    let v = nets[1].view();
    assert_eq!(v.state, "ended");
    assert!(v.why.contains("left"), "{v:?}");
    // The menu's asks reach the host.
    let asks = NetAsk::from_event(
        "net.host_requested",
        &[("players".into(), argus_ecs::Value::Int(3))],
    );
    assert_eq!(
        asks,
        Some(NetAsk::Host {
            players: 3,
            scene: None
        })
    );
}
