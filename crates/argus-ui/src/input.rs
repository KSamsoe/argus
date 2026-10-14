//! `ui.input` (sim stage, first): focus and presses from a player's actions.
//!
//! Each interactive [`UiRoot`] reads the actions of its player slot: the nav
//! axis moves focus to the nearest button in that direction (or the one a
//! button names), once when pressed and again every 0.12 s after 0.4 s held;
//! confirm presses the focused button; cancel emits `ui.cancel`. A pointer
//! (an action whose value is a place on the canvas) focuses the button under
//! it when it moves, and the click action presses that button. Gameplay
//! never sees keys (§12 "Input actions"): a menu works from a keyboard, a
//! gamepad, or a test's `input.inject`, and a replay presses the same.
//!
//! A text box (`UiTextBox`, issue 23) is a button that, pressed, types:
//! it sets its slot's `Actions.typing`, so the keyboard types into
//! `Actions.typed` instead of moving actions, and takes that text in;
//! Enter sends it (`ui.submitted` and its event), Escape, the cancel
//! action, or a press elsewhere stops it. The tree's scroll action (the
//! wheel) scrolls the `UiScroll` node under the pointer.
//!
//! It runs first in the sim stage, on the layout of the tick before, so
//! what it emits (`ui.pressed {entity, event}`, the button's own event,
//! `ui.focused`, `ui.cancel`) is seen by the game's systems in the same
//! tick.

use crate::components::{UiButton, UiRect, UiRoot, UiScroll, UiText, UiTextBox};
use crate::layout::Ids;
use crate::Shared;
use argus_ecs::{ComponentId, Entity, Parent, Result, StrId, Value, Vec2, World};
use argus_sim::{Scheduler, Stage, SystemCtx, SystemDesc};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// The input system's name.
pub const INPUT: &str = "ui.input";

/// Seconds a direction is held before focus moves again, and then between.
const REPEAT_DELAY: f32 = 0.4;
const REPEAT_EVERY: f32 = 0.12;

struct InIds {
    ids: Ids,
    button: ComponentId,
    textbox: ComponentId,
    slot: Option<ComponentId>,
    actions: Option<ComponentId>,
}

pub(crate) fn install(
    world: &mut World,
    sched: &mut Scheduler,
    _shared: Rc<RefCell<Shared>>,
) -> Result<()> {
    let ids = InIds {
        ids: Ids::of(world),
        button: world.component_id_of::<UiButton>().expect("registered"),
        textbox: world.component_id_of::<UiTextBox>().expect("registered"),
        slot: world.component_id("PlayerSlot"),
        actions: world.component_id("Actions"),
    };
    // Unfrozen: a menu works while the game stands still (a pause).
    let mut desc = SystemDesc::new(INPUT, Stage::Sim)
        .first()
        .unfrozen()
        .reads(ids.ids.node)
        .reads(ids.ids.rect)
        .reads(ids.ids.parent)
        .writes(ids.ids.root)
        .writes(ids.button)
        .writes(ids.textbox)
        .writes(ids.ids.text)
        .writes(ids.ids.scroll)
        .source("argus-ui:input")
        .doc("Moves focus between the buttons of each interactive UI tree with its player's nav action (nearest in the direction, or the neighbour a button names), and presses: confirm the focused button, click the one under the pointer (ui.pressed {entity, event} and the button's event); cancel emits ui.cancel. The pointer focuses what it moves over.");
    if let Some(c) = ids.slot {
        desc = desc.reads(c);
    }
    // Actions.typing: a text box makes its slot's keyboard type.
    if let Some(c) = ids.actions {
        desc = desc.writes(c);
    }
    sched.add_system(desc, Box::new(move |ctx| run(ctx, &ids)))
}

/// A button of some tree, as this tick sees it.
#[derive(Clone)]
struct Btn {
    e: Entity,
    b: UiButton,
    r: UiRect,
    root: Entity,
}

impl Btn {
    fn usable(&self) -> bool {
        self.r.shown
            && !self.b.disabled
            && self.r.alpha > 0.0
            && self.r.pos.x + self.r.size.x > self.r.clip_min.x
            && self.r.pos.y + self.r.size.y > self.r.clip_min.y
            && self.r.pos.x < self.r.clip_max.x
            && self.r.pos.y < self.r.clip_max.y
    }
    fn center(&self) -> Vec2 {
        Vec2::new(
            self.r.pos.x + self.r.size.x * 0.5,
            self.r.pos.y + self.r.size.y * 0.5,
        )
    }
    fn contains(&self, p: Vec2) -> bool {
        let lo = Vec2::new(
            self.r.pos.x.max(self.r.clip_min.x),
            self.r.pos.y.max(self.r.clip_min.y),
        );
        let hi = Vec2::new(
            (self.r.pos.x + self.r.size.x).min(self.r.clip_max.x),
            (self.r.pos.y + self.r.size.y).min(self.r.clip_max.y),
        );
        p.x >= lo.x && p.y >= lo.y && p.x < hi.x && p.y < hi.y
    }
}

/// The top of `e`'s tree: up through parents that are UI nodes.
fn top_of(world: &World, node: ComponentId, e: Entity) -> Entity {
    let mut cur = e;
    for _ in 0..1000 {
        match world.get::<Parent>(cur) {
            Ok(p) if !p.entity.is_null() && world.has(p.entity, node) => cur = p.entity,
            _ => break,
        }
    }
    cur
}

fn name_or(world: &World, id: StrId, default: &str) -> String {
    let s = world.strings().resolve(id);
    if s.is_empty() {
        default.to_string()
    } else {
        s.to_string()
    }
}

/// The actions a tree reads, this tick.
#[derive(Default)]
struct Acts {
    nav: Vec2,
    confirm: bool,
    cancel: bool,
    click: bool,
    pointer: Option<Vec2>,
    /// The scroll action: the wheel, up positive.
    scroll: f32,
    /// What the slot typed (`Actions.typed`).
    typed: String,
}

fn actions(world: &World, player: Option<Entity>, root: &UiRoot) -> Acts {
    let Some(p) = player else {
        return Acts::default();
    };
    let get = |name: &str| world.get_field(p, &format!("Actions.{name}")).ok();
    let nav = name_or(world, root.nav, "ui_move");
    let confirm = name_or(world, root.confirm, "ui_confirm");
    let cancel = name_or(world, root.cancel, "ui_cancel");
    let click = name_or(world, root.click, "ui_click");
    let pointer = name_or(world, root.pointer, "pointer");
    let scroll = name_or(world, root.scroll, "ui_scroll");
    let pressed = |n: &str| matches!(get(&format!("{n}_pressed")), Some(Value::Bool(true)));
    Acts {
        nav: match get(&nav) {
            Some(Value::Vec2(v)) => v,
            _ => Vec2::ZERO,
        },
        confirm: pressed(&confirm),
        cancel: pressed(&cancel),
        click: pressed(&click),
        pointer: match get(&pointer) {
            Some(Value::Vec2(v)) => Some(v),
            _ => None,
        },
        scroll: match get(&scroll) {
            Some(Value::Float(v)) => v as f32,
            _ => 0.0,
        },
        typed: match get("typed") {
            Some(Value::Str(s)) => s,
            _ => String::new(),
        },
    }
}

/// A text box as this tick changes it.
struct TextBox {
    e: Entity,
    b: UiTextBox,
    text: String,
}

/// The characters typed, into `t`: Backspace takes the last one off,
/// Enter sends it, Escape stops typing.
fn type_into(
    t: &mut TextBox,
    typed: &str,
    events: &mut Vec<(String, Vec<(String, Value)>)>,
    world: &World,
) {
    for c in typed.chars() {
        if !t.b.typing {
            break;
        }
        match c {
            '\u{8}' => {
                t.text.pop();
            }
            '\r' | '\n' => submit(t, events, world),
            '\u{1b}' => t.b.typing = false,
            c if c.is_control() => {}
            c => {
                if t.text.chars().count() < t.b.max.max(1) as usize {
                    t.text.push(c);
                }
            }
        }
    }
}

/// Send a text box's text: `ui.submitted` and its own event.
fn submit(t: &mut TextBox, events: &mut Vec<(String, Vec<(String, Value)>)>, world: &World) {
    let fields = vec![
        ("entity".to_string(), Value::Entity(t.e)),
        ("text".to_string(), Value::Str(t.text.clone())),
    ];
    events.push(("ui.submitted".to_string(), fields.clone()));
    let ev = world.strings().resolve(t.b.event).to_string();
    if !ev.is_empty() {
        events.push((ev, fields));
    }
    if t.b.clear {
        t.text.clear();
    }
    if !t.b.keep {
        t.b.typing = false;
    }
}

/// The nearest usable button from `from` in direction `d` (canvas y down).
fn nearest(btns: &[&Btn], from: &Btn, d: Vec2) -> Option<Entity> {
    let c = from.center();
    let mut best: Option<(f32, u32, Entity)> = None;
    for b in btns {
        if b.e == from.e {
            continue;
        }
        let o = b.center();
        let (dx, dy) = (o.x - c.x, o.y - c.y);
        let (along, across) = if d.x > 0.0 {
            (dx, dy.abs())
        } else if d.x < 0.0 {
            (-dx, dy.abs())
        } else if d.y > 0.0 {
            (dy, dx.abs())
        } else {
            (-dy, dx.abs())
        };
        if along <= 0.5 {
            continue;
        }
        let score = along + across * 2.0;
        if best.is_none_or(|(s, dep, _)| score < s || (score == s && b.r.depth < dep)) {
            best = Some((score, b.r.depth, b.e));
        }
    }
    best.map(|(_, _, e)| e)
}

fn run(ctx: &mut SystemCtx<'_>, ids: &InIds) -> Result<()> {
    let world = ctx.world();
    let dt = ctx.dt as f32;
    let mut btns: Vec<Btn> = Vec::new();
    world
        .query()
        .has(ids.button)
        .has(ids.ids.rect)
        .for_each_view(|v| {
            let bs = v.column::<UiButton>()?;
            let rs = v.column::<UiRect>()?;
            for (i, e) in v.entities().iter().enumerate() {
                btns.push(Btn {
                    e: *e,
                    b: bs[i],
                    r: rs[i],
                    root: Entity::NULL,
                });
            }
            Ok(())
        })?;
    for b in &mut btns {
        b.root = top_of(world, ids.ids.node, b.e);
    }
    btns.sort_by_key(|b| (b.r.depth, b.e));

    // Text boxes, and nodes that scroll, with the tree each is in.
    let mut boxes: Vec<TextBox> = Vec::new();
    world.query().has(ids.textbox).for_each_view(|v| {
        let bs = v.column::<UiTextBox>()?;
        for (i, e) in v.entities().iter().enumerate() {
            boxes.push(TextBox {
                e: *e,
                b: bs[i],
                text: world.strings().resolve(bs[i].text).to_string(),
            });
        }
        Ok(())
    })?;
    boxes.sort_by_key(|t| t.e);
    let boxes_before: Vec<UiTextBox> = boxes.iter().map(|t| t.b).collect();
    let texts_before: Vec<String> = boxes.iter().map(|t| t.text.clone()).collect();
    let mut scrolls: Vec<(Entity, UiScroll, UiRect, Entity)> = Vec::new();
    world
        .query()
        .has(ids.ids.scroll)
        .has(ids.ids.rect)
        .for_each_view(|v| {
            let ss = v.column::<UiScroll>()?;
            let rs = v.column::<UiRect>()?;
            for (i, e) in v.entities().iter().enumerate() {
                scrolls.push((*e, ss[i], rs[i], Entity::NULL));
            }
            Ok(())
        })?;
    for s in &mut scrolls {
        s.3 = top_of(world, ids.ids.node, s.0);
    }
    let mut scroll_writes: Vec<(Entity, f32)> = Vec::new();
    let mut typing_writes: Vec<(Entity, bool)> = Vec::new();

    // The players' action entities by slot.
    let mut players: HashMap<u8, Entity> = HashMap::new();
    if let (Some(slot), Some(acts)) = (ids.slot, ids.actions) {
        let mut ents = world.query().has(slot).has(acts).entities();
        ents.sort();
        for e in ents {
            if let Ok(Value::UInt(s)) = world.get_field(e, "PlayerSlot.slot") {
                players.entry(s as u8).or_insert(e);
            }
        }
    }

    let mut roots = world.query().has(ids.ids.root).entities();
    roots.sort();
    let mut root_writes: Vec<(Entity, UiRoot)> = Vec::new();
    let mut focus_of: HashMap<Entity, Entity> = HashMap::new();
    let mut hovered: Option<Entity> = None;
    let mut pressed: Vec<Entity> = Vec::new();
    let mut events: Vec<(String, Vec<(String, Value)>)> = Vec::new();
    for r_e in roots {
        let Ok(mut r) = world.get::<UiRoot>(r_e) else {
            continue;
        };
        let before = r;
        let mine: Vec<&Btn> = btns
            .iter()
            .filter(|b| b.root == r_e && b.usable())
            .collect();
        if !r.interactive {
            continue;
        }
        let slot = if r.slot == 0 { 1 } else { r.slot };
        let player = players.get(&slot).copied();
        let acts = actions(world, player, &r);
        let in_tree = |e: Entity| btns.iter().any(|b| b.e == e && b.root == r_e);
        let was_typing = boxes.iter().any(|t| t.b.typing && in_tree(t.e));
        let find = |e: Entity| mine.iter().find(|b| b.e == e).copied();
        // Focus on something that can take it.
        if find(r.focus).is_none() {
            r.focus = mine.first().map_or(Entity::NULL, |b| b.e);
        }
        // Nav: once on press, then repeating while held.
        let dir = Vec2::new(
            if acts.nav.x > 0.5 {
                1.0
            } else if acts.nav.x < -0.5 {
                -1.0
            } else {
                0.0
            },
            // Up on the stick is up the canvas, whose y grows down.
            if acts.nav.y > 0.5 {
                -1.0
            } else if acts.nav.y < -0.5 {
                1.0
            } else {
                0.0
            },
        );
        let moving = dir != Vec2::ZERO;
        let step = if dir != r.held {
            r.wait = REPEAT_DELAY;
            moving
        } else if moving {
            r.wait -= dt;
            if r.wait <= 0.0 {
                r.wait = REPEAT_EVERY;
                true
            } else {
                false
            }
        } else {
            false
        };
        r.held = dir;
        if step {
            if let Some(f) = find(r.focus) {
                // One axis at a time: the stronger one.
                let d = if dir.x != 0.0 && dir.y != 0.0 {
                    if acts.nav.x.abs() >= acts.nav.y.abs() {
                        Vec2::new(dir.x, 0.0)
                    } else {
                        Vec2::new(0.0, dir.y)
                    }
                } else {
                    dir
                };
                let named = if d.x > 0.0 {
                    f.b.right
                } else if d.x < 0.0 {
                    f.b.left
                } else if d.y > 0.0 {
                    f.b.down
                } else {
                    f.b.up
                };
                let to = find(named).map(|b| b.e).or_else(|| nearest(&mine, f, d));
                if let Some(to) = to {
                    r.focus = to;
                }
            }
        }
        // The pointer: what is under it, focused when it moves.
        let mut under = None;
        if let Some(p) = acts.pointer {
            under = mine.iter().rev().find(|b| b.contains(p)).map(|b| b.e);
            // While a box types, the pointer passing over a button does not
            // take the focus (and the typing) away; a click does.
            if p != r.pointer_at && !was_typing {
                if let Some(u) = under {
                    r.focus = u;
                }
                r.pointer_at = p;
            }
            if under.is_some() {
                hovered = under;
            }
        }
        // Presses.
        let mut press = |e: Entity, how: &str, pressed: &mut Vec<Entity>| {
            if let Some(b) = find(e) {
                pressed.push(e);
                let ev = world.strings().resolve(b.b.event).to_string();
                events.push((
                    "ui.pressed".to_string(),
                    vec![
                        ("entity".to_string(), Value::Entity(e)),
                        ("event".to_string(), Value::Str(ev.clone())),
                        ("by".to_string(), Value::Str(how.to_string())),
                    ],
                ));
                if !ev.is_empty() {
                    events.push((ev, vec![("entity".to_string(), Value::Entity(e))]));
                }
            }
        };
        if let Some(p) = acts.pointer {
            r.pointer_at = p;
        }
        let mut pressed_here: Option<(Entity, bool)> = None;
        if acts.click {
            if let Some(u) = under {
                r.focus = u;
                press(u, "pointer", &mut pressed);
                pressed_here = Some((u, true));
            }
        } else if acts.confirm && !r.focus.is_null() {
            press(r.focus, "confirm", &mut pressed);
            pressed_here = Some((r.focus, false));
        }
        // Text boxes (issue 23).
        let mut cancel_eaten = false;
        match pressed_here {
            Some((p, by_click)) => {
                let typing_p = boxes.iter().any(|t| t.e == p && t.b.typing);
                for t in boxes.iter_mut().filter(|t| in_tree(t.e)) {
                    if t.e == p {
                        if typing_p && !by_click {
                            // Confirm on a box that types (a gamepad's): send.
                            submit(t, &mut events, world);
                        } else {
                            t.b.typing = true;
                        }
                    } else {
                        t.b.typing = false;
                    }
                }
            }
            None if acts.click && under.is_none() => {
                for t in boxes.iter_mut().filter(|t| in_tree(t.e)) {
                    t.b.typing = false;
                }
            }
            None => {}
        }
        if let Some(t) = boxes.iter_mut().find(|t| t.b.typing && in_tree(t.e)) {
            if r.focus != t.e && r.focus != before.focus && pressed_here.is_none() {
                // The focus moved off it this tick (a gamepad's nav): it
                // stops. One a script set typing takes the focus instead.
                t.b.typing = false;
            } else if acts.cancel {
                t.b.typing = false;
                cancel_eaten = true;
            } else {
                r.focus = t.e;
                type_into(t, &acts.typed, &mut events, world);
            }
        }
        // A box that types keeps its slot typing (a script may have set it
        // going); one that stopped lets the keyboard go. A game's own use of
        // Actions.typing, with no box typing, is left alone.
        let now_typing = boxes.iter().any(|t| t.b.typing && in_tree(t.e));
        if let Some(p) = player {
            let on = world.get_field(p, "Actions.typing").ok() == Some(Value::Bool(true));
            if (now_typing && !on) || (was_typing && !now_typing && on) {
                typing_writes.push((p, now_typing));
            }
        }
        // The wheel scrolls the scrolling node under the pointer.
        if acts.scroll != 0.0 {
            if let Some(p) = acts.pointer {
                let hit = scrolls
                    .iter()
                    .filter(|s| s.3 == r_e && s.2.shown && contains(&s.2, p))
                    .max_by_key(|s| s.2.depth);
                if let Some((e, s, _, _)) = hit {
                    let y = (s.y - acts.scroll * s.step).max(0.0);
                    if y != s.y {
                        scroll_writes.push((*e, y));
                    }
                }
            }
        }
        if acts.cancel && !cancel_eaten {
            events.push((
                "ui.cancel".to_string(),
                vec![("root".to_string(), Value::Entity(r_e))],
            ));
        }
        if r.focus != before.focus && !r.focus.is_null() {
            events.push((
                "ui.focused".to_string(),
                vec![
                    ("entity".to_string(), Value::Entity(r.focus)),
                    ("root".to_string(), Value::Entity(r_e)),
                ],
            ));
        }
        focus_of.insert(r_e, r.focus);
        if r != before {
            root_writes.push((r_e, r));
        }
    }

    // Write back what changed.
    let mut button_writes = Vec::new();
    for b in &btns {
        let focused = focus_of.get(&b.root) == Some(&b.e);
        let want = UiButton {
            focused,
            hovered: hovered == Some(b.e),
            pressed: pressed.contains(&b.e),
            ..b.b
        };
        if want != b.b {
            button_writes.push((b.e, want));
        }
    }
    for (e, r) in root_writes {
        ctx.update::<UiRoot, _>(e, |x| *x = r)?;
    }
    for (e, b) in button_writes {
        ctx.update::<UiButton, _>(e, |x| *x = b)?;
    }
    // The boxes: what changed, and what each shows (the text and a caret
    // while it types; the hint while it is empty).
    for (i, t) in boxes.iter().enumerate() {
        let text_id = if t.text != texts_before[i] {
            world.strings().intern(&t.text)
        } else {
            t.b.text
        };
        let want = UiTextBox {
            text: text_id,
            ..t.b
        };
        let shown = if t.b.typing {
            format!("{}|", t.text)
        } else if t.text.is_empty() {
            world.strings().resolve(t.b.hint).to_string()
        } else {
            t.text.clone()
        };
        let show = match world.get::<UiText>(t.e) {
            Ok(ut) if world.strings().resolve(ut.text) != shown.as_str() => {
                Some(world.strings().intern(&shown))
            }
            _ => None,
        };
        let changed = want != boxes_before[i];
        let e = t.e;
        if changed {
            ctx.update::<UiTextBox, _>(e, |x| *x = want)?;
        }
        if let Some(id) = show {
            ctx.update::<UiText, _>(e, |x| x.text = id)?;
        }
    }
    for (e, on) in typing_writes {
        ctx.set_field(e, "Actions.typing", Value::Bool(on))?;
    }
    for (e, y) in scroll_writes {
        ctx.update::<UiScroll, _>(e, |s| s.y = y)?;
    }
    for (name, fields) in events {
        ctx.emit(name, fields);
    }
    Ok(())
}

/// Whether `p` is in a node's rect where its clip lets it be seen.
fn contains(r: &UiRect, p: Vec2) -> bool {
    let lo = Vec2::new(r.pos.x.max(r.clip_min.x), r.pos.y.max(r.clip_min.y));
    let hi = Vec2::new(
        (r.pos.x + r.size.x).min(r.clip_max.x),
        (r.pos.y + r.size.y).min(r.clip_max.y),
    );
    p.x >= lo.x && p.y >= lo.y && p.x < hi.x && p.y < hi.y
}
