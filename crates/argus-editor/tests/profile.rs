//! Phase 4 item 6: the profiler view, driven headless over a real socket
//! on a copy of the fixture named as Outpost, with a system too slow for
//! Outpost's tick. The view shows §17's rows beside what is measured, the
//! row over its bound names the system, the slowest tick is one click
//! away, and a capture writes its file.

#[path = "../../argus-daemon/tests/common/mod.rs"]
mod common;

use argus_client::Client;
use argus_daemon::{serve, socket_name_for, Transport};
use argus_editor::app::Tab;
use argus_editor::Editor;
use eframe::egui;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use serde_json::{json, Value as Json};
use std::path::Path;
use std::time::{Duration, Instant};

fn until(h: &mut Harness<'_, Editor>, what: &str, ok: impl Fn(&Editor) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        h.step();
        if ok(h.state()) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!(
        "timed out waiting for {what}; problem {:?}; targets {}",
        h.state().problem,
        h.state().profile.targets
    );
}

fn result(c: &mut Client, method: &str, params: Json) -> Json {
    let r = c.call(method, params).unwrap();
    assert!(r.get("error").is_none(), "{method}: {r}");
    r["result"].clone()
}

#[test]
fn a_person_sees_the_tick_over_outposts_target_and_the_system_that_did_it() {
    let t = common::temp("editor-profile");
    common::project(t.path());
    let toml =
        common::read(t.path(), "argus.toml").replace("name = \"fixture\"", "name = \"outpost\"");
    common::write(t.path(), "argus.toml", &toml);
    common::write(
        t.path(),
        "systems/slow.luau",
        r#"system "sim.slow" {
  stage = "sim",
  doc = "Counts to a large number every tick.",
  run = function(q)
    local x = 0
    for i = 1, 3000000 do x = x + i % 7 end
  end,
}
"#,
    );
    let name = format!("{}-profile", socket_name_for(t.path()));
    let (r, n) = (t.path().to_path_buf(), name.clone());
    std::thread::spawn(move || {
        let d = common::open(&r);
        let _ = serve(d, Transport::Socket(n), Some(&r));
    });
    let mut agent = loop {
        if let Ok(c) = Client::connect(&name) {
            break c;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    result(&mut agent, "sim.step", json!({ "n": 60 }));

    let mut h = Harness::builder()
        .with_size(egui::vec2(1500.0, 1700.0))
        .wgpu()
        .build_state(|ctx, ed: &mut Editor| ed.ui(ctx), {
            let mut ed = Editor::new(&name, "human");
            ed.audio = false;
            ed
        });
    until(&mut h, "the bar", |ed| ed.status["project"] == "outpost");
    h.get_by_label("Profile").click();
    until(&mut h, "the profile", |ed| {
        ed.profile.targets["game"] == "outpost"
            && ed.profile.systems["ticks"]
                .as_array()
                .is_some_and(|t| t.len() == 60)
            && ed.profile.worst["rows"]
                .as_array()
                .is_some_and(|w| !w.is_empty())
    });
    assert_eq!(h.state().tab, Tab::Profile);
    h.step();
    // The row over its bound, and the system to look at.
    assert!(h
        .get_all_by_label_contains(": look at sim.slow")
        .next()
        .is_some());
    h.get_by_label_contains("§17 targets for outpost: ");
    h.get_by_label("Sim tick at 10× speed, 200 colonists, 20k items");
    assert!(
        h.get_all_by_label("sim.slow").next().is_some(),
        "in the systems too"
    );
    h.get_by_label_contains("Heaviest: sim.slow");
    assert_eq!(h.state().profile.budget_ms(), 1.6);

    // The slowest tick, one click away.
    let worst = h.state().profile.worst["rows"][0]["tick"].as_u64().unwrap();
    h.get_all_by_label("Go there").next().unwrap().click();
    until(&mut h, "at the slowest tick", |ed| {
        ed.status["tick"] == worst
    });
    assert!(h
        .state()
        .commands
        .contains(&format!("argus sim seek frame={worst}")));

    // A capture of the next 20 ticks.
    h.state_mut().profile.capture_ticks = 20;
    h.step();
    h.get_by_label("Capture for Perfetto or Tracy").click();
    h.step();
    h.get_by_label("Capture").click();
    until(&mut h, "armed", |ed| ed.profile.capturing());
    result(&mut agent, "sim.step", json!({ "n": 25 }));
    until(&mut h, "written", |ed| ed.profile.captured.is_some());
    let path = h.state().profile.captured.clone().unwrap();
    assert!(!path.starts_with(r"\\?\"), "a path to paste: {path}");
    let trace = std::fs::read_to_string(&path).unwrap();
    assert!(trace.contains("sim.slow"), "every system's spans");
    assert!(h
        .state()
        .commands
        .iter()
        .any(|l| l == "argus prof capture ticks=20"));
    h.step();
    h.get_by_label("Copy path");

    let img = h.render().expect("drawn");
    let (w, hh) = img.dimensions();
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("editor-profile.png");
    let f = std::io::BufWriter::new(std::fs::File::create(&out).unwrap());
    let mut enc = png::Encoder::new(f, w, hh);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .unwrap()
        .write_image_data(&img.into_raw())
        .unwrap();
}
