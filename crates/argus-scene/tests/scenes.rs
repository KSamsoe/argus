//! Scenes, rooms, streaming, and the authored-versus-runtime split (§3, §4, §6).

mod common;

use argus_ecs::{Entity, Event, Parent, Value};
use argus_scene::{codes, Scene, SeedPolicy};
use common::*;

fn game_events(w: &argus_ecs::World) -> Vec<(String, Vec<(String, Value)>)> {
    w.events()
        .iter()
        .filter_map(|r| match &r.event {
            Event::Game { name, fields } => Some((name.clone(), fields.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn a_flat_scene_spawns_authored_entities_with_ids() {
    let t = temp("flat");
    project(t.path());
    let p = open(t.path());
    assert_eq!(p.scenes(), ["arena", "hollow"]);
    assert_eq!(p.config.default_scene.as_deref(), Some("arena"));

    let mut w = world();
    let mut s = Scene::open(&p, "arena", w.registry()).unwrap();
    assert!(s.issues().is_empty(), "{:?}", s.issues());
    assert_eq!(s.doc.clock.as_deref(), Some("discrete"));
    assert_eq!(s.doc.seed, SeedPolicy::Random);
    assert_eq!(s.doc.enable, ["sim.*"]);
    assert!(!s.has_rooms());

    let report = s.spawn_all(&mut w).unwrap();
    assert_eq!(report.entities, 2);
    assert_eq!(report.tags_registered, ["player"]);
    assert!(report.pending.is_empty());

    let player = w.entity("@player").unwrap();
    assert!(w.is_authored(player));
    assert_eq!(
        w.get::<Health>(player).unwrap(),
        Health { cur: 10, max: 10 }
    );
    assert!(w.has(player, w.component_id("player").unwrap()));
    let spawner = w.entity("@spawner.north").unwrap();
    assert_eq!(w.get::<Door>(spawner).unwrap().leads_to, player);
    assert_eq!(s.entity_of("@player"), Some(player));
    assert!(s.is_authored("player"));

    // Runtime entities are never authored and never written.
    let bullet = w.spawn();
    assert!(!w.is_authored(bullet));
    assert!(w.string_id(bullet).is_none());
    assert!(!s.is_authored(&bullet.to_string()));
    let ctx = p.ctx(w.registry());
    let e = s
        .set_override(&ctx, &bullet.to_string(), "Health.cur", &Value::Int(1))
        .unwrap_err();
    assert_eq!(e.code, codes::NOT_AUTHORED);
}

#[test]
fn rooms_stream_by_graph_distance_and_refs_follow() {
    let t = temp("rooms");
    project(t.path());
    let p = open(t.path());
    let mut w = world();
    let mut s = Scene::open(&p, "hollow", w.registry()).unwrap();
    assert!(s.issues().is_empty(), "{:?}", s.issues());
    assert!(s.has_rooms());
    assert_eq!(s.doc.seed, SeedPolicy::Fixed(7));
    assert_eq!(s.room_distance("cavern-03", "cavern-05"), Some(2));

    let report = s.spawn_around(&mut w, None).unwrap();
    assert_eq!(report.rooms, ["cavern-03", "cavern-04"]);
    assert_eq!(s.loaded_rooms(), ["cavern-03", "cavern-04"]);
    assert_eq!(s.current_room(), Some("cavern-03"));
    // 2 + light, 1 + light
    assert_eq!(report.entities, 5);

    let east = w.entity("@cavern-03/door.east").unwrap();
    let west = w.entity("@cavern-04/door.west").unwrap();
    assert_eq!(w.get::<Door>(east).unwrap().leads_to, west);
    assert_eq!(w.get::<Door>(west).unwrap().leads_to, east);
    assert!(w.get::<Door>(east).unwrap().locked);
    let key = w.get_field(east, "Door.key").unwrap();
    assert_eq!(key, Value::Ref("item:key.brass".into()));

    // Children carry Parent and their own overrides.
    let light = w.entity("@cavern-04/door.west/light").unwrap();
    assert_eq!(w.get::<Parent>(light).unwrap().entity, west);
    assert_eq!(w.get::<Light>(light).unwrap().intensity, 3.0);
    let elight = w.entity("@cavern-03/door.east/light").unwrap();
    assert_eq!(w.get::<Light>(elight).unwrap().intensity, 1.0);

    // Walk to cavern-05: cavern-03 leaves, cavern-05 arrives, refs into
    // cavern-03 go null and are remembered.
    w.drain_events();
    let r = s.transition(&mut w, "cavern-05").unwrap();
    assert_eq!(r.rooms, ["cavern-05"]);
    assert_eq!(s.loaded_rooms(), ["cavern-04", "cavern-05"]);
    assert!(!w.is_alive(east));
    assert_eq!(w.get::<Door>(west).unwrap().leads_to, Entity::NULL);
    let bat = w.entity("@cavern-05/bat").unwrap();
    assert_eq!(w.get::<Health>(bat).unwrap(), Health { cur: 1, max: 3 });
    assert!(w.has_type::<Enemy>(bat));
    let names: Vec<String> = game_events(&w).into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, ["room.unloaded", "room.loaded", "room.transition"]);
    let (_, f) = game_events(&w).pop().unwrap();
    assert_eq!(f[0], ("from".to_string(), Value::Str("cavern-03".into())));
    assert_eq!(f[1], ("to".to_string(), Value::Str("cavern-05".into())));

    // No dangling refs were left for maintenance to find.
    assert_eq!(w.end_frame(), 0);

    // Back to cavern-04: cavern-03 returns and the ref is filled again.
    let r = s.transition(&mut w, "cavern-04").unwrap();
    assert_eq!(r.rooms, ["cavern-03"]);
    let east2 = w.entity("@cavern-03/door.east").unwrap();
    assert_ne!(east2, east, "a re-streamed room gets fresh handles");
    assert_eq!(w.get::<Door>(west).unwrap().leads_to, east2);
    assert_eq!(w.get::<Door>(east2).unwrap().leads_to, west);
    // cavern-03 (3) + cavern-04 (2) + cavern-05 (1): radius 1 around cavern-04.
    assert_eq!(w.authored_count(), 6);
}

#[test]
fn unloaded_room_targets_are_pending_not_errors() {
    let t = temp("pending");
    project(t.path());
    let p = open(t.path());
    let mut w = world();
    let mut s = Scene::open(&p, "hollow", w.registry()).unwrap();
    let r = s.load_rooms(&mut w, &["cavern-04".to_string()]).unwrap();
    assert_eq!(
        r.pending,
        [(
            "cavern-04/door.west".to_string(),
            "Door.leads_to".to_string(),
            "cavern-03/door.east".to_string()
        )]
    );
    let west = w.entity("@cavern-04/door.west").unwrap();
    assert_eq!(w.get::<Door>(west).unwrap().leads_to, Entity::NULL);
    s.load_rooms(&mut w, &["cavern-03".to_string()]).unwrap();
    let east = w.entity("@cavern-03/door.east").unwrap();
    assert_eq!(w.get::<Door>(west).unwrap().leads_to, east);
    assert_eq!(s.unload_room(&mut w, "cavern-04").unwrap(), 2);
    assert_eq!(w.get::<Door>(east).unwrap().leads_to, Entity::NULL);
}

#[test]
fn graph_and_reference_problems_are_issues_with_locations() {
    let t = temp("issues");
    project(t.path());
    // room.toml claims a neighbour the graph does not have; graph names a ghost room.
    write(
        t.path(),
        "scenes/hollow/rooms/cavern-05/room.toml",
        "neighbors = [\"cavern-03\"]\n",
    );
    write(
        t.path(),
        "scenes/hollow/scene.toml",
        "seed = 7\n\n[rooms]\nstart = \"cavern-03\"\nradius = 1\n\n[rooms.graph]\n\"cavern-03\" = [\"cavern-04\", \"cavern-99\"]\n\"cavern-04\" = [\"cavern-05\"]\n",
    );
    write(
        t.path(),
        "scenes/hollow/rooms/cavern-05/lost.toml",
        "[Door]\nleads_to = \"@nowhere\"\nkey = \"item:nope\"\n",
    );
    let p = open(t.path());
    let mut w = world();
    let mut s = Scene::open(&p, "hollow", w.registry()).unwrap();
    let codes_seen: Vec<u32> = s.issues().iter().map(|e| e.code).collect();
    assert!(
        codes_seen.contains(&codes::UNKNOWN_ROOM),
        "{:?}",
        s.issues()
    );
    let mismatch = s
        .issues()
        .iter()
        .find(|e| e.code == codes::BAD_SCENE)
        .unwrap();
    assert!(
        mismatch
            .message
            .contains("cavern-05/room.toml lists neighbor \"cavern-03\""),
        "{mismatch}"
    );
    let refs: Vec<&argus_ecs::ArgusError> = s
        .issues()
        .iter()
        .filter(|e| e.code == codes::MISSING_REFERENCE)
        .collect();
    assert_eq!(refs.len(), 2, "{refs:?}");
    assert!(refs.iter().any(|e| e.message.contains("\"item:nope\"")
        && e.r#ref.as_deref() == Some("scenes/hollow/rooms/cavern-05/lost.toml:3")));
    assert!(refs.iter().any(|e| e.message.contains("\"@nowhere\"")
        && e.r#ref.as_deref() == Some("scenes/hollow/rooms/cavern-05/lost.toml:2")));
    let e = s.spawn_around(&mut w, None).unwrap_err();
    assert_eq!(e.code, codes::SCENE_NOT_CLEAN);

    // A missing scene names the ones that exist.
    let e = Scene::open(&p, "nope", w.registry()).unwrap_err();
    assert_eq!(e.code, codes::MISSING_FILE);
    assert!(e.hint.contains("arena, hollow"), "{}", e.hint);
    // A bad clock spec is refused at the line.
    write(
        t.path(),
        "scenes/arena/scene.toml",
        "clock = \"sometimes\"\n",
    );
    let e = Scene::open(&p, "arena", w.registry()).unwrap_err();
    assert_eq!(e.r#ref.as_deref(), Some("scenes/arena/scene.toml:1"));
}

#[test]
fn loading_is_deterministic() {
    let t = temp("determ");
    project(t.path());
    let p = open(t.path());
    let load = || {
        let mut w = world();
        let mut s = Scene::open(&p, "hollow", w.registry()).unwrap();
        s.spawn_all(&mut w).unwrap();
        w
    };
    let a = load();
    let b = load();
    let ids_a: Vec<(String, Entity)> = a
        .named_entities()
        .into_iter()
        .map(|(s, e)| (s.to_string(), e))
        .collect();
    let ids_b: Vec<(String, Entity)> = b
        .named_entities()
        .into_iter()
        .map(|(s, e)| (s.to_string(), e))
        .collect();
    assert_eq!(ids_a, ids_b);
    let (sa, sb) = (a.summary(), b.summary());
    assert_eq!(
        (sa.entities, sa.authored, sa.components),
        (sb.entities, sb.authored, sb.components)
    );
    let shape = |s: &argus_ecs::WorldSummary| -> Vec<(Vec<String>, usize)> {
        s.archetypes
            .iter()
            .map(|a| (a.components.clone(), a.entities))
            .collect()
    };
    assert_eq!(shape(&sa), shape(&sb));
    for (id, e) in ids_a {
        for c in a.components_of(e).unwrap().to_vec() {
            assert_eq!(
                a.get_component(e, c).unwrap(),
                b.get_component(e, c).unwrap(),
                "{id}"
            );
        }
    }
    // And a snapshot of one restores bit-for-bit into the other.
    let snap = a.snapshot();
    let restored = snap.into_world();
    assert_eq!(shape(&restored.summary()), shape(&sb));
    assert_eq!(restored.named_entities(), b.named_entities());
}
