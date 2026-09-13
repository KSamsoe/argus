//! §3 "Transform hierarchy is first-class": Parent gives a local transform,
//! propagated in a dedicated stage; entities without Parent pay nothing.

use argus_ecs::{
    hierarchy, propagate, Entity, Event, GlobalTransform, Parent, Transform, Vec2, World,
};

fn close(a: Vec2, b: Vec2) -> bool {
    (a.x - b.x).abs() < 1e-4 && (a.y - b.y).abs() < 1e-4
}

fn spawn_tf(w: &mut World, id: &str, t: Transform, parent: Option<Entity>) -> Entity {
    let e = w.spawn_authored(id).unwrap();
    w.insert(e, t).unwrap();
    w.insert(e, GlobalTransform::default()).unwrap();
    if let Some(p) = parent {
        w.insert(e, Parent { entity: p }).unwrap();
    }
    e
}

#[test]
fn roots_copy_and_children_compose() {
    let mut w = World::new();
    hierarchy::register(&mut w).unwrap();
    let body = spawn_tf(&mut w, "body", Transform::at(10.0, 5.0), None);
    let arm = spawn_tf(&mut w, "arm", Transform::at(2.0, 0.0), Some(body));
    let hand = spawn_tf(&mut w, "hand", Transform::at(1.0, 1.0), Some(arm));
    propagate(&mut w).unwrap();
    assert_eq!(
        w.get::<GlobalTransform>(body).unwrap().pos,
        Vec2::new(10.0, 5.0)
    );
    assert_eq!(
        w.get::<GlobalTransform>(arm).unwrap().pos,
        Vec2::new(12.0, 5.0)
    );
    assert_eq!(
        w.get::<GlobalTransform>(hand).unwrap().pos,
        Vec2::new(13.0, 6.0)
    );

    // Moving the root moves the whole chain next propagate.
    w.update::<Transform, _>(body, |t| t.pos.x = 0.0).unwrap();
    propagate(&mut w).unwrap();
    assert_eq!(
        w.get::<GlobalTransform>(hand).unwrap().pos,
        Vec2::new(3.0, 6.0)
    );
}

#[test]
fn rotation_and_scale_compose() {
    let mut w = World::new();
    let root = spawn_tf(
        &mut w,
        "root",
        Transform {
            pos: Vec2::new(1.0, 1.0),
            rot: std::f32::consts::FRAC_PI_2,
            scale: Vec2::new(2.0, 2.0),
        },
        None,
    );
    let child = spawn_tf(&mut w, "child", Transform::at(1.0, 0.0), Some(root));
    propagate(&mut w).unwrap();
    let g = w.get::<GlobalTransform>(child).unwrap();
    // (1,0) scaled by 2 -> (2,0), rotated 90deg -> (0,2), translated -> (1,3).
    assert!(close(g.pos, Vec2::new(1.0, 3.0)), "{:?}", g.pos);
    assert!((g.rot - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
    assert_eq!(g.scale, Vec2::new(2.0, 2.0));
}

#[test]
fn children_processed_in_depth_order_regardless_of_spawn_order() {
    let mut w = World::new();
    hierarchy::register(&mut w).unwrap();
    // Spawn the grandchild first, then wire parents afterwards.
    let c = w.spawn_authored("c").unwrap();
    let b = w.spawn_authored("b").unwrap();
    let a = w.spawn_authored("a").unwrap();
    for (e, x) in [(c, 1.0), (b, 10.0), (a, 100.0)] {
        w.insert(e, Transform::at(x, 0.0)).unwrap();
        w.insert(e, GlobalTransform::default()).unwrap();
    }
    w.insert(c, Parent { entity: b }).unwrap();
    w.insert(b, Parent { entity: a }).unwrap();
    propagate(&mut w).unwrap();
    assert_eq!(w.get::<GlobalTransform>(c).unwrap().pos.x, 111.0);
    assert_eq!(w.get::<GlobalTransform>(b).unwrap().pos.x, 110.0);
}

#[test]
fn cycles_are_reported_not_hung() {
    let mut w = World::new();
    let a = spawn_tf(&mut w, "a", Transform::at(1.0, 0.0), None);
    let b = spawn_tf(&mut w, "b", Transform::at(1.0, 0.0), Some(a));
    w.insert(a, Parent { entity: b }).unwrap();
    propagate(&mut w).unwrap();
    let cycles = w
        .events()
        .iter()
        .filter(|r| matches!(r.event, Event::HierarchyCycle { .. }))
        .count();
    assert_eq!(cycles, 1);
    // Both still got a finite transform.
    assert!(w.get::<GlobalTransform>(a).unwrap().pos.x.is_finite());
    assert!(w.get::<GlobalTransform>(b).unwrap().pos.x.is_finite());
}

#[test]
fn dead_parent_is_nulled_at_frame_end_and_child_becomes_root() {
    let mut w = World::new();
    let p = spawn_tf(&mut w, "p", Transform::at(5.0, 5.0), None);
    let c = spawn_tf(&mut w, "c", Transform::at(1.0, 0.0), Some(p));
    propagate(&mut w).unwrap();
    assert_eq!(
        w.get::<GlobalTransform>(c).unwrap().pos,
        Vec2::new(6.0, 5.0)
    );

    w.despawn(p).unwrap();
    // Before maintenance the ref is stale; propagate treats a dead parent as absent.
    propagate(&mut w).unwrap();
    assert_eq!(
        w.get::<GlobalTransform>(c).unwrap().pos,
        Vec2::new(1.0, 0.0)
    );
    let nulled = w.end_frame();
    assert_eq!(nulled, 1);
    assert_eq!(w.get::<Parent>(c).unwrap().entity, Entity::NULL);
    let parent_id = w.component_id_of::<Parent>().unwrap();
    assert!(w.events().iter().any(|r| r.event
        == Event::DanglingRefNulled {
            entity: c,
            component: parent_id,
            field: "entity".into(),
            target: p
        }));
}

#[test]
fn entities_without_parent_are_only_copied_and_others_untouched() {
    let mut w = World::new();
    hierarchy::register(&mut w).unwrap();
    let bullet = w.spawn();
    w.insert(bullet, Transform::at(3.0, 4.0)).unwrap(); // no GlobalTransform: not visited
    let root = spawn_tf(&mut w, "root", Transform::at(7.0, 8.0), None);
    propagate(&mut w).unwrap();
    assert!(!w.has_type::<GlobalTransform>(bullet));
    assert_eq!(w.get::<Transform>(bullet).unwrap().pos, Vec2::new(3.0, 4.0));
    assert_eq!(
        w.get::<GlobalTransform>(root).unwrap().pos,
        Vec2::new(7.0, 8.0)
    );
}

#[test]
fn parent_without_transform_is_treated_as_origin() {
    let mut w = World::new();
    hierarchy::register(&mut w).unwrap();
    let group = w.spawn_authored("group").unwrap();
    let c = spawn_tf(&mut w, "c", Transform::at(2.0, 2.0), Some(group));
    propagate(&mut w).unwrap();
    assert_eq!(
        w.get::<GlobalTransform>(c).unwrap().pos,
        Vec2::new(2.0, 2.0)
    );
}

#[test]
fn transform_schemas_are_documented() {
    let mut w = World::new();
    hierarchy::register(&mut w).unwrap();
    let t = w.describe("Transform").unwrap();
    assert!(!t.doc.is_empty());
    assert_eq!(
        t.field("scale").unwrap().default,
        argus_ecs::Value::Vec2(Vec2::ONE)
    );
    let p = w.describe("Parent").unwrap();
    assert!(p.has_entity_refs());
    assert!(w
        .describe("GlobalTransform")
        .unwrap()
        .doc
        .contains("Do not write"));
}
