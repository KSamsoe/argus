//! Phase 5 item 4: Delve's floor is a runtime map (`maps/floor.tilemap.toml`,
//! painted by `systems/floor.luau` from the seed), so its cells are part of
//! the run. A report a player makes after taking the stairs replays to the
//! same world and the same floor, whether its keyframe is before the
//! descent (the replay paints floor 2 again) or after it (the keyframe
//! holds the cells); and Delve built into release packs, which ship the
//! map as its index file and no cells, plays as Delve from its folder.

use argus_player::pack::{self, BuildOptions, Pack};
use argus_player::Player;
use argus_tiles::RuntimeCells;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

const MAP: &str = "maps/floor.tilemap.toml";

fn folder() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/delve")
}

fn delve(root: &Path) -> Player {
    Player::open(root, None, None).unwrap()
}

/// The runtime maps' cells, as a keyframe holds them.
fn cells(p: &Player) -> RuntimeCells {
    p.sources.tiles.as_ref().unwrap().borrow().runtime_cells()
}

/// The cells the game plays on: the floor and walls layers. Delve's art
/// paints three more (stone, shade, decor), and decor changes with what
/// happens (a monster's remains, the hero's blood).
fn play_cells(all: &RuntimeCells) -> RuntimeCells {
    all.clone()
        .into_iter()
        .map(|(file, cs)| {
            let kept = cs
                .into_iter()
                .filter(|(layer, ..)| layer == "floor" || layer == "walls")
                .collect();
            (file, kept)
        })
        .collect()
}

/// The floor's walls layer, top row first: `#` a wall, `.` open.
fn walls(p: &Player) -> Vec<String> {
    let st = p.sources.tiles.as_ref().unwrap();
    let mut st = st.borrow_mut();
    let m = st.open(MAP).unwrap();
    let li = m.layer("walls").unwrap();
    let (w, h) = (m.index.width, m.index.height);
    (0..h)
        .map(|y| {
            (0..w)
                .map(|x| {
                    if m.cell(li, x, y).unwrap() == 0 {
                        '.'
                    } else {
                        '#'
                    }
                })
                .collect()
        })
        .collect()
}

fn uint(p: &Player, id: &str, path: &str) -> u64 {
    let e = p.world.entity(id).unwrap();
    match p.world.get_field(e, path).unwrap() {
        argus_ecs::Value::UInt(n) => n,
        v => panic!("{id} {path}: {v:?}"),
    }
}

fn dead(p: &Player) -> bool {
    let e = p.world.entity("@player").unwrap();
    p.world.get_field(e, "Player.dead").unwrap() == argus_ecs::Value::Bool(true)
}

fn hero(p: &Player) -> (i64, i64) {
    let [x, y] = p.position("@player").unwrap();
    (x as i64, y as i64)
}

fn stairs(p: &Player) -> (i64, i64) {
    let id = p.world.component_id("Stairs").unwrap();
    let e = p.world.query().has(id).entities()[0];
    let t = p.world.get::<argus_ecs::Transform>(e).unwrap();
    (t.pos.x as i64, t.pos.y as i64)
}

/// The hero's next step on the shortest way to the stairs over the map's
/// open cells, or None on them. World (x, y) is the map's cell
/// (x, height - 1 - y): its rows go down.
fn toward_stairs(p: &Player) -> Option<(i64, i64)> {
    let rows = walls(p);
    let (w, h) = (rows[0].len() as i64, rows.len() as i64);
    let open = |x: i64, y: i64| {
        x >= 0
            && y >= 0
            && x < w
            && y < h
            && rows[(h - 1 - y) as usize].as_bytes()[x as usize] == b'.'
    };
    let (from, to) = (hero(p), stairs(p));
    if from == to {
        return None;
    }
    // Steps to the stairs from every open cell, then down them.
    let dirs = [(1, 0), (-1, 0), (0, 1), (0, -1)];
    let mut dist = vec![u32::MAX; (w * h) as usize];
    let at = |x: i64, y: i64| (y * w + x) as usize;
    dist[at(to.0, to.1)] = 0;
    let mut queue = VecDeque::from([to]);
    while let Some((x, y)) = queue.pop_front() {
        for (dx, dy) in dirs {
            let (nx, ny) = (x + dx, y + dy);
            if open(nx, ny) && dist[at(nx, ny)] == u32::MAX {
                dist[at(nx, ny)] = dist[at(x, y)] + 1;
                queue.push_back((nx, ny));
            }
        }
    }
    let here = dist[at(from.0, from.1)];
    assert_ne!(
        here,
        u32::MAX,
        "no way from {from:?} to the stairs at {to:?}"
    );
    dirs.into_iter().find(|&(dx, dy)| {
        open(from.0 + dx, from.1 + dy) && dist[at(from.0 + dx, from.1 + dy)] + 1 == here
    })
}

/// A key pressed and let go, as a player does it; the turns it took.
fn press(p: &mut Player, key: &str) -> u64 {
    p.key(key, true) + p.key(key, false)
}

/// Walk the hero to the stairs with the arrow keys (hitting whatever
/// stands in the way) and take them: Enter asks, and the next turn makes
/// floor 2. Returns the tick that painted it.
fn walk_down(p: &mut Player) -> u64 {
    for _ in 0..80 {
        assert!(!dead(p), "the hero died on the way to the stairs");
        let Some(step) = toward_stairs(p) else { break };
        let key = match step {
            (1, 0) => "ArrowRight",
            (-1, 0) => "ArrowLeft",
            (0, 1) => "ArrowUp",
            _ => "ArrowDown",
        };
        assert_eq!(press(p, key), 1, "a step is one turn");
    }
    assert_eq!(hero(p), stairs(p), "on the stairs");
    assert_eq!(press(p, "Enter"), 1);
    assert_eq!(uint(p, "@floor", "Floor.n"), 1, "asked, not yet gone");
    let made = p.tick();
    assert_eq!(press(p, "Period"), 1);
    assert_eq!(uint(p, "@floor", "Floor.n"), 2);
    assert_eq!(uint(p, "@player", "Player.floor"), 2);
    made
}

#[test]
fn a_report_after_the_stairs_replays_to_the_same_floor() {
    let mut p = delve(&folder());
    p.record();
    let floor1 = cells(&p);
    let rows1 = walls(&p);
    assert_eq!((rows1.len(), rows1[0].len()), (20, 40));
    assert!(
        rows1.concat().matches('.').count() >= 40,
        "rooms: {rows1:#?}"
    );
    let made = walk_down(&mut p);
    let floor2 = cells(&p);
    assert_ne!(floor2, floor1, "floor 2 is another floor");
    assert_ne!(walls(&p), rows1);

    // Just after: the report reaches back before the descent, and its
    // replay paints floor 2 again from the same draws.
    let bytes = p.report("down the stairs", "report", "test").unwrap();
    let mut fresh = delve(&folder());
    assert_eq!(cells(&fresh), floor1, "a fresh run is on floor 1");
    let out = fresh.play_report(&bytes).unwrap();
    println!(
        "floor 2 painted in tick {made}; the first report replays from {} to {}",
        out["from"], out["to"]
    );
    assert!(out["from"].as_u64().unwrap() <= made, "{out}");
    assert_eq!(out["same"], true, "{out}");
    assert_eq!(cells(&fresh), floor2, "the replayed floor is the run's");
    assert_eq!(walls(&fresh), walls(&p));

    // Later: both of the recorder's keyframes are past the descent, so the
    // report's cells come from its keyframe.
    while p.recorder.as_ref().unwrap().from() <= made {
        assert_eq!(press(&mut p, "Period"), 1);
    }
    // Waiting changes no cell the game plays on; the art's decor may have
    // gained the hero's blood.
    let now = cells(&p);
    assert_eq!(
        play_cells(&now),
        play_cells(&floor2),
        "waiting leaves the floor as it is"
    );
    let bytes = p.report("floor 2, later", "report", "test").unwrap();
    let mut fresh = delve(&folder());
    let out = fresh.play_report(&bytes).unwrap();
    println!("the later one from {} to {}", out["from"], out["to"]);
    assert!(out["from"].as_u64().unwrap() > made, "{out}");
    assert_eq!(out["same"], true, "{out}");
    assert_eq!(
        cells(&fresh),
        now,
        "the keyframe's floor, the art's layers too"
    );
    assert_eq!(walls(&fresh), walls(&p));
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let name = e.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let (a, b) = (e.path(), to.join(&name));
        if a.is_dir() {
            copy(&a, &b);
        } else {
            std::fs::copy(&a, &b).unwrap();
        }
    }
}

/// A temp directory removed on drop, used as its path.
struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl std::ops::Deref for Dir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

fn temp(tag: &str) -> Dir {
    let dir = std::env::temp_dir().join(format!("argus-delve-floor-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    Dir(dir)
}

/// The world's summary, the Luau heap's size left out (it depends on when
/// the collector ran, and on bytecode or source).
fn world(p: &Player) -> String {
    let mut s = p.status();
    s.as_object_mut().unwrap().remove("script_bytes");
    s.to_string()
}

/// Packs written as a build's data/ directory and mounted at a root that
/// is nowhere on disk.
fn mount(packs: &[Pack]) -> (Dir, PathBuf) {
    let base = temp("data");
    let data = base.join("data");
    std::fs::create_dir_all(&data).unwrap();
    for p in packs {
        std::fs::write(data.join(p.file_name()), p.encode()).unwrap();
    }
    let root = base.join("game");
    pack::mount_dir(&data, &root, None).unwrap();
    (base, root)
}

#[test]
fn delve_from_its_packs_plays_as_delve_from_its_folder() {
    // A copy: a build imports the assets into the copy's cache.
    let dir = temp("game");
    copy(&folder(), &dir);
    let built = pack::build(
        &dir,
        &BuildOptions {
            code: "test".into(),
            since: vec![],
        },
    )
    .unwrap();
    let files: Vec<&str> = built.packs[0].files.iter().map(|f| f.0.as_str()).collect();
    assert!(
        files.contains(&MAP),
        "the map ships as its index: {files:?}"
    );
    assert_eq!(
        files.iter().filter(|f| f.starts_with("maps/")).count(),
        1,
        "and no cells: {files:?}"
    );
    assert!(
        !files.contains(&"assets/tiles/floor.png"),
        "the floor's tile is on the shared pages: {files:?}"
    );
    let (_data, root) = mount(&built.packs);

    let mut a = delve(&dir);
    let mut b = delve(&root);
    assert_eq!(world(&b), world(&a));
    assert_eq!(cells(&b), cells(&a), "the same first floor");
    let made = (walk_down(&mut a), walk_down(&mut b));
    assert_eq!(made.0, made.1);
    assert_eq!(world(&b), world(&a));
    assert_eq!(cells(&b), cells(&a), "the same second floor");
    for _ in 0..10 {
        press(&mut a, "Period");
        press(&mut b, "Period");
    }
    assert_eq!(world(&b), world(&a), "the same turns on it");
}
