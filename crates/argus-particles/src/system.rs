//! `particles.update`: every emitter and every pool, every tick, last in
//! the late stage.

use crate::{Effect, Emitter, ParticleStore, Pool};
use argus_assets::{Area, Space};
use argus_ecs::{
    ComponentId, Entity, Event, GlobalTransform, Parent, Result, StrId, Transform, Value, World,
};
use argus_sim::{Pcg32, Scheduler, Stage, SystemCtx, SystemDesc};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// The particle system's name.
pub const SYSTEM: &str = "particles.update";

struct State {
    store: Rc<RefCell<ParticleStore>>,
    generation: u64,
    effects: HashMap<StrId, Option<Rc<Effect>>>,
    reported: HashSet<Entity>,
    list: Vec<(Entity, Emitter)>,
    bursts: Vec<(Entity, u32, Option<[f32; 2]>)>,
    gone: Vec<Entity>,
}

pub(crate) fn install(
    world: &mut World,
    sched: &mut Scheduler,
    store: Rc<RefCell<ParticleStore>>,
) -> Result<()> {
    let em = world.component_id_of::<Emitter>().expect("registered");
    let tr = world.component_id_of::<Transform>();
    let gt = world.component_id_of::<GlobalTransform>();
    let mut desc = SystemDesc::new(SYSTEM, Stage::Late)
        .between_turns()
        .writes(em)
        .rng("particles")
        .last()
        .source("argus-particles:update")
        .doc("Advances every emitter's particle pool by sim time (gravity, drag, the velocity curve, age) and emits new particles from the effect's rate and opening burst, Emitter.burst, and particles.burst events {emitter, count, pos?}. Writes Emitter's counters; emits particles.done when an emitter has finished and its last particle is gone, and particles.error once for an effect it cannot find.");
    if let Some(tr) = tr {
        desc = desc.reads(tr);
    }
    if let Some(gt) = gt {
        desc = desc.reads(gt);
    }
    if let Some(pa) = world.component_id_of::<Parent>() {
        desc = desc.reads(pa);
    }
    let mut st = State {
        store,
        generation: 0,
        effects: HashMap::new(),
        reported: HashSet::new(),
        list: Vec::new(),
        bursts: Vec::new(),
        gone: Vec::new(),
    };
    sched.add_system(desc, Box::new(move |ctx| run(ctx, &mut st, em)))
}

/// An entity's world position and rotation. A root's `Transform` is this
/// tick's; a child's `GlobalTransform` is from the last hierarchy pass,
/// which runs after the late stage.
fn place(world: &World, e: Entity) -> ([f32; 2], f32) {
    if world.get::<Parent>(e).is_ok() {
        if let Ok(g) = world.get::<GlobalTransform>(e) {
            return ([g.pos.x, g.pos.y], g.rot);
        }
    }
    match world.get::<Transform>(e) {
        Ok(t) => ([t.pos.x, t.pos.y], t.rot),
        Err(_) => ([0.0, 0.0], 0.0),
    }
}

fn range(rng: &mut Pcg32, r: [f32; 2]) -> f32 {
    if r[1] > r[0] {
        r[0] + (r[1] - r[0]) * rng.next_f32()
    } else {
        r[0]
    }
}

/// Emit one particle born `age` seconds ago at `at` (world space, or
/// relative to the emitter for a local effect), aimed by the emitter's
/// rotation `rot`.
fn spawn(pool: &mut Pool, rng: &mut Pcg32, at: [f32; 2], rot: f32, age: f32) {
    let fx = &*pool.effect;
    let s = &fx.spec;
    let life = range(rng, s.lifetime).max(1e-4);
    let speed = range(rng, s.speed);
    let dir = (s.direction + (rng.next_f32() - 0.5) * s.spread).to_radians() + rot;
    let (ds, dc) = dir.sin_cos();
    let v = [speed * dc, speed * ds];
    let off = match s.area {
        Area::Point => [0.0, 0.0],
        Area::Circle { radius } => {
            let r = radius * rng.next_f32().sqrt();
            let (a, b) = (rng.next_f32() * std::f32::consts::TAU).sin_cos();
            [r * b, r * a]
        }
        Area::Box { size } => [
            (rng.next_f32() - 0.5) * size[0],
            (rng.next_f32() - 0.5) * size[1],
        ],
    };
    let (rs, rc) = rot.sin_cos();
    let birth = [
        at[0] + off[0] * rc - off[1] * rs,
        at[1] + off[0] * rs + off[1] * rc,
    ];
    let k = fx.velocity.as_ref().map_or(1.0, |c| c.at(0.0));
    let p = [birth[0] + v[0] * k * age, birth[1] + v[1] * k * age];
    pool.push(p, v, age / life, 1.0 / life);
}

/// Emit `n` at once at `at`; returns (emitted, dropped).
fn burst(pool: &mut Pool, rng: &mut Pcg32, n: u32, at: [f32; 2], rot: f32) -> (u32, u32) {
    let room = (pool.effect.spec.max as usize).saturating_sub(pool.len()) as u32;
    let k = n.min(room);
    for _ in 0..k {
        spawn(pool, rng, at, rot, 0.0);
    }
    (k, n - k)
}

fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Int(i) => Some(*i as f64),
        Value::UInt(u) => Some(*u as f64),
        Value::Float(f) => Some(*f),
        _ => None,
    }
}

fn run(ctx: &mut SystemCtx<'_>, st: &mut State, em: ComponentId) -> Result<()> {
    let any = ctx
        .world()
        .archetypes()
        .iter()
        .any(|a| !a.is_empty() && a.has(em));
    let store = st.store.clone();
    let mut store = store.borrow_mut();
    if !any {
        store.pools.clear();
        return Ok(());
    }
    if store.generation != st.generation {
        st.effects.clear();
        st.generation = store.generation;
    }
    let ctx_dt = ctx.dt;
    let dt = ctx_dt as f32;
    let tick = ctx.tick;

    st.list.clear();
    let list = &mut st.list;
    ctx.query()
        .has(em)
        .each::<&Emitter>(|e, m| list.push((e, *m)))?;
    st.list.sort_unstable_by_key(|x| x.0);

    // Burst requests from events this tick: {emitter, count, pos?}.
    st.bursts.clear();
    for r in ctx.events_now("particles.burst") {
        if let Event::Game { fields, .. } = &r.event {
            let mut target = Entity::NULL;
            let mut count = 1u32;
            let mut pos = None;
            for (k, v) in fields {
                match (k.as_str(), v) {
                    ("emitter", Value::Entity(e)) => target = *e,
                    ("count", v) => count = num(v).unwrap_or(1.0).clamp(0.0, 1e6) as u32,
                    ("pos", Value::Vec2(p)) => pos = Some([p.x, p.y]),
                    _ => {}
                }
            }
            if !target.is_null() {
                st.bursts.push((target, count, pos));
            }
        }
    }

    st.gone.clear();
    let mut errors: Vec<(Entity, String)> = Vec::new();
    let mut finished: Vec<Entity> = Vec::new();
    for i in 0..st.list.len() {
        let (e, mut m) = st.list[i];
        let world = ctx.world();
        let fx = st
            .effects
            .entry(m.effect)
            .or_insert_with(|| {
                store
                    .effects
                    .get(world.strings().resolve(m.effect).as_str())
                    .cloned()
            })
            .clone();
        let Some(fx) = fx else {
            if st.reported.insert(e) {
                let name = world.strings().resolve(m.effect);
                let known: Vec<&str> = store.effects.keys().map(String::as_str).collect();
                errors.push((
                    e,
                    format!(
                        "effect {name:?} is not an emitter asset; the effects are {}",
                        if known.is_empty() {
                            "none yet".to_string()
                        } else {
                            known.join(", ")
                        }
                    ),
                ));
            }
            continue;
        };
        let (origin, rot) = place(world, e);
        let pool = store
            .pools
            .entry(e)
            .or_insert_with(|| Pool::new(fx.clone(), origin));
        if !Rc::ptr_eq(&pool.effect, &fx) {
            // A re-import: the new effect takes over the particles already out.
            pool.effect = fx.clone();
            let max = fx.spec.max as usize;
            if pool.len() > max {
                pool.truncate(max);
            }
        }
        pool.origin_prev = if pool.tick + 1 == tick {
            pool.origin
        } else {
            origin
        };
        pool.origin = origin;
        pool.step(dt);
        let local = fx.spec.space == Space::Local;
        let at = |p: [f32; 2]| {
            if local {
                [p[0] - origin[0], p[1] - origin[1]]
            } else {
                p
            }
        };
        let rng = ctx.rng()?;
        let (mut emitted, mut dropped) = (0u32, 0u32);
        let mut add = |r: (u32, u32)| {
            emitted += r.0;
            dropped += r.1;
        };

        if m.playing {
            if !m.started {
                m.started = true;
                m.time = 0.0;
                m.carry = 0.0;
                m.done = false;
                pool.announced = false;
                add(burst(pool, rng, fx.spec.burst, at(origin), rot));
            } else if !m.done {
                // The rate, each particle born when the accumulator crosses a
                // whole number, from where the emitter was at that moment.
                // In f64, with a hair of slack, so 60 a second at 60 Hz is one
                // a tick and not sometimes none.
                let d = fx.spec.duration as f64;
                let step = ctx_dt;
                let span = if d > 0.0 {
                    step.min((d - m.time as f64).max(0.0))
                } else {
                    step
                };
                let rate = fx.spec.rate * m.rate_scale;
                if rate > 0.0 && span > 0.0 {
                    let c0 = m.carry;
                    let owed = c0 as f64 + rate as f64 * span;
                    let n = (owed + 1e-4).floor() as u32;
                    m.carry = (owed - n as f64).max(0.0) as f32;
                    let prev = pool.origin_prev;
                    for j in 0..n {
                        let f = ((j as f32 + 1.0 - c0) / (rate * dt)).clamp(0.0, 1.0);
                        if pool.full() {
                            add((0, n - j));
                            break;
                        }
                        let born = if local {
                            [0.0, 0.0]
                        } else {
                            [
                                prev[0] + (origin[0] - prev[0]) * f,
                                prev[1] + (origin[1] - prev[1]) * f,
                            ]
                        };
                        spawn(pool, rng, born, rot, (1.0 - f) * dt);
                        add((1, 0));
                    }
                }
                m.time += dt;
            }
            if m.started
                && ((fx.spec.duration > 0.0 && m.time >= fx.spec.duration) || fx.spec.rate <= 0.0)
            {
                m.done = true;
            }
        }
        if m.burst > 0 {
            add(burst(pool, rng, m.burst, at(origin), rot));
            m.burst = 0;
        }
        for &(target, n, pos) in &st.bursts {
            if target == e {
                add(burst(pool, rng, n, at(pos.unwrap_or(origin)), rot));
            }
        }
        m.emitted = m.emitted.saturating_add(emitted);
        m.dropped = m.dropped.saturating_add(dropped);
        m.live = pool.len() as u32;
        if m.done && pool.is_empty() && !pool.announced {
            pool.announced = true;
            finished.push(e);
            if m.auto_despawn {
                st.gone.push(e);
            }
        }
        pool.tick = tick;
        ctx.update::<Emitter, _>(e, |x| *x = m)?;
    }
    // Pools of emitters that are gone, or lost their effect, go too.
    store.pools.retain(|_, p| p.tick == tick);
    drop(store);
    for (e, message) in errors {
        ctx.emit(
            "particles.error",
            vec![
                ("emitter".into(), Value::Entity(e)),
                ("message".into(), Value::Str(message)),
            ],
        );
    }
    for e in finished {
        ctx.emit("particles.done", vec![("emitter".into(), Value::Entity(e))]);
    }
    for i in 0..st.gone.len() {
        let e = st.gone[i];
        if ctx.is_alive(e) {
            ctx.despawn(e)?;
        }
    }
    Ok(())
}
