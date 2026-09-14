//! §17 targets this crate owns. Run in release:
//! `cargo test -p argus-scene --release --test perf -- --ignored --nocapture`.
//!
//! - Hollow: 10,000 authored entities across 200 rooms.
//! - Cold scene open with a warm index: < 300 ms (there is no index yet, so
//!   this measures open plus resolve, the part that stays in the path).
//! - Room stream-in: < 4 ms (a phase-1 number, measured now for the record).

mod common;

use argus_scene::Scene;
use common::*;
use std::time::Instant;

fn big_project(root: &std::path::Path) {
    project(root);
    let mut scene =
        String::from("seed = 1\n\n[rooms]\nstart = \"r000\"\nradius = 1\n\n[rooms.graph]\n");
    for r in 0..200 {
        if r + 1 < 200 {
            scene.push_str(&format!("\"r{r:03}\" = [\"r{:03}\"]\n", r + 1));
        }
    }
    write(root, "scenes/big/scene.toml", &scene);
    for r in 0..200 {
        write(
            root,
            &format!("scenes/big/rooms/r{r:03}/room.toml"),
            "bounds = [0.0, 0.0, 64.0, 64.0]\n",
        );
        for i in 0..50 {
            let body = if i % 2 == 0 {
                format!(
                    "prefab = \"enemy/bat\"\n\n[Transform]\npos = [{}.0, {}.0]\n\n[Health]\ncur = {}\n",
                    i, r, i
                )
            } else {
                format!(
                    "prefab = \"door/locked\"\n\n[Transform]\npos = [{}.0, {}.0]\n\n[Door]\nleads_to = \"@e{:03}\"\n",
                    i,
                    r,
                    i - 1
                )
            };
            write(
                root,
                &format!("scenes/big/rooms/r{r:03}/e{i:03}.toml"),
                &body,
            );
        }
    }
}

#[test]
#[ignore]
fn cold_open_of_ten_thousand_entities_in_two_hundred_rooms() {
    let t = temp("perf-open");
    big_project(t.path());
    let p = open(t.path());

    let w = world();
    let start = Instant::now();
    let s = Scene::open(&p, "big", w.registry()).unwrap();
    let open_ms = start.elapsed().as_secs_f64() * 1000.0;
    assert!(
        s.issues().is_empty(),
        "{:?}",
        &s.issues()[..s.issues().len().min(3)]
    );
    assert_eq!(s.ids().len(), 10_000);
    println!("scene.open (parse + resolve, 10k entities, 200 rooms): {open_ms:.1} ms");
    assert!(open_ms < 300.0, "cold open {open_ms:.1} ms >= 300 ms");

    // Loading everything into a world, for the record.
    let mut w = world();
    let mut s = s;
    let start = Instant::now();
    let r = s.spawn_all(&mut w).unwrap();
    let all_ms = start.elapsed().as_secs_f64() * 1000.0;
    println!("spawn_all: {} entities in {all_ms:.1} ms", r.entities);
    assert_eq!(w.authored_count(), 10_000 + 5_000);
}

/// Not a target: where the open time goes, so the number above can be read.
#[test]
#[ignore]
fn where_the_open_time_goes() {
    let t = temp("perf-phases");
    big_project(t.path());
    let files = argus_scene::moves::content_files(t.path());
    let scene_files: Vec<&String> = files
        .iter()
        .filter(|f| f.starts_with("scenes/big/rooms/") && !f.ends_with("room.toml"))
        .collect();
    let start = Instant::now();
    let texts: Vec<String> = scene_files
        .iter()
        .map(|f| std::fs::read_to_string(t.path().join(f)).unwrap())
        .collect();
    let read_ms = start.elapsed().as_secs_f64() * 1000.0;
    let start = Instant::now();
    let docs: Vec<argus_scene::EntityDoc> = scene_files
        .iter()
        .zip(&texts)
        .map(|(f, text)| argus_scene::EntityDoc::parse(f, text).unwrap())
        .collect();
    let parse_ms = start.elapsed().as_secs_f64() * 1000.0;
    let p = open(t.path());
    let w = world();
    let ctx = p.ctx(w.registry());
    let start = Instant::now();
    let mut issues = Vec::new();
    let mut n = 0;
    for (f, d) in scene_files.iter().zip(&docs) {
        if f.ends_with("room.toml") {
            continue;
        }
        argus_scene::resolve::resolve(&ctx, f, d, f, &mut issues).unwrap();
        n += 1;
    }
    let resolve_ms = start.elapsed().as_secs_f64() * 1000.0;
    println!(
        "{} files: read {read_ms:.1} ms, parse {parse_ms:.1} ms, resolve {n} entities {resolve_ms:.1} ms",
        scene_files.len()
    );
}

#[test]
#[ignore]
fn room_stream_in_stays_under_a_frame() {
    let t = temp("perf-stream");
    big_project(t.path());
    let p = open(t.path());
    let mut w = world();
    let mut s = Scene::open(&p, "big", w.registry()).unwrap();
    s.spawn_around(&mut w, None).unwrap();
    let mut worst: f64 = 0.0;
    for r in 1..40 {
        let start = Instant::now();
        s.transition(&mut w, &format!("r{r:03}")).unwrap();
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        worst = worst.max(ms);
    }
    println!("room transition (one room in, one out, 75 entities each): worst {worst:.2} ms");
    assert!(worst < 4.0, "stream-in {worst:.2} ms >= 4 ms");
}
