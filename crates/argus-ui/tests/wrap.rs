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

/// Issue 14: words with line breaks whose lines wrap too ran past the
/// bottom of their panel. A panel of a fixed width holds the text; it must
/// be as tall as every line the words are drawn in.
#[test]
fn text_with_breaks_and_wrapping_lines_is_laid_out_as_tall_as_it_is_drawn() {
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
    let column = w.strings().intern("column");
    let plate = w.spawn();
    w.insert(
        plate,
        UiNode {
            size: Vec2::new(58.0, 0.0),
            pad: Vec2::new(2.0, 1.0),
            direction: column,
            align_self: start,
            ..UiNode::default()
        },
    )
    .unwrap();
    w.insert(plate, argus_ecs::Parent { entity: top }).unwrap();
    let text = w.spawn();
    w.insert(text, UiNode::default()).unwrap();
    w.insert(text, argus_ecs::Parent { entity: plate }).unwrap();
    // 54 pixels inside the padding, nine characters of 6: each paragraph
    // is two lines ("aaaa bbbb" and "cccc"), four lines of 10 in all.
    let words = w.strings().intern("aaaa bbbb cccc\ndddd eeee ffff");
    w.insert(
        text,
        UiText {
            text: words,
            size: 8.0,
            ..UiText::default()
        },
    )
    .unwrap();
    s.run_load(&mut w).unwrap();
    let r = w.get::<UiRect>(text).unwrap();
    let p = w.get::<UiRect>(plate).unwrap();
    assert_eq!(r.size.y, 40.0, "four lines: {:?} in {:?}", r.size, p.size);
    assert_eq!(p.size.y, 42.0, "the plate holds them: {:?}", p.size);
}

/// Issue 14, as it turned out: the words were measured right, and their
/// plate shrank. A node's `min` takes the place of its content as the
/// smallest it may be, as in CSS, so in a column short of room a plate with
/// a `min` shrinks past its words down to that `min`. `shrink = 0` keeps
/// it at least as big as what it holds; without a `min` it is anyway.
#[test]
fn a_plate_with_a_min_shrinks_past_its_words_unless_it_does_not_shrink() {
    let plate_and_text = |min: f32, shrink: f32| {
        let mut w = World::new();
        let mut s = Scheduler::new(SchedulerConfig::default());
        argus_ecs::hierarchy::register(&mut w).unwrap();
        argus_camera::register(&mut w).unwrap();
        argus_ui::install(&mut w, &mut s, Rc::new(RefCell::new(Shared::default()))).unwrap();
        let column = w.strings().intern("column");
        // A column 100 high holding the plate and a row 40 high.
        let top = w.spawn();
        let size = Vec2::new(200.0, 100.0);
        w.insert(
            top,
            UiNode {
                direction: column,
                size,
                ..UiNode::default()
            },
        )
        .unwrap();
        let plate = w.spawn();
        let node = UiNode {
            size: Vec2::new(58.0, 0.0),
            min: Vec2::new(0.0, min),
            pad: Vec2::new(2.0, 1.0),
            direction: column,
            shrink,
            ..UiNode::default()
        };
        w.insert(plate, node).unwrap();
        w.insert(plate, argus_ecs::Parent { entity: top }).unwrap();
        let text = w.spawn();
        w.insert(text, UiNode::default()).unwrap();
        w.insert(text, argus_ecs::Parent { entity: plate }).unwrap();
        // Seven paragraphs of two lines each: 140 high.
        let words = w.strings().intern(
            "aaaa bbbb cccc\ndddd eeee ffff\ngggg hhhh iiii\njjjj kkkk llll\nmmmm nnnn oooo\npppp qqqq rrrr\nssss tttt uuuu",
        );
        w.insert(
            text,
            UiText {
                text: words,
                size: 8.0,
                ..UiText::default()
            },
        )
        .unwrap();
        let row = w.spawn();
        w.insert(
            row,
            UiNode {
                size: Vec2::new(10.0, 40.0),
                ..UiNode::default()
            },
        )
        .unwrap();
        w.insert(row, argus_ecs::Parent { entity: top }).unwrap();
        s.run_load(&mut w).unwrap();
        let h = |e| w.get::<UiRect>(e).unwrap().size.y;
        (h(plate), h(text))
    };
    assert_eq!(
        plate_and_text(0.0, 1.0),
        (142.0, 140.0),
        "no min: the words keep their plate"
    );
    let (plate, text) = plate_and_text(30.0, 1.0);
    assert_eq!(text, 140.0, "the words are measured right");
    assert!(
        plate < text,
        "a min lets the plate shrink past them: {plate}"
    );
    assert_eq!(
        plate_and_text(30.0, 0.0),
        (142.0, 140.0),
        "shrink = 0 keeps them"
    );
}
