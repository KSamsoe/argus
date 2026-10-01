//! The phase 4 gate (design §18): "Everything the editor does appears in
//! the change feed as a command the agent could have issued." Every panel
//! is driven headless as a person drives it, over a real socket, on one
//! project with something for each panel: a lamp whose light is a tune
//! field and a timeline about it, a hero with an animator, a tileset and a
//! map, a door with a box to tick. The agent runs, asks, proposes, and
//! carves beside the person. Then the whole feed, every author's rows in
//! order, is typed into the CLI against a fresh daemon on a fresh copy of
//! the project, and the two are the same world: the same digest, clock,
//! and world in use, and the same files byte for byte.

#[path = "../../argus-daemon/tests/common/mod.rs"]
mod common;

use argus_cli::{run, EXIT_OK};
use argus_client::{command, Client};
use argus_daemon::{serve, socket_name_for, Transport};
use argus_editor::app::Tab;
use argus_editor::Editor;
use eframe::egui::{self, accesskit::Role, Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use serde_json::{json, Value as Json};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

const MACHINE: &str = r#"# The hero: idle until hurt enough to run.
type = "animator"
initial = "idle"

[states.idle]
clip = "idle"

[[states.idle.transitions]]
to = "run"
when = "Health.cur > 5"

[states.run]
clip = "run"
fps = 8.0
events = [{ frame = 1, name = "footstep" }]

[[states.run.transitions]]
to = "idle"
when = "Health.cur <= 5"
"#;

const SHOW: &str = r#"# The lamp comes up over a second, then flares.
["show"]
length = 2.0

["show.glow"]
timeline = "timeline:show"
track = "lamp"
at = 0.5
kind = "tween"
who = "actor"
path = "Light.intensity"
to = [5.0, 0.0]
duration = 1.0

["show.flare"]
timeline = "timeline:show"
track = "lamp"
at = 1.5
kind = "event"
effects = "actor:Light.intensity = 9; emit flared"
"#;

const TILESET: &str = r#"grid = [4, 4]

[[frames]]
index = 15
properties = { solid = true }

[autotile.rock]
mode = "4bit"
tiles = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
"#;

const MAP: &str = "scenes/arena/room.tilemap.toml";

fn png(root: &Path, rel: &str, w: u32, h: u32, px: impl Fn(u32, u32) -> [u8; 4]) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut data = Vec::new();
    for y in 0..h {
        for x in 0..w {
            data.extend_from_slice(&px(x, y));
        }
    }
    let f = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    let mut enc = png::Encoder::new(f, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&data).unwrap();
}

/// The fixture project, and what each panel needs in it.
fn gate_project(root: &Path) {
    common::project(root);
    common::write(
        root,
        "scenes/arena/lamp.toml",
        "[Transform]\npos = [5.0, 0.0]\n\n[Light]\nintensity = 1.0\n",
    );
    common::write(root, "defs/timeline.toml", SHOW);
    png(root, "assets/hero.png", 32, 8, |_, _| [230, 110, 60, 255]);
    common::write(
        root,
        "assets/hero.toml",
        "grid = [8, 1]\n\n[clips.idle]\nframes = [0]\n\n[clips.run]\nfrom = 1\nto = 4\nfps = 12\nloop = true\n",
    );
    common::write(root, "assets/anim/hero.toml", MACHINE);
    common::write(
        root,
        "scenes/arena/hero.toml",
        "[Transform]\npos = [2.0, 0.0]\n\n[Sprite]\natlas = \"hero\"\n\n[Animator]\nmachine = \"anim/hero\"\n\n[Health]\ncur = 3\nmax = 10\n",
    );
    png(root, "assets/tiles/rock.png", 16, 16, |x, y| {
        let i = (y / 4) * 4 + x / 4;
        [(i * 15) as u8, 200 - (i * 10) as u8, 90, 255]
    });
    common::write(root, "assets/tiles/rock.toml", TILESET);
    common::write(
        root,
        "scenes/arena/camera.toml",
        "[Transform]\npos = [8.0, -4.0]\n\n[Camera]\nheight = 10.0\n",
    );
}

fn daemon(root: &Path, tag: &str) -> String {
    let name = format!("{}-{tag}", socket_name_for(root));
    let (r, n) = (root.to_path_buf(), name.clone());
    std::thread::spawn(move || {
        let d = common::open(&r);
        let _ = serve(d, Transport::Socket(n), Some(&r));
    });
    for _ in 0..300 {
        if Client::connect(&name).is_ok() {
            return name;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("daemon {tag} did not come up");
}

fn until(h: &mut Harness<'_, Editor>, what: &str, ok: impl Fn(&Editor) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        h.step();
        if ok(h.state()) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!(
        "timed out waiting for {what}; problem {:?}; status {}",
        h.state().problem,
        h.state().status
    );
}

fn result(c: &mut Client, method: &str, params: Json) -> Json {
    let r = c.call(method, params).unwrap();
    assert!(r.get("error").is_none(), "{method}: {r}");
    r["result"].clone()
}

fn tick(ed: &Editor) -> u64 {
    ed.status["tick"].as_u64().unwrap_or(0)
}

fn field(ed: &Editor, c: &str, f: &str) -> Json {
    ed.inspector.got["components"][c][f].clone()
}

fn centre_of(h: &Harness<'_, Editor>, label: &str, role: Role) -> Pos2 {
    let b = h
        .get_all_by_label(label)
        .find(|n| n.role() == role)
        .unwrap_or_else(|| panic!("no {role:?} {label:?}"))
        .bounding_box()
        .unwrap();
    Pos2::new(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32)
}

fn button(h: &mut Harness<'_, Editor>, p: Pos2, pressed: bool) {
    h.input_mut().events.push(Event::PointerMoved(p));
    h.input_mut().events.push(Event::PointerButton {
        pos: p,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    });
    h.step();
}

/// Press at a point, move to another in steps, let go: a person's drag.
fn drag(h: &mut Harness<'_, Editor>, from: Pos2, to: Pos2) {
    button(h, from, true);
    for i in 1..=8 {
        let p = from + (to - from) * (i as f32 / 8.0);
        h.input_mut().events.push(Event::PointerMoved(p));
        h.step();
        std::thread::sleep(Duration::from_millis(20));
    }
    button(h, to, false);
    h.step();
}

fn click_at(h: &mut Harness<'_, Editor>, p: Pos2) {
    h.input_mut().events.push(Event::PointerMoved(p));
    h.step();
    button(h, p, true);
    button(h, p, false);
}

fn press(h: &mut Harness<'_, Editor>, key: Key) {
    for pressed in [true, false] {
        h.input_mut().events.push(Event::Key {
            key,
            physical_key: Some(key),
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        });
        h.step();
        std::thread::sleep(Duration::from_millis(60));
        h.step();
    }
}

/// The scrubber's x for a tick.
fn run_x(h: &Harness<'_, Editor>, t: u64) -> Pos2 {
    let b = h
        .get_all_by_label("Run")
        .find(|n| n.role() == Role::Slider)
        .expect("the run's track")
        .bounding_box()
        .unwrap();
    let (lo, hi) = h.state().scrubber.range();
    let f = (t - lo) as f32 / (hi - lo).max(1) as f32;
    Pos2::new(
        b.x0 as f32 + (b.x1 - b.x0) as f32 * f,
        ((b.y0 + b.y1) / 2.0) as f32,
    )
}

/// Every file of a project but its `.argus`, by path.
fn files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            let rel = p
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if rel == ".argus" {
                continue;
            }
            if p.is_dir() {
                walk(root, &p, out);
            } else {
                out.insert(rel, std::fs::read(&p).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

/// What the two worlds are compared on: the world's digest, the clock, and
/// the world in use.
fn world(c: &mut Client) -> Json {
    let s = result(c, "sim.status", json!({ "digest": true }));
    json!({
        "digest": s["digest"], "tick": s["tick"], "frame": s["frame"], "speed": s["speed"],
        "playing": s["playing"], "paused": s["paused"], "fork": s["fork"],
    })
}

#[test]
fn every_panels_changes_are_the_feeds_commands_and_make_the_same_world() {
    let (ta, tb) = (common::temp("gate-a"), common::temp("gate-b"));
    gate_project(ta.path());
    gate_project(tb.path());
    let a = daemon(ta.path(), "gate-a");
    let mut agent = Client::connect(&a).unwrap();
    result(
        &mut agent,
        "session.identify",
        json!({ "author": "agent:claude" }),
    );
    let mut h = Harness::builder()
        .with_size(egui::vec2(1600.0, 1000.0))
        .wgpu()
        .build_state(|ctx, ed: &mut Editor| ed.ui(ctx), {
            let mut ed = Editor::new(&a, "human");
            ed.audio = false;
            ed
        });
    until(&mut h, "the game's frames", |ed| {
        ed.status["project"] == "fixture" && ed.viewport.view.is_some()
    });
    let mut did: Vec<&str> = Vec::new();

    // The bar: a step, the speeds.
    h.get_by_label("Step").click();
    until(&mut h, "a step", |ed| tick(ed) == 1);
    h.get_by_label("3x").click();
    until(&mut h, "3x", |ed| ed.status["speed"] == 3.0);
    h.get_by_label("1x").click();
    until(&mut h, "1x", |ed| ed.status["speed"] == 1.0);
    did.push("bar");

    // The agent walks the player, for a run to look at.
    result(
        &mut agent,
        "input.inject",
        json!({ "action": "move", "value": [1, 0], "frames": 120 }),
    );
    result(&mut agent, "sim.step", json!({ "n": 239 }));
    until(&mut h, "tick 240", |ed| tick(ed) == 240);

    // The hierarchy and the inspector: the door's box ticked, undone,
    // redone; the player locked and unlocked; a light added to the player.
    h.get_by_label("@door").click();
    until(&mut h, "the door inspected", |ed| {
        ed.inspector.got["id"] == "door"
    });
    h.step();
    h.get_all_by_role(Role::CheckBox)
        .find(|n| n.label().is_none_or(|l| l.is_empty()))
        .expect("the door's box")
        .click();
    until(&mut h, "the door locked", |ed| {
        field(ed, "Door", "locked") == true
    });
    h.get_by_label("Undo").click();
    until(&mut h, "the door back", |ed| {
        field(ed, "Door", "locked") == false
    });
    h.get_by_label("Redo").click();
    until(&mut h, "the door locked again", |ed| {
        field(ed, "Door", "locked") == true
    });
    h.get_by_label("@player").click();
    until(&mut h, "the player inspected", |ed| {
        ed.inspector.got["id"] == "player"
    });
    h.step();
    h.get_all_by_label("Lock")
        .next()
        .expect("the entity's Lock")
        .click();
    until(&mut h, "the lock", |ed| {
        ed.inspector.locks.iter().any(|l| l["author"] == "human")
    });
    h.step();
    h.get_by_label("Unlock").click();
    until(&mut h, "no lock", |ed| ed.inspector.locks.is_empty());
    h.get_by_role_and_label(Role::ComboBox, "Component to add")
        .click();
    h.step();
    h.get_all_by_label("Light")
        .last()
        .expect("Light in the list")
        .click();
    h.step();
    h.press_key(Key::Escape);
    h.get_by_label("Add").click();
    until(&mut h, "the player's light", |ed| {
        ed.inspector.got["components"]["Light"].is_object()
    });
    did.push("hierarchy and inspector");

    // The tuning panel: the lamp's light, dragged, kept.
    h.get_by_label("Tuning").click();
    until(&mut h, "the lamp's slider", |ed| {
        ed.tuning
            .rows
            .iter()
            .any(|r| r["entity"] == "@lamp" && r["path"] == "Light.intensity")
    });
    h.state_mut().tuning.filter = "lamp".into();
    h.step();
    h.step();
    let b = h
        .get_all_by_label("intensity")
        .find(|n| n.role() == Role::Slider)
        .expect("the slider")
        .bounding_box()
        .unwrap();
    let y = ((b.y0 + b.y1) / 2.0) as f32;
    let x = |f: f32| b.x0 as f32 + (b.x1 - b.x0) as f32 * f;
    drag(&mut h, Pos2::new(x(0.1), y), Pos2::new(x(0.4), y));
    until(&mut h, "the session's path", |ed| {
        ed.tuning
            .story
            .iter()
            .any(|l| l.contains("@lamp Light.intensity"))
    });
    h.state_mut().tuning.note = "a brighter lamp".into();
    h.step();
    h.get_by_label("Keep").click();
    until(&mut h, "kept", |ed| ed.tuning.session.is_null());
    h.state_mut().tuning.filter.clear();
    did.push("tuning");

    // The agent asks and proposes; the person answers with a key, tries
    // the proposal, and keeps it.
    result(
        &mut agent,
        "obs.review",
        json!({ "question": "does the lamp read as warm?", "options": ["yes", "no"] }),
    );
    let tx = result(
        &mut agent,
        "edit.begin",
        json!({ "author": "agent:claude", "note": "tougher player" }),
    )["tx"]
        .clone();
    result(
        &mut agent,
        "edit.set",
        json!({ "tx": tx, "entity": "@player", "path": "Health.max", "value": 5 }),
    );
    result(
        &mut agent,
        "edit.propose",
        json!({ "tx": tx, "note": "a tougher player" }),
    );
    until(&mut h, "the agent's queue", |ed| ed.agent.waiting() == 2);
    h.step();
    h.get_by_label_contains("The agent asks:").click();
    h.step();
    assert_eq!(h.state().tab, Tab::Agent);
    press(&mut h, Key::Num1);
    until(&mut h, "answered", |ed| ed.agent.reviews.is_empty());
    h.get_by_label("Try it").click();
    until(&mut h, "the trial", |ed| {
        ed.agent.proposals["trial"]["showing"] == "B"
    });
    h.step();
    h.get_by_label("A: as it is").click();
    until(&mut h, "A", |ed| {
        ed.agent.proposals["trial"]["showing"] == "A"
    });
    h.step();
    h.get_by_label("B: tougher player").click();
    until(&mut h, "B again", |ed| {
        ed.agent.proposals["trial"]["showing"] == "B"
    });
    h.step();
    h.get_by_label("Keep B").click();
    until(&mut h, "kept", |ed| ed.agent.proposals["pending"] == 0);
    did.push("agent panel");

    // The viewport: play, click into the game, F8, a note, Enter; then
    // Stop, back where play began.
    let before = tick(h.state());
    h.get_by_label("Play").click();
    until(&mut h, "playing", |ed| ed.status["playing"] == true);
    let r = h.state().viewport.image_rect.unwrap();
    click_at(&mut h, r.center());
    assert!(h.state().viewport.focused, "the keys in the game");
    press(&mut h, Key::F8);
    let deadline = Instant::now() + Duration::from_secs(10);
    while result(&mut agent, "mark.list", json!({}))["rows"][0]["id"].is_null() {
        assert!(Instant::now() < deadline, "F8 dropped a mark");
        h.step();
    }
    h.input_mut()
        .events
        .push(Event::Text("the lamp flickers".into()));
    h.step();
    press(&mut h, Key::Enter);
    let deadline = Instant::now() + Duration::from_secs(10);
    while result(&mut agent, "mark.get", json!({ "id": 1 }))["note"] != "the lamp flickers" {
        assert!(Instant::now() < deadline, "the note arrived");
        h.step();
        std::thread::sleep(Duration::from_millis(20));
    }
    h.get_by_label("Stop").click();
    until(&mut h, "stopped where play began", |ed| {
        ed.status["playing"] == false && tick(ed) == before
    });
    did.push("viewport");

    // The agent panel again: the mark's note set right, and closed.
    until(&mut h, "the mark listed", |ed| ed.agent.marks.len() == 1);
    h.state_mut()
        .agent
        .mark_notes
        .insert(1, "the lamp flickers at night".into());
    h.step();
    h.get_by_label("Save note").click();
    until(&mut h, "the note saved", |ed| {
        ed.agent
            .marks
            .first()
            .is_some_and(|m| m["note"] == "the lamp flickers at night")
    });
    h.get_by_label("Close").click();
    until(&mut h, "the mark closed", |ed| ed.agent.marks.is_empty());

    // The scrubber: dragged back, a world tried from there and stepped,
    // back to the run.
    until(&mut h, "the run on the track", |ed| {
        ed.scrubber.ring["end"].as_u64().is_some_and(|e| e >= 240)
    });
    h.step();
    // The run reaches back past the trial: the pick kept its past.
    assert_eq!(
        h.state().scrubber.range().0,
        0,
        "{}",
        h.state().scrubber.ring
    );
    let (from, to) = (run_x(&h, before), run_x(&h, 120));
    drag(&mut h, from, to);
    until(&mut h, "back near 120", |ed| {
        tick(ed).abs_diff(120) <= 3 && ed.scrubber.at.is_none()
    });
    let at = tick(h.state());
    h.get_by_label("Try from here").click();
    until(&mut h, "in try-1", |ed| ed.scrubber.ring["fork"] == "try-1");
    h.get_by_label("Frame +1").click();
    until(&mut h, "a step in it", |ed| tick(ed) == at + 1);
    h.get_all_by_label("World")
        .find(|n| n.role() == Role::ComboBox)
        .expect("the world list")
        .click();
    h.step();
    h.get_by_label("main").click();
    until(&mut h, "in main", |ed| ed.scrubber.ring["fork"] == "main");
    h.press_key(Key::Escape);
    did.push("scrubber");

    // The profiler: a slow tick, gone to.
    h.get_by_label("Profile").click();
    until(&mut h, "the worst ticks", |ed| {
        ed.profile.worst["rows"]
            .as_array()
            .is_some_and(|r| !r.is_empty())
    });
    h.step();
    let seeks = |ed: &Editor| {
        ed.commands
            .iter()
            .filter(|l| l.starts_with("argus sim seek"))
            .count()
    };
    let n = seeks(h.state());
    h.get_all_by_label("Go there")
        .find(|n| !n.is_disabled())
        .expect("a tick of this run to go to")
        .click();
    until(&mut h, "gone there", |ed| seeks(ed) == n + 1);
    did.push("profiler");

    // A failing test's replay, saved by the agent, opened from the list.
    result(
        &mut agent,
        "replay.save",
        json!({ "dir": ".argus/replays/gate/seed1", "note": "the lamp went dark" }),
    );
    until(&mut h, "the replay listed", |ed| {
        ed.scrubber.replays["total"] == 1
    });
    h.step();
    h.get_by_label("Replays (1)").click();
    h.step();
    h.get_by_label("Open").click();
    until(&mut h, "the replay open", |ed| {
        ed.commands
            .iter()
            .any(|l| l.starts_with("argus replay open"))
            && ed.problem.is_none()
    });
    h.press_key(Key::Escape);
    for _ in 0..10 {
        h.step();
    }

    // The tiles: the agent's map, painted with the pencil; a new map.
    result(
        &mut agent,
        "scene.tiles.create",
        json!({ "file": MAP, "width": 32, "height": 16, "chunk": 16,
                "layers": [{ "name": "ground", "tileset": "tiles/rock", "collision": true }] }),
    );
    let tx = result(
        &mut agent,
        "edit.begin",
        json!({ "author": "agent:claude", "note": "a room" }),
    )["tx"]
        .clone();
    result(
        &mut agent,
        "edit.spawn",
        json!({ "tx": tx, "id": "cave", "components": { "Tilemap": { "map": MAP }, "Transform": { "pos": [0.0, 0.0] } } }),
    );
    result(&mut agent, "edit.commit", json!({ "tx": tx }));
    h.get_by_label("Tiles").click();
    until(&mut h, "the map and its tileset", |ed| {
        ed.tiles.current().is_some_and(|m| m["entity"] == "@cave")
            && ed.tiles.tileset_frames("tiles/rock") == 16
            && ed.tiles.layer.as_deref() == Some("ground")
            && ed.viewport.view.is_some()
    });
    h.step();
    h.get_by_label("tile 16").click();
    h.step();
    let cell = |h: &Harness<'_, Editor>, c: [i64; 2]| {
        let m = h.state().mapping().expect("a frame with its view");
        h.state().tiles.screen_of_cell(&m, c).expect("on the map")
    };
    let (p, q) = (cell(&h, [3, 2]), cell(&h, [6, 2]));
    drag(&mut h, p, q);
    until(&mut h, "the stroke landed", |ed| {
        ed.tiles.pending.is_none() && ed.tiles.stroke.is_empty()
    });
    until(&mut h, "the tilesets", |ed| {
        ed.tiles.sprites.iter().any(|s| s == "tiles/rock")
    });
    h.state_mut().tiles.new_map.file = "scenes/arena/deco.tilemap.toml".into();
    h.state_mut().tiles.new_map.tileset = "tiles/rock".into();
    h.state_mut().tiles.new_map.width = 8;
    h.state_mut().tiles.new_map.height = 4;
    h.step();
    h.get_by_label("New tilemap").click();
    h.step();
    h.get_by_label("Create, its corner at the view's top left")
        .click();
    until(&mut h, "the new map placed", |ed| {
        ed.tiles.maps.iter().any(|m| m["entity"] == "@deco")
    });
    did.push("tiles");

    // The animator: the hero's run, an event added.
    h.get_by_label("@hero").click();
    h.get_by_label("Animator").click();
    until(&mut h, "the hero's machine", |ed| {
        ed.animator.machine.as_deref() == Some("anim/hero")
            && ed.animator.sidecar["doc"]["states"]["run"]["clip"] == "run"
            && ed.animator.clips["clips"]["run"].is_object()
    });
    h.step();
    h.get_by_label("idle  (clip idle, 1 frame)").click();
    h.get_by_label("run  (clip run, 4 frames)").click();
    h.step();
    h.step();
    h.get_all_by_label("+ event")
        .next()
        .expect("+ event")
        .click();
    until(&mut h, "the event", |ed| {
        ed.animator.sidecar["doc"]["states"]["run"]["events"]
            .as_array()
            .is_some_and(|e| e.len() == 2)
    });
    did.push("animator");

    // The timeline: played with the lamp, a cue dragged sooner, a cue
    // added, scrubbed back.
    h.get_by_label("@lamp").click();
    h.get_by_label("Timeline").click();
    until(&mut h, "the show", |ed| {
        ed.tab == Tab::Timeline && ed.timelines.cues().len() == 2
    });
    for _ in 0..3 {
        h.step();
    }
    h.get_all_by_label("selected")
        .next()
        .expect("Actor: selected")
        .click();
    h.step();
    h.get_by_label("Play timeline").click();
    until(&mut h, "a play", |ed| ed.timelines.playing().is_some());
    result(&mut agent, "sim.step", json!({ "n": 100 }));
    until(&mut h, "1.6 s in", |ed| {
        ed.timelines.head().is_some_and(|t| t > 1.6)
    });
    h.step();
    let p = centre_of(&h, "show.flare", Role::Button);
    drag(&mut h, p, p + egui::vec2(-45.0, 0.0));
    until(&mut h, "the flare sooner", |ed| {
        ed.timelines
            .cues()
            .iter()
            .any(|c| c.key == "show.flare" && c.at < 1.5)
    });
    h.get_all_by_label("effects")
        .rfind(|n| n.role() == Role::TextInput)
        .expect("the new cue's effects")
        .type_text("actor:Light.intensity = 2; emit dimmed");
    h.step();
    h.get_by_label("Add cue").click();
    until(&mut h, "the new cue", |ed| ed.timelines.cues().len() == 3);
    h.step();
    let b = h.get_by_label("Timeline time").bounding_box().unwrap();
    let y = ((b.y0 + b.y1) / 2.0) as f32;
    let w = (b.x1 - b.x0) as f32;
    drag(
        &mut h,
        Pos2::new(b.x0 as f32 + w * 0.7, y),
        Pos2::new(b.x0 as f32 + w * 0.3, y),
    );
    until(&mut h, "the scrub", |ed| {
        ed.commands
            .iter()
            .any(|l| l.starts_with("argus timeline seek"))
            && ed.timelines.note.is_none()
    });
    did.push("timeline");

    // The change feed: a read.
    h.get_by_label("Change feed").click();
    h.step();
    assert!(h.state().problem.is_none(), "{:?}", h.state().problem);
    for _ in 0..20 {
        h.step();
        std::thread::sleep(Duration::from_millis(10));
    }

    // The whole feed, every author's rows in order.
    let feed = result(&mut agent, "edit.feed", json!({ "limit": 5000 }));
    let mut rows = feed["rows"].as_array().unwrap().clone();
    rows.sort_by_key(|r| r["id"].as_u64().unwrap_or(0));
    let lines: Vec<String> = rows
        .iter()
        .map(|r| r["command"].as_str().unwrap_or("").to_string())
        .collect();
    assert!(lines.iter().all(|l| l.starts_with("argus ")), "{lines:#?}");
    let by_human = rows.iter().filter(|r| r["author"] == "human").count();
    println!(
        "the gate's feed: {} rows, {by_human} by the person, panels {did:?}",
        lines.len()
    );
    for l in &lines {
        println!("  {l}");
    }
    let wa = world(&mut agent);
    let fa = files(ta.path());
    drop(h);

    // Typed into the CLI against a fresh daemon on a fresh copy.
    let b = daemon(tb.path(), "gate-b");
    for l in &lines {
        let mut argv = vec!["--socket".to_string(), b.clone()];
        argv.extend(command::split(l).unwrap().into_iter().skip(1));
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run(&argv, &mut out, &mut err);
        assert_eq!(
            code,
            EXIT_OK,
            "{l}: {}{}",
            String::from_utf8_lossy(&out),
            String::from_utf8_lossy(&err)
        );
    }
    let mut cb = Client::connect(&b).unwrap();
    let wb = world(&mut cb);
    assert_eq!(wa, wb, "the same world from the feed's commands");
    let fb = files(tb.path());
    let differ: Vec<&String> = fa
        .keys()
        .chain(fb.keys())
        .filter(|k| fa.get(*k) != fb.get(*k))
        .collect();
    assert!(differ.is_empty(), "files that differ: {differ:?}");
}
