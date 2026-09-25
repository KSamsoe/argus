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
//! It runs first in the sim stage, on the layout of the tick before, so
//! what it emits (`ui.pressed {entity, event}`, the button's own event,
//! `ui.focused`, `ui.cancel`) is seen by the game's systems in the same
//! tick.

use crate::components::{UiButton, UiRect, UiRoot};
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
        .source("argus-ui:input")
        .doc("Moves focus between the buttons of each interactive UI tree with its player's nav action (nearest in the direction, or the neighbour a button names), and presses: confirm the focused button, click the one under the pointer (ui.pressed {entity, event} and the button's event); cancel emits ui.cancel. The pointer focuses what it moves over.");
    for c in [ids.slot, ids.actions].into_iter().flatten() {
        desc = desc.reads(c);
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
        let acts = actions(world, players.get(&slot).copied(), &r);
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
            if p != r.pointer_at {
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
        if acts.click {
            if let Some(u) = under {
                r.focus = u;
                press(u, "pointer", &mut pressed);
            }
        } else if acts.confirm && !r.focus.is_null() {
            press(r.focus, "confirm", &mut pressed);
        }
        if acts.cancel {
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
    for (name, fields) in events {
        ctx.emit(name, fields);
    }
    Ok(())
}
