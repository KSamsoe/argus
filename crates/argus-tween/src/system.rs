//! `tween.update`: every tween, every tick, in the late stage.

use crate::ease::{ease, NAMES};
use crate::Tween;
use argus_ecs::{ComponentId, Entity, FieldSchema, FieldType, Result, StrId, Value, Vec2, World};
use argus_sim::{Scheduler, Stage, SystemCtx, SystemDesc};
use std::collections::{HashMap, HashSet};

/// The tween system's name.
pub const SYSTEM: &str = "tween.update";

/// Seconds short of its end at which a play counts as ended.
const END_EPS: f32 = 1e-4;

/// A resolved `Component.field[.x|.y]`.
#[derive(Clone)]
struct Target {
    comp: ComponentId,
    comp_name: String,
    field: FieldSchema,
    axis: Option<u8>,
}

#[derive(Default)]
struct State {
    paths: HashMap<StrId, std::result::Result<Target, String>>,
    easings: HashMap<StrId, Option<usize>>,
    reported: HashSet<Entity>,
}

pub(crate) fn install(world: &mut World, sched: &mut Scheduler) -> Result<()> {
    let tw = world.component_id_of::<Tween>().expect("registered");
    let mut st = State::default();
    sched.add_system(
        SystemDesc::new(SYSTEM, Stage::Late)
            .writes(tw)
            .source("argus-tween:update")
            .doc("Advances every Tween by sim time and writes its eased value into its target's field; emits its on_complete event and despawns it when done. Declares a write of each component a tween targets the first time it touches it."),
        Box::new(move |ctx| run(ctx, &mut st, tw)),
    )
}

fn resolve(world: &World, path: &str) -> std::result::Result<Target, String> {
    let parts: Vec<&str> = path.split('.').collect();
    let (base, axis) = match parts.len() {
        2 => (path.to_string(), None),
        3 if parts[2] == "x" => (format!("{}.{}", parts[0], parts[1]), Some(0)),
        3 if parts[2] == "y" => (format!("{}.{}", parts[0], parts[1]), Some(1)),
        _ => {
            return Err(format!(
                "path {path:?} is not Component.field or Component.vec.x"
            ))
        }
    };
    let (comp, field) = world
        .registry()
        .resolve_path(&base)
        .map_err(|e| format!("path {path:?}: {}", e.message))?;
    let numeric = !matches!(
        field.ty,
        FieldType::Str | FieldType::Ref | FieldType::Entity | FieldType::Bool | FieldType::Vec2
    );
    match (field.ty, axis) {
        (FieldType::Vec2, _) => {}
        (_, Some(_)) => return Err(format!("path {path:?}: only a vec2 has .x and .y")),
        _ if !numeric => {
            return Err(format!(
                "path {path:?} is a {} field; a tween drives numbers and vec2s",
                field.ty.name()
            ))
        }
        _ => {}
    }
    Ok(Target {
        comp,
        comp_name: world.registry().schema(comp).name.clone(),
        field: field.clone(),
        axis,
    })
}

fn as_vec(v: &Value, axis: Option<u8>) -> Vec2 {
    match (v, axis) {
        (Value::Vec2(v), None) => *v,
        (Value::Vec2(v), Some(0)) => Vec2::new(v.x, 0.0),
        (Value::Vec2(v), Some(_)) => Vec2::new(v.y, 0.0),
        (Value::Float(f), _) => Vec2::new(*f as f32, 0.0),
        (Value::Int(i), _) => Vec2::new(*i as f32, 0.0),
        (Value::UInt(u), _) => Vec2::new(*u as f32, 0.0),
        _ => Vec2::ZERO,
    }
}

/// Write `v` into the target's field: whole vec2, one axis, or a number
/// (rounded for integer fields), clamped to the field's range so an
/// overshooting ease (back, elastic) never fails the write.
fn write(world: &World, e: Entity, tg: &Target, v: Vec2) -> Result<()> {
    let clamp = |x: f64| match tg.field.range {
        Some((lo, hi)) => x.clamp(lo, hi),
        None => x,
    };
    let value = match (tg.field.ty, tg.axis) {
        (FieldType::Vec2, None) => Value::Vec2(v),
        (FieldType::Vec2, Some(a)) => {
            let mut cur = match world.get_field_by(e, tg.comp, &tg.field)? {
                Value::Vec2(c) => c,
                _ => Vec2::ZERO,
            };
            if a == 0 {
                cur.x = v.x;
            } else {
                cur.y = v.x;
            }
            Value::Vec2(cur)
        }
        (FieldType::F32 | FieldType::F64, _) => Value::Float(clamp(v.x as f64)),
        (FieldType::U8 | FieldType::U16 | FieldType::U32 | FieldType::U64, _) => {
            Value::UInt(clamp(v.x as f64).round().max(0.0) as u64)
        }
        _ => Value::Int(clamp(v.x as f64).round() as i64),
    };
    world.set_field_by(e, tg.comp, &tg.field, value)
}

type Events = Vec<(String, Vec<(String, Value)>)>;

fn run(ctx: &mut SystemCtx<'_>, st: &mut State, tw: ComponentId) -> Result<()> {
    if !ctx
        .world()
        .archetypes()
        .iter()
        .any(|a| !a.is_empty() && a.has(tw))
    {
        return Ok(());
    }
    let dt = ctx.dt as f32;
    let mut ents = ctx.query().has(tw).entities();
    ents.sort();
    let mut events: Events = Vec::new();
    let mut gone: Vec<Entity> = Vec::new();
    for e in ents {
        let mut t: Tween = ctx.get(e)?;
        if t.done {
            if !t.keep {
                gone.push(e);
            }
            continue;
        }
        let fail = |t: &mut Tween,
                    msg: String,
                    st: &mut State,
                    events: &mut Events,
                    gone: &mut Vec<Entity>| {
            if st.reported.insert(e) {
                events.push((
                    "tween.error".into(),
                    vec![
                        ("tween".into(), Value::Entity(e)),
                        ("message".into(), Value::Str(msg)),
                    ],
                ));
            }
            t.done = true;
            if !t.keep {
                gone.push(e);
            }
        };
        if t.target.is_null() || !ctx.is_alive(t.target) {
            // What it moved is gone; so is the tween.
            t.done = true;
            if !t.keep {
                gone.push(e);
            }
            ctx.update::<Tween, _>(e, |x| *x = t)?;
            continue;
        }
        let world = ctx.world();
        let tg = st
            .paths
            .entry(t.path)
            .or_insert_with(|| resolve(world, &world.strings().resolve(t.path)))
            .clone();
        let tg = match tg {
            Ok(tg) if ctx.has(t.target, tg.comp) => tg,
            Ok(tg) => {
                let msg = format!("target {} has no {}", t.target, tg.comp_name);
                fail(&mut t, msg, st, &mut events, &mut gone);
                ctx.update::<Tween, _>(e, |x| *x = t)?;
                continue;
            }
            Err(msg) => {
                fail(&mut t, msg, st, &mut events, &mut gone);
                ctx.update::<Tween, _>(e, |x| *x = t)?;
                continue;
            }
        };
        let curve = *st.easings.entry(t.easing).or_insert_with(|| {
            let n = world.strings().resolve(t.easing);
            if n.is_empty() {
                Some(0)
            } else {
                NAMES.iter().position(|x| *x == n)
            }
        });
        let Some(curve) = curve else {
            let msg = format!(
                "easing {:?} is not one of {}",
                world.strings().resolve(t.easing),
                NAMES.join(", ")
            );
            fail(&mut t, msg, st, &mut events, &mut gone);
            ctx.update::<Tween, _>(e, |x| *x = t)?;
            continue;
        };
        ctx.declare(tg.comp, true);
        let world = ctx.world();

        // The delay, then the start value, then time.
        let mut step = dt;
        if t.delay > 0.0 {
            let used = step.min(t.delay);
            t.delay -= used;
            step -= used;
            if t.delay > 0.0 {
                ctx.update::<Tween, _>(e, |x| *x = t)?;
                continue;
            }
        }
        if !t.started {
            if t.from_current {
                t.from = as_vec(&world.get_field_by(t.target, tg.comp, &tg.field)?, tg.axis);
            }
            t.started = true;
        }
        t.time += step;
        let d = t.duration.max(0.0);
        let mut finished = d <= 0.0;
        if d > 0.0 {
            // Within a hair of the end is the end: sixty steps of 1/60 in
            // f32 add up to just under 1, and a tween of a second ends on
            // its sixtieth tick, before a timeline's cue at its end time.
            while t.time >= d - END_EPS {
                if t.repeats < 0 || t.plays < t.repeats {
                    t.time -= d;
                    t.plays += 1;
                } else {
                    t.time = d;
                    finished = true;
                    break;
                }
            }
        }
        let p = if d > 0.0 {
            (t.time / d).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let p = if t.yoyo && t.plays % 2 == 1 {
            1.0 - p
        } else {
            p
        };
        let k = ease(NAMES[curve], p).unwrap_or(p);
        let v = Vec2::new(
            t.from.x + (t.to.x - t.from.x) * k,
            t.from.y + (t.to.y - t.from.y) * k,
        );
        write(world, t.target, &tg, v)?;
        t.value = v;
        if finished {
            t.done = true;
            if t.on_complete != StrId::EMPTY {
                events.push((
                    world.strings().resolve(t.on_complete),
                    vec![
                        ("entity".into(), Value::Entity(t.target)),
                        ("tween".into(), Value::Entity(e)),
                        ("path".into(), Value::Str(world.strings().resolve(t.path))),
                    ],
                ));
            }
            if !t.keep {
                gone.push(e);
            }
        }
        ctx.update::<Tween, _>(e, |x| *x = t)?;
    }
    for e in gone {
        if ctx.is_alive(e) {
            ctx.despawn(e)?;
        }
    }
    for (n, f) in events {
        ctx.emit(n, f);
    }
    Ok(())
}
