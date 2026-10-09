//! Text wrapped inside a node's `max` width (found at the phase 5 gate):
//! a sign with `max` and wrapped words kept one line's height, and its
//! other lines spilled out below the panel. The layout measures a leaf
//! before it clamps it to `max`, so the words must wrap at `max` already.

use argus_ecs::{StrId, Vec2, World};
use argus_sim::{Scheduler, SchedulerConfig};
use argus_ui::{Shared, UiNode, UiRect, UiText};
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn words_wrap_at_a_nodes_max_width_and_the_node_is_as_tall_as_its_lines() {
    let mut w = World::new();
    let mut s = Scheduler::new(SchedulerConfig::default());
    argus_ecs::hierarchy::register(&mut w).unwrap();
    argus_camera::register(&mut w).unwrap();
    argus_ui::install(&mut w, &mut s, Rc::new(RefCell::new(Shared::default()))).unwrap();
    let top = w.spawn();
    w.insert(
        top,
        UiNode {
            align: StrId::EMPTY,
            ..UiNode::default()
        },
    )
    .unwrap();
    let start = w.strings().intern("start");
    let sign = w.spawn();
    w.insert(
        sign,
        UiNode {
            max: Vec2::new(48.0, 0.0),
            pad: Vec2::new(2.0, 1.0),
            align_self: start,
            ..UiNode::default()
        },
    )
    .unwrap();
    w.insert(sign, argus_ecs::Parent { entity: top }).unwrap();
    let words = w.strings().intern("one two three");
    w.insert(
        sign,
        UiText {
            text: words,
            size: 8.0,
            ..UiText::default()
        },
    )
    .unwrap();
    s.run_load(&mut w).unwrap();
    let r = w.get::<UiRect>(sign).unwrap();
    // 13 characters of 6 pixels do not fit in 48 less 4 of padding: "one
    // two" and "three", two lines of 10, padded 1 above and below.
    assert!(r.size.x <= 48.0, "{:?}", r.size);
    assert_eq!(r.size.y, 22.0, "two lines and the padding: {:?}", r.size);
}
