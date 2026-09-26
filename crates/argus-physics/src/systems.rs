//! The three physics systems and their events.

use crate::env::{Aabb, Asker, Env, Solid, Support, EPS};
use crate::mover::{move_solid, Actor, Params};
use crate::{CharacterController, Collider, KinematicBody};
use argus_ecs::{
    ComponentId, Entity, GlobalTransform, Result, StrId, Transform, Value, Vec2, World,
};
use argus_sim::{Scheduler, Stage, SystemCtx, SystemDesc};
use argus_tiles::{MapStore, TileGrid, Tilemap};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Actions to velocities, first in the sim stage.
pub const CONTROL: &str = "physics.control";
/// Solids, then actors, moved against tiles and each other; last in the sim stage.
pub const MOVE: &str = "physics.move";
/// Contacts between colliders, after the move.
pub const OVERLAP: &str = "physics.overlap";

type Events = Vec<(String, Vec<(String, Value)>)>;

fn ev(out: &mut Events, name: &str, fields: Vec<(&str, Value)>) {
    out.push((
        name.to_string(),
        fields
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
    ));
}

pub(crate) fn install(
    world: &mut World,
    sched: &mut Scheduler,
    tiles: Rc<RefCell<MapStore>>,
    contacts: Rc<RefCell<crate::Contacts>>,
) -> Result<()> {
    let col = world.component_id_of::<Collider>().expect("registered");
    let kb = world
        .component_id_of::<KinematicBody>()
        .expect("registered");
    let cc = world
        .component_id_of::<CharacterController>()
        .expect("registered");
    let tm = world.component_id_of::<Tilemap>().expect("registered");
    let tr = world
        .component_id_of::<Transform>()
        .expect("hierarchy registered");
    let gt = world
        .component_id_of::<GlobalTransform>()
        .expect("hierarchy registered");
    let actions = world.component_id(argus_input_actions());

    let mut desc = SystemDesc::new(CONTROL, Stage::Sim)
        .writes(cc)
        .writes(kb)
        .source("argus-physics:control")
        .doc("Turns each CharacterController's actions into its KinematicBody's velocity and gravity: run, jump with coyote time and buffering, variable jump height, and dropping through one-way platforms.");
    if let Some(a) = actions {
        desc = desc.reads(a);
    }
    let mut paths: HashMap<(StrId, &'static str), String> = HashMap::new();
    sched.add_system(
        desc,
        Box::new(move |ctx| control(ctx, cc, kb, actions, &mut paths)),
    )?;

    let store = tiles.clone();
    sched.add_system(
        SystemDesc::new(MOVE, Stage::Sim)
            .last()
            .reads(col)
            .reads(cc)
            .reads(tm)
            .reads(gt)
            .writes(tr)
            .writes(kb)
            .source("argus-physics:move")
            .doc("Moves every KinematicBody: solids first, carrying riders and pushing what they hit, then actors against tiles and solids, x then y. Writes the contact fields and emits body.landed, body.wall, body.ceiling, body.corner_corrected, and body.crushed."),
        Box::new(move |ctx| step(ctx, &store, col, kb, cc, tm, tr, gt)),
    )?;

    let mut state = OverlapState {
        contacts,
        slots: Vec::new(),
    };
    sched.add_system(
        SystemDesc::new(OVERLAP, Stage::Sim)
            .last()
            .after(MOVE)
            .reads(col)
            .reads(tr)
            .reads(gt)
            .source("argus-physics:overlap")
            .doc("Rebuilds the shared broadphase from where things are now and emits overlap.begin and overlap.end for every pair of colliders whose layers meet."),
        Box::new(move |ctx| overlap(ctx, &mut state, col, gt)),
    )?;
    Ok(())
}

/// The name of the input crate's generated actions component (kept here
/// so this crate need not depend on argus-input).
fn argus_input_actions() -> &'static str {
    "Actions"
}

/// Whether any entity has component `c`, without building a query: the
/// systems return at once, allocating nothing, in a scene with no bodies.
fn any_with(ctx: &SystemCtx<'_>, c: ComponentId) -> bool {
    ctx.world()
        .archetypes()
        .iter()
        .any(|a| !a.is_empty() && a.has(c))
}

fn approach(v: f32, target: f32, step: f32) -> f32 {
    if v < target {
        (v + step).min(target)
    } else {
        (v - step).max(target)
    }
}

// ----- physics.control ----------------------------------------------------------

fn control(
    ctx: &mut SystemCtx<'_>,
    cc: ComponentId,
    kb: ComponentId,
    actions: Option<ComponentId>,
    paths: &mut HashMap<(StrId, &'static str), String>,
) -> Result<()> {
    if !any_with(ctx, cc) {
        return Ok(());
    }
    let dt = ctx.dt as f32;
    let mut events: Events = Vec::new();
    let entities = ctx.query().has(cc).has(kb).entities();
    for e in entities {
        let mut c: CharacterController = ctx.get(e)?;
        let mut k: KinematicBody = ctx.get(e)?;
        let mut tapped = false;
        if actions.is_some_and(|a| ctx.has(e, a)) {
            // Keyed by the default too: both names are empty by default,
            // and an empty move_action must not answer for jump_action.
            let mut path = |id: StrId, default: &'static str, w: &World| {
                paths
                    .entry((id, default))
                    .or_insert_with(|| {
                        let n = w.strings().resolve(id);
                        format!(
                            "Actions.{}",
                            if n.is_empty() { default } else { n.as_str() }
                        )
                    })
                    .clone()
            };
            let (mp, jp) = (
                path(c.move_action, "move", ctx.world()),
                path(c.jump_action, "jump", ctx.world()),
            );
            match ctx.get_field(e, &mp) {
                Ok(Value::Vec2(v)) => {
                    c.move_x = v.x;
                    c.move_y = v.y;
                }
                Ok(Value::Float(f)) => c.move_x = f as f32,
                _ => {}
            }
            if let Ok(Value::Bool(b)) = ctx.get_field(e, &jp) {
                c.jump = b;
            }
            // A press and its release in one tick leave the button up, and
            // the edge still counts: a tap shorter than a tick, or one made
            // during a hit-stop, whose input lands when it ends (phase 2
            // item 6).
            let jpp = paths
                .entry((c.jump_action, "jump_pressed"))
                .or_insert_with(|| format!("{jp}_pressed"))
                .clone();
            tapped = matches!(ctx.get_field(e, &jpp), Ok(Value::Bool(true)));
        }
        // A moving platform's sideways speed kept on a jump off it; added
        // after this tick's run acceleration so the ground's friction does
        // not take it back at once.
        let mut carry = 0.0f32;
        let pressed = (c.jump && !c.jump_was) || tapped;
        c.jump_was = c.jump;
        if k.grounded {
            c.coyote = c.coyote_time;
        } else {
            c.coyote = (c.coyote - dt).max(0.0);
        }
        c.buffer = if pressed {
            c.jump_buffer.max(dt * 0.5)
        } else {
            (c.buffer - dt).max(0.0)
        };

        if pressed && k.grounded && k.on_one_way && c.move_y < -0.5 {
            // Down and jump on a one-way platform: through it, no jump.
            k.drop = 0.1;
            c.buffer = 0.0;
            k.grounded = false;
            ev(
                &mut events,
                "body.dropped",
                vec![("entity", Value::Entity(e))],
            );
        } else if c.buffer > 0.0 && (k.grounded || c.coyote > 0.0) {
            let mut boost = Vec2::ZERO;
            if k.grounded && !k.ground.is_null() && ctx.is_alive(k.ground) {
                if let Ok(p) = ctx.get::<KinematicBody>(k.ground) {
                    boost = Vec2::new(
                        p.velocity.x * c.platform_boost,
                        p.velocity.y.max(0.0) * c.platform_boost,
                    );
                }
            }
            k.velocity.y = c.takeoff() + boost.y;
            carry = boost.x;
            ev(
                &mut events,
                "body.jumped",
                vec![
                    ("entity", Value::Entity(e)),
                    ("coyote", Value::Bool(!k.grounded)),
                    ("buffered", Value::Bool(!pressed)),
                ],
            );
            c.rising = true;
            c.buffer = 0.0;
            c.coyote = 0.0;
        }
        if c.rising && !c.jump && k.velocity.y > 0.0 {
            k.velocity.y *= c.jump_cut;
            c.rising = false;
        }
        if k.velocity.y <= 0.0 {
            c.rising = false;
        }
        let g = c.gravity();
        k.gravity = if k.velocity.y <= 0.0 {
            g * c.fall_gravity
        } else {
            g
        };
        k.max_fall = c.max_fall;

        let target = c.move_x.clamp(-1.0, 1.0) * c.run_speed;
        let (acc, dec) = if k.grounded {
            (c.ground_accel, c.ground_decel)
        } else {
            (c.air_accel, c.air_decel)
        };
        let v = k.velocity.x;
        let speeding_up =
            target != 0.0 && (v == 0.0 || v.signum() == target.signum()) && v.abs() < target.abs();
        let rate = if speeding_up { acc } else { dec };
        k.velocity.x = approach(v, target, rate * dt) + carry;

        ctx.update::<CharacterController, _>(e, |x| *x = c)?;
        ctx.update::<KinematicBody, _>(e, |x| *x = k)?;
    }
    for (n, f) in events {
        ctx.emit(n, f);
    }
    Ok(())
}

// ----- physics.move ------------------------------------------------------------

struct Meta {
    e: Entity,
    k: KinematicBody,
    off: Vec2,
    was_grounded: bool,
    was_wall: i8,
}

fn position(ctx: &SystemCtx<'_>, e: Entity, gt: ComponentId) -> [f32; 2] {
    if ctx.has(e, gt) {
        if let Ok(g) = ctx.get::<GlobalTransform>(e) {
            return [g.pos.x, g.pos.y];
        }
    }
    ctx.get::<Transform>(e)
        .map(|t| [t.pos.x, t.pos.y])
        .unwrap_or([0.0, 0.0])
}

/// Exact for constant gravity: the average of the velocities at the start
/// and end of the tick, so a jump's apex is its declared height.
fn integrate(v: Vec2, gravity: f32, max_fall: f32, dt: f32) -> (f32, f32, f32) {
    let vy0 = v.y;
    let mut vy1 = vy0 - gravity * dt;
    if max_fall > 0.0 {
        vy1 = vy1.max(-max_fall);
    }
    (v.x * dt, (vy0 + vy1) * 0.5 * dt, vy1)
}

#[allow(clippy::too_many_arguments)]
fn step(
    ctx: &mut SystemCtx<'_>,
    tiles: &Rc<RefCell<MapStore>>,
    col: ComponentId,
    kb: ComponentId,
    cc: ComponentId,
    tm: ComponentId,
    tr: ComponentId,
    gt: ComponentId,
) -> Result<()> {
    if !any_with(ctx, kb) {
        return Ok(());
    }
    let dt = ctx.dt as f32;
    let mut store = tiles.borrow_mut();

    // The tile grids this tick.
    let mut grids: Vec<TileGrid> = Vec::new();
    for e in ctx.query().has(tm).entities() {
        let t: Tilemap = ctx.get(e)?;
        let file = ctx.world().strings().resolve(t.map);
        if file.is_empty() {
            continue;
        }
        if let Some(g) = store.grid(&file, position(ctx, e, gt)) {
            grids.push(g);
        }
    }

    // Bodies, in entity order.
    let mut ents = ctx.query().has(kb).has(col).has(tr).entities();
    ents.sort();
    let mut solids: Vec<Solid> = Vec::new();
    let mut solid_meta: Vec<Meta> = Vec::new();
    let mut actors: Vec<Actor> = Vec::new();
    let mut actor_meta: Vec<Meta> = Vec::new();
    for e in ents {
        let k: KinematicBody = ctx.get(e)?;
        let c: Collider = ctx.get(e)?;
        let t: Transform = ctx.get(e)?;
        let (hw, hh) = if c.radius > 0.0 {
            (c.radius, c.radius)
        } else {
            (c.size.x.abs() * 0.5, c.size.y.abs() * 0.5)
        };
        let b = Aabb::around(t.pos.x + c.offset.x, t.pos.y + c.offset.y, hw, hh);
        let meta = Meta {
            e,
            k,
            off: c.offset,
            was_grounded: k.grounded,
            was_wall: k.wall,
        };
        if k.solid {
            solids.push(Solid {
                e,
                b,
                one_way: k.one_way,
                layer: c.layer,
                mask: c.mask,
                moved: [0.0, 0.0],
            });
            solid_meta.push(meta);
        } else {
            let p = if ctx.has(e, cc) {
                let ch: CharacterController = ctx.get(e)?;
                Params {
                    step: ch.step_height.max(0.0),
                    corner: ch.corner_correction.max(0.0),
                }
            } else {
                Params::default()
            };
            actors.push(Actor {
                e,
                b,
                who: Asker {
                    e,
                    layer: c.layer,
                    mask: c.mask,
                    dropping: k.drop > 0.0,
                },
                p,
                vel: k.velocity,
                grounded: k.grounded,
                support: None,
                wall: 0,
                ceiling: false,
                corrected: None,
                crushed_by: None,
                impact: 0.0,
            });
            actor_meta.push(meta);
        }
    }
    if actors.is_empty() && solids.is_empty() {
        return Ok(());
    }

    let mut env = Env {
        store: &mut store,
        grids: &grids,
        solids,
    };

    // Who rides which solid, from last tick's contact and where they are.
    let mut rides: Vec<Option<usize>> = vec![None; actors.len()];
    for (i, a) in actors.iter_mut().enumerate() {
        let m = &actor_meta[i];
        if m.k.grounded && !m.k.ground.is_null() {
            if let Some(si) = env.solids.iter().position(|s| s.e == m.k.ground) {
                let s = env.solids[si].b;
                if (a.b.y0 - s.y1).abs() <= 4.0 * EPS && a.b.x0 < s.x1 - EPS && s.x0 < a.b.x1 - EPS
                {
                    rides[i] = Some(si);
                    a.support = Some(Support::Body {
                        idx: si,
                        one_way: env.solids[si].one_way,
                    });
                }
            }
        }
        if a.grounded && a.support.is_none() {
            // On tiles last tick: what is under it now.
            let feet = a.b.y0;
            a.support = env
                .ground(&a.b, feet - 2.0 * EPS, feet + EPS, feet + EPS, feet, &a.who)
                .map(|(_, s)| s);
            if a.support.is_none() {
                a.grounded = false;
            }
        }
    }

    // Solids move first, carrying and pushing.
    for (si, m) in solid_meta.iter_mut().enumerate() {
        let k = m.k;
        let (dx, dy, vy1) = integrate(k.velocity, k.gravity, k.max_fall, dt);
        m.k.velocity.y = vy1;
        let riders: Vec<bool> = rides.iter().map(|r| *r == Some(si)).collect();
        move_solid(&mut env, si, dx, dy, &mut actors, &riders);
    }

    // Then actors, on their own velocity.
    for (i, a) in actors.iter_mut().enumerate() {
        let k = actor_meta[i].k;
        let (dx, dy, vy1) = integrate(a.vel, k.gravity, k.max_fall, dt);
        let rising = a.vel.y > 0.0;
        a.vel.y = vy1;
        if rising {
            a.grounded = false;
            a.support = None;
        }
        let (_, hit_x) = a.move_x(&mut env, dx);
        if hit_x {
            a.vel.x = 0.0;
        }
        let (_, hit_y) = a.move_y(&mut env, dy, false);
        if hit_y && a.ceiling {
            a.vel.y = a.vel.y.min(0.0);
        }
        if a.grounded && a.vel.y < 0.0 {
            a.vel.y = 0.0;
        }
    }

    // Write back and report.
    let mut events: Events = Vec::new();
    for (si, s) in env.solids.iter().enumerate() {
        let m = &solid_meta[si];
        let pos = Vec2::new(s.b.cx() - m.off.x, s.b.cy() - m.off.y);
        let vel = m.k.velocity;
        ctx.update::<Transform, _>(m.e, |t| t.pos = pos)?;
        ctx.update::<KinematicBody, _>(m.e, |x| x.velocity = vel)?;
    }
    for (i, a) in actors.iter().enumerate() {
        let m = &actor_meta[i];
        let ground = match a.support {
            Some(Support::Body { idx, .. }) if a.grounded => env.solids[idx].e,
            _ => Entity::NULL,
        };
        let pos = Vec2::new(a.b.cx() - m.off.x, a.b.cy() - m.off.y);
        let (grounded, vel, wall, ceiling) = (a.grounded, a.vel, a.wall, a.ceiling);
        let on_one_way = grounded && a.support.is_some_and(|s| s.one_way());
        let slope = if grounded {
            a.support.map_or(0.0, |s| s.slope())
        } else {
            0.0
        };
        ctx.update::<Transform, _>(m.e, |t| t.pos = pos)?;
        ctx.update::<KinematicBody, _>(m.e, |x| {
            x.velocity = vel;
            x.grounded = grounded;
            x.ground = ground;
            x.on_one_way = on_one_way;
            x.slope = slope;
            x.wall = wall;
            x.ceiling = ceiling;
            x.drop = (x.drop - dt).max(0.0);
        })?;
        let e = Value::Entity(m.e);
        if grounded && !m.was_grounded {
            let mut f = vec![
                ("entity", e.clone()),
                ("speed", Value::Float(a.impact.max(0.0) as f64)),
            ];
            if !ground.is_null() {
                f.push(("on", Value::Entity(ground)));
            }
            ev(&mut events, "body.landed", f);
        }
        if wall != 0 && m.was_wall != wall {
            ev(
                &mut events,
                "body.wall",
                vec![("entity", e.clone()), ("side", Value::Int(wall as i64))],
            );
        }
        if ceiling {
            ev(&mut events, "body.ceiling", vec![("entity", e.clone())]);
        }
        if let Some([dx, dy]) = a.corrected {
            ev(
                &mut events,
                "body.corner_corrected",
                vec![
                    ("entity", e.clone()),
                    ("dx", Value::Float(dx as f64)),
                    ("dy", Value::Float(dy as f64)),
                ],
            );
        }
        if let Some(by) = a.crushed_by {
            ev(
                &mut events,
                "body.crushed",
                vec![("entity", e.clone()), ("by", Value::Entity(by))],
            );
        }
    }
    drop(env);
    drop(store);
    for (n, f) in events {
        ctx.emit(n, f);
    }
    Ok(())
}

// ----- physics.overlap ---------------------------------------------------------

#[derive(Default)]
struct OverlapState {
    /// Pairs overlapping at the end of last tick, sorted.
    contacts: Rc<RefCell<crate::Contacts>>,
    /// Slot of each collider this tick, by entity index (u32::MAX: none).
    slots: Vec<u32>,
}

#[derive(Clone, Copy)]
struct Shape {
    e: Entity,
    /// Hashed position.
    px: f32,
    py: f32,
    /// Shape centre.
    cx: f32,
    cy: f32,
    hw: f32,
    hh: f32,
    r: f32,
    layer: u32,
    mask: u32,
    /// Distance from the hashed position to the farthest point of the shape.
    reach: f32,
}

fn meet(a: &Shape, b: &Shape) -> bool {
    if (a.mask & b.layer) == 0 || (b.mask & a.layer) == 0 {
        return false;
    }
    let (dx, dy) = (b.cx - a.cx, b.cy - a.cy);
    match (a.r > 0.0, b.r > 0.0) {
        (true, true) => {
            let rr = a.r + b.r - EPS;
            dx * dx + dy * dy < rr * rr
        }
        (false, false) => dx.abs() < a.hw + b.hw - EPS && dy.abs() < a.hh + b.hh - EPS,
        (true, false) | (false, true) => {
            let (c, bx) = if a.r > 0.0 { (a, b) } else { (b, a) };
            let qx = (c.cx - bx.cx).clamp(-bx.hw, bx.hw) + bx.cx;
            let qy = (c.cy - bx.cy).clamp(-bx.hh, bx.hh) + bx.cy;
            let (ex, ey) = (c.cx - qx, c.cy - qy);
            let rr = c.r - EPS;
            ex * ex + ey * ey < rr * rr
        }
    }
}

fn overlap(
    ctx: &mut SystemCtx<'_>,
    st: &mut OverlapState,
    col: ComponentId,
    gt: ComponentId,
) -> Result<()> {
    if st.contacts.borrow().pairs.is_empty() && !any_with(ctx, col) {
        return Ok(());
    }
    let mut shapes: Vec<Shape> = Vec::new();
    let mut add = |e: Entity, p: Vec2, c: &Collider| {
        let (hw, hh) = (c.size.x.abs() * 0.5, c.size.y.abs() * 0.5);
        let r = c.radius.max(0.0);
        let extent = if r > 0.0 {
            r
        } else {
            (hw * hw + hh * hh).sqrt()
        };
        shapes.push(Shape {
            e,
            px: p.x,
            py: p.y,
            cx: p.x + c.offset.x,
            cy: p.y + c.offset.y,
            hw,
            hh,
            r,
            layer: c.layer,
            mask: c.mask,
            reach: extent + (c.offset.x * c.offset.x + c.offset.y * c.offset.y).sqrt(),
        });
    };
    ctx.query()
        .has(col)
        .not(gt)
        .each::<(&Collider, &Transform)>(|e, (c, t)| add(e, t.pos, c))?;
    ctx.query()
        .has(col)
        .each::<(&Collider, &GlobalTransform)>(|e, (c, g)| add(e, g.pos, c))?;
    if shapes.is_empty() && st.contacts.borrow().pairs.is_empty() {
        return Ok(());
    }
    shapes.sort_by_key(|s| s.e);
    ctx.rebuild_broadphase();

    let max_reach = shapes.iter().map(|s| s.reach).fold(0.0f32, f32::max);
    for (i, s) in shapes.iter().enumerate() {
        let ix = s.e.index() as usize;
        if st.slots.len() <= ix {
            st.slots.resize(ix + 1, u32::MAX);
        }
        st.slots[ix] = i as u32;
    }
    let mut pairs: Vec<(Entity, Entity)> = Vec::new();
    {
        let hash = ctx.world().broadphase();
        for (i, a) in shapes.iter().enumerate() {
            hash.for_each_within(a.px, a.py, a.reach + max_reach, |e, _, _| {
                let Some(&j) = st.slots.get(e.index() as usize) else {
                    return;
                };
                let j = j as usize;
                if j == u32::MAX as usize || j <= i || shapes[j].e != e {
                    return;
                }
                if meet(a, &shapes[j]) {
                    pairs.push((a.e, e));
                }
            });
        }
    }
    for s in &shapes {
        st.slots[s.e.index() as usize] = u32::MAX;
    }
    pairs.sort();
    pairs.dedup();

    // Begin and end, by merging with last tick's sorted pairs.
    let mut events: Events = Vec::new();
    let (mut i, mut j) = (0, 0);
    let prev = std::mem::take(&mut st.contacts.borrow_mut().pairs);
    while i < pairs.len() || j < prev.len() {
        match (pairs.get(i), prev.get(j)) {
            (Some(a), Some(b)) if a == b => {
                i += 1;
                j += 1;
            }
            (Some(a), b) if b.is_none_or(|b| a < b) => {
                ev(
                    &mut events,
                    "overlap.begin",
                    vec![
                        ("entity", Value::Entity(a.0)),
                        ("other", Value::Entity(a.1)),
                    ],
                );
                i += 1;
            }
            (_, Some(b)) => {
                ev(
                    &mut events,
                    "overlap.end",
                    vec![
                        ("entity", Value::Entity(b.0)),
                        ("other", Value::Entity(b.1)),
                    ],
                );
                j += 1;
            }
            _ => unreachable!(),
        }
    }
    st.contacts.borrow_mut().pairs = pairs;
    for (n, f) in events {
        ctx.emit(n, f);
    }
    Ok(())
}
