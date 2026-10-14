//! 2D staples item 1: debug drawing from scripts. A system's shapes are
//! drawn over the world in a picture and listed in the semantic frame;
//! they stay until that system runs again; a bad option is a script
//! error; and a release player keeps none.

mod common;

use common::*;
use serde_json::{json, Value as Json};
use std::path::Path;

fn project(root: &Path) {
    write(
        root,
        "argus.toml",
        "name = \"draws\"\nclock = \"continuous@60\"\ndefault_scene = \"main\"\nplugins = []\n",
    );
    write(
        root,
        "systems/mark.luau",
        r##"system "sim.mark" {
  stage = "sim",
  doc = "Draws one of each shape.",
  run = function(q)
    q:draw_line(vector.create(-20, 0, 0), vector.create(20, 0, 0), { color = "#ff0000", width = 6 })
    q:draw_box(vector.create(10, 10, 0), vector.create(4, 4, 0), { color = "#00ff00", fill = true })
    q:draw_circle(vector.create(-10, 10, 0), 2, { color = "#0000ff", fill = true })
    q:draw_circle(vector.create(-10, -10, 0), 3, { color = "#ff00ff" })
    q:draw_arrow({ 0, -5 }, { 5, -5 })
    q:draw_text(vector.create(10, -10, 0), "hi", { size = 20 })
  end,
}
"##,
    );
    write(
        root,
        "systems/blink.luau",
        r#"system "sim.blink" {
  stage = "sim",
  doc = "Draws on even ticks only.",
  run = function(q)
    if q.tick % 2 == 0 then q:draw_circle(vector.create(0, 0, 0), 1) end
  end,
}
"#,
    );
    write(
        root,
        "systems/careless.luau",
        r##"system "sim.careless" {
  stage = "sim",
  doc = "Names a colour the way CSS does.",
  run = function(q)
    if q.tick == 4 then q:draw_line({ 0, 0 }, { 1, 1 }, { color = "red" }) end
  end,
}
"##,
    );
    write(
        root,
        "scenes/main/scene.toml",
        "clock = \"continuous@60\"\nseed = 1\n",
    );
}

fn shapes(d: &mut argus_daemon::Daemon, by: &str) -> Vec<Json> {
    let f = ok(d, "obs.frame", json!({ "format": "semantic" }));
    f["debug"]["shapes"]
        .as_array()
        .map(|a| a.iter().filter(|s| s["by"] == by).cloned().collect())
        .unwrap_or_default()
}

#[test]
fn a_script_draws_shapes_over_the_world_until_it_runs_again() {
    let t = temp("draws");
    project(t.path());
    let mut d = open(t.path());
    ok(&mut d, "sim.step", json!({ "n": 2 }));

    // No camera: the view is the origin, 40 units tall, so a 640 x 360
    // frame is 9 pixels a unit, x = 320 + 9 wx, y = 180 - 9 wy.
    let marks = shapes(&mut d, "sim.mark");
    assert_eq!(marks.len(), 6, "{marks:?}");
    assert_eq!(marks[0]["line"], json!([140.0, 180.0, 500.0, 180.0]));
    assert_eq!(marks[0]["color"], "#ff0000");
    assert_eq!(marks[1]["box"], json!([392.0, 72.0, 428.0, 108.0]));
    assert_eq!(marks[1]["fill"], true);
    assert_eq!(marks[2]["circle"], json!([230.0, 90.0, 18.0]));
    assert_eq!(marks[3]["circle"], json!([230.0, 270.0, 27.0]));
    assert_eq!(marks[4]["arrow"], json!([320.0, 225.0, 365.0, 225.0]));
    assert_eq!(marks[4]["color"], "#ffd23f", "amber unless told");
    assert_eq!(marks[5]["text"], "hi");
    assert_eq!(marks[5]["at"], json!([410.0, 270.0]));

    // sim.blink ran last on tick 1 and drew nothing: its tick-0 circle is
    // gone. On tick 2 it draws again.
    assert_eq!(shapes(&mut d, "sim.blink").len(), 0);
    ok(&mut d, "sim.step", json!({ "n": 1 }));
    assert_eq!(shapes(&mut d, "sim.blink").len(), 1);
    // A frame between ticks (the game paused) still has them.
    assert_eq!(shapes(&mut d, "sim.blink").len(), 1);

    // Drawn: over the world, in the colours asked for.
    if let Some(r) = frame(&mut d, json!({ "width": 640, "height": 360 })) {
        let (w, _, px) = argus_assets::decode_png(Path::new(r["path"].as_str().unwrap())).unwrap();
        let at = |x: u32, y: u32| {
            let i = ((y * w + x) * 4) as usize;
            [px[i], px[i + 1], px[i + 2]]
        };
        let near = |p: [u8; 3], q: [u8; 3]| p.iter().zip(q).all(|(a, b)| a.abs_diff(b) < 40);
        assert!(
            near(at(200, 180), [255, 0, 0]),
            "the line: {:?}",
            at(200, 180)
        );
        assert!(near(at(410, 90), [0, 255, 0]), "the box: {:?}", at(410, 90));
        assert!(
            near(at(230, 90), [0, 0, 255]),
            "the disc: {:?}",
            at(230, 90)
        );
        assert!(
            !near(at(230, 270), [255, 0, 255]),
            "an outline is hollow: {:?}",
            at(230, 270)
        );
        let ring = (250..262).any(|x| near(at(x, 270), [255, 0, 255]));
        assert!(ring, "the outline at 27 px: {:?}", at(257, 270));
    }

    // A colour by name is refused, saying what a colour is.
    ok(&mut d, "sim.step", json!({ "n": 2 }));
    let errs = ok(&mut d, "script.errors", json!({})).to_string();
    assert!(
        errs.contains("sim.careless") && errs.contains("#rrggbb"),
        "{errs}"
    );
}

#[test]
fn a_release_player_keeps_no_shapes() {
    let t = temp("draws-release");
    project(t.path());
    let mut p = argus_player::Player::open(t.path(), None, None).unwrap();
    p.step(3).unwrap();
    assert!(!p.draws().borrow().on());
    assert!(p.draws().borrow().is_empty());
}
