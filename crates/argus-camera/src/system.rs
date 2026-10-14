//! `camera.update`: every camera, every tick, at the end of the late stage.

use crate::{current, position, Camera, CameraShake, Zones};
use argus_ecs::{
    ComponentId, Entity, Event, GlobalTransform, Result, StrId, Transform, Value, Vec2, World,
};
use argus_sim::{Scheduler, Stage, SystemCtx, SystemDesc};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// The camera system's name.
pub const SYSTEM: &str = "camera.update";

#[derive(Default)]
struct State {
    /// The drawing camera after last tick.
    drawing: Option<Entity>,
    /// `Actions.<name>` paths by action name.
    paths: HashMap<StrId, String>,
}

pub(crate) fn install(
    world: &mut World,
    sched: &mut Scheduler,
    zones: Rc<RefCell<Zones>>,
) -> Result<()> {
    let cam = world.component_id_of::<Camera>().expect("registered");
    let shake = world.component_id_of::<CameraShake>().expect("registered");
    let tr = world
        .component_id_of::<Transform>()
        .expect("hierarchy registered");
    let gt = world
        .component_id_of::<GlobalTransform>()
        .expect("hierarchy registered");
    let actions = world.component_id("Actions");
    // On load too: the first update snaps to the target inside its room, so
    // the first frame (before any tick, or any turn) is framed.
    // Unfrozen too: during a hit-stop (phase 2 item 6) the view holds where
    // it is and the shake plays on, so a stopped shake does not hang still.
    let mut desc = SystemDesc::new(SYSTEM, Stage::Late)
        .last()
        .on_load()
        .unfrozen()
        .between_turns()
        .writes(cam)
        .writes(shake)
        .writes(tr)
        .reads(gt)
        .source("argus-camera:update")
        .doc("Moves every camera that follows a target or is panned (dead zone, lookahead, smoothing, room bounds, zoom), shakes the ones with trauma, writes each camera's view rect, and emits camera.cut when the drawing camera changes.");
    if let Some(a) = actions {
        desc = desc.reads(a);
    }
    let mut st = State::default();
    sched.add_system(
        desc,
        Box::new(move |ctx| run(ctx, &mut st, &zones, cam, shake, tr, actions)),
    )
}

/// Frame-rate independent approach: `tau` seconds is the time constant.
fn smooth(v: f32, to: f32, tau: f32, dt: f32) -> f32 {
    if tau <= 0.0 {
        to
    } else {
        v + (to - v) * (1.0 - (-dt / tau).exp())
    }
}

/// Where the camera wants to be on one axis so the aim is inside the dead
/// zone around it.
fn dead(pos: f32, aim: f32, zone: f32) -> f32 {
    let zone = zone.max(0.0);
    if aim - pos > zone {
        aim - zone
    } else if pos - aim > zone {
        aim + zone
    } else {
        pos
    }
}

/// Keep a view of half size `half` centred at `c` inside `[lo, hi]`; a
/// view wider than the bounds is centred on them.
fn clamp_axis(c: f32, half: f32, lo: f32, hi: f32) -> f32 {
    if hi - lo <= 2.0 * half {
        (lo + hi) * 0.5
    } else {
        c.clamp(lo + half, hi - half)
    }
}

fn hash(seed: u32, i: i64) -> f32 {
    let mut x = (i as u32).wrapping_mul(0x9E37_79B1) ^ seed.wrapping_mul(0x85EB_CA77);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    x = x.wrapping_mul(0x297A_2D39);
    x ^= x >> 15;
    (x as f32 / u32::MAX as f32) * 2.0 - 1.0
}

/// Smooth value noise in -1..1: the same seed and time give the same value,
/// so a replay shakes the same way, and no gameplay random stream is used.
fn noise(seed: u32, t: f32) -> f32 {
    let i = t.floor();
    let f = t - i;
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash(seed, i as i64);
    let b = hash(seed, i as i64 + 1);
    a + (b - a) * u
}

fn num(v: &Value) -> Option<f32> {
    match v {
        Value::Float(f) => Some(*f as f32),
        Value::Int(i) => Some(*i as f32),
        Value::UInt(u) => Some(*u as f32),
        _ => None,
    }
}

type Events = Vec<(String, Vec<(String, Value)>)>;

fn run(
    ctx: &mut SystemCtx<'_>,
    st: &mut State,
    zones: &Rc<RefCell<Zones>>,
    cam: ComponentId,
    shake: ComponentId,
    tr: ComponentId,
    actions: Option<ComponentId>,
) -> Result<()> {
    let any = ctx
        .world()
        .archetypes()
        .iter()
        .any(|a| !a.is_empty() && a.has(cam));
    if !any && st.drawing.is_none() {
        return Ok(());
    }
    let dt = ctx.dt as f32;
    let time = ctx.time as f32;
    // camera.shake {amount, camera}: raise trauma on that camera, or on the
    // one drawing.
    let requests: Vec<(f32, Option<Entity>)> = ctx
        .events_now("camera.shake")
        .filter_map(|r| match &r.event {
            Event::Game { fields, .. } => {
                let get = |k: &str| fields.iter().find(|(n, _)| n == k).map(|(_, v)| v);
                let amount = get("amount").and_then(num).unwrap_or(0.5);
                let camera = match get("camera") {
                    Some(Value::Entity(e)) => Some(*e),
                    _ => None,
                };
                Some((amount, camera))
            }
            _ => None,
        })
        .collect();
    let drawing_before = current(ctx.world());
    let zones = zones.borrow();
    let mut events: Events = Vec::new();
    let mut cams = ctx.query().has(cam).entities();
    cams.sort();
    // The camera that drew last tick, and what it showed: a blend into a
    // new one starts there. It is the world's (Camera.drew), so a seek or a
    // load blends as the run did.
    let mut drew: Option<(Entity, [f32; 2], f32)> = None;
    for &e in &cams {
        let c: Camera = ctx.get(e)?;
        if c.drew && drew.is_none() {
            drew = Some((
                e,
                [
                    (c.view_min.x + c.view_max.x) * 0.5,
                    (c.view_min.y + c.view_max.y) * 0.5,
                ],
                c.view_max.y - c.view_min.y,
            ));
        }
    }
    let mut blend_into: Option<f32> = None;
    for e in cams {
        let mut c: Camera = ctx.get(e)?;
        let has_actions = actions.is_some_and(|a| ctx.has(e, a));
        let mut path = |id: StrId, w: &World| {
            st.paths
                .entry(id)
                .or_insert_with(|| format!("Actions.{}", w.strings().resolve(id)))
                .clone()
        };
        // A hit-stop holds the view: no follow, pan, or zoom; only the shake.
        let moving = !ctx.frozen;
        if moving && has_actions && c.zoom_action != StrId::EMPTY {
            let p = path(c.zoom_action, ctx.world());
            if let Some(z) = ctx.get_field(e, &p).ok().as_ref().and_then(num) {
                c.height *= (-z * c.zoom_speed * dt).exp();
                if c.zoom_min > 0.0 {
                    c.height = c.height.max(c.zoom_min);
                }
                if c.zoom_max > 0.0 {
                    c.height = c.height.min(c.zoom_max);
                }
            }
        }
        let half = [
            c.height.max(0.01) * 0.5 * c.aspect,
            c.height.max(0.01) * 0.5,
        ];
        let follow = !c.target.is_null() && c.target != e && ctx.is_alive(c.target);
        let pan = has_actions && c.pan_action != StrId::EMPTY;
        let has_t = ctx.has(e, tr);
        let mut pos = position(ctx.world(), e).unwrap_or([0.0, 0.0]);
        if moving && has_t && (follow || pan) {
            pos = ctx.get::<Transform>(e).map(|t| [t.pos.x, t.pos.y])?;
            let (mut want, snap, focus) = if follow {
                let mut snap = false;
                let tp = position(ctx.world(), c.target).unwrap_or(pos);
                if !c.tracking {
                    c.target_last = Vec2::new(tp[0], tp[1]);
                    c.tracking = true;
                    snap = true;
                }
                let v = if dt > 0.0 {
                    [
                        (tp[0] - c.target_last.x) / dt,
                        (tp[1] - c.target_last.y) / dt,
                    ]
                } else {
                    [0.0, 0.0]
                };
                c.target_last = Vec2::new(tp[0], tp[1]);
                let lim = |v: f32, secs: f32, max: f32| (v * secs).clamp(-max.abs(), max.abs());
                let wa = [
                    lim(v[0], c.lookahead.x, c.lookahead_max.x),
                    lim(v[1], c.lookahead.y, c.lookahead_max.y),
                ];
                c.ahead = if snap {
                    Vec2::new(wa[0], wa[1])
                } else {
                    Vec2::new(
                        smooth(c.ahead.x, wa[0], c.lookahead_smoothing, dt),
                        smooth(c.ahead.y, wa[1], c.lookahead_smoothing, dt),
                    )
                };
                let aim = [
                    tp[0] + c.offset.x + c.ahead.x,
                    tp[1] + c.offset.y + c.ahead.y,
                ];
                let want = if snap {
                    aim
                } else {
                    [
                        dead(pos[0], aim[0], c.dead_zone.x),
                        dead(pos[1], aim[1], c.dead_zone.y),
                    ]
                };
                (want, snap, tp)
            } else {
                c.tracking = false;
                let p = path(c.pan_action, ctx.world());
                let v = match ctx.get_field(e, &p) {
                    Ok(Value::Vec2(v)) => [v.x, v.y],
                    Ok(other) => [num(&other).unwrap_or(0.0), 0.0],
                    Err(_) => [0.0, 0.0],
                };
                let step = c.pan_speed * c.height * dt;
                ([pos[0] + v[0] * step, pos[1] + v[1] * step], true, pos)
            };
            if c.bounded {
                let held = ctx.world().strings().resolve(c.zone);
                let zi = zones
                    .find(focus[0], focus[1])
                    .or_else(|| zones.named(&held));
                if let Some(zi) = zi {
                    let (name, r) = &zones.rects[zi];
                    if *name != held {
                        c.zone = ctx.world().strings().intern(name);
                        events.push((
                            "camera.zone".into(),
                            vec![
                                ("entity".into(), Value::Entity(e)),
                                ("zone".into(), Value::Str(name.clone())),
                            ],
                        ));
                    }
                    want = [
                        clamp_axis(want[0], half[0], r[0], r[2]),
                        clamp_axis(want[1], half[1], r[1], r[3]),
                    ];
                }
            }
            pos = if snap {
                want
            } else {
                [
                    smooth(pos[0], want[0], c.smoothing.x, dt),
                    smooth(pos[1], want[1], c.smoothing.y, dt),
                ]
            };
            let p = Vec2::new(pos[0], pos[1]);
            ctx.update::<Transform, _>(e, |t| t.pos = p)?;
        } else if moving {
            c.tracking = false;
        }

        let mut off = [0.0f32, 0.0];
        if ctx.has(e, shake) {
            let mut s: CameraShake = ctx.get(e)?;
            for (amount, to) in &requests {
                if *to == Some(e) || (to.is_none() && drawing_before == Some(e)) {
                    s.trauma = (s.trauma + amount).clamp(0.0, 1.0);
                }
            }
            let k = s.trauma.clamp(0.0, 1.0).powf(s.curve.max(0.01));
            let t = time * s.frequency;
            let seed = e.index().wrapping_mul(3);
            if k > 0.0 {
                s.offset = Vec2::new(
                    s.max_offset.x * k * noise(seed, t),
                    s.max_offset.y * k * noise(seed + 1, t),
                );
                s.angle = s.max_angle * k * noise(seed + 2, t);
            } else {
                // Exactly still (not -0.0) when there is no trauma.
                s.offset = Vec2::ZERO;
                s.angle = 0.0;
            }
            s.trauma = (s.trauma - s.decay * dt).max(0.0);
            off = [s.offset.x, s.offset.y];
            ctx.update::<CameraShake, _>(e, |x| *x = s)?;
        }
        // A blend: when it starts drawing, from what the last camera showed
        // towards its own view; a hit-stop holds it where it is.
        let draws = drawing_before == Some(e);
        if draws && !c.drew {
            c.blend_left = 0.0;
            if let Some((_, p, h)) = drew.filter(|d| d.0 != e && c.blend > 0.0) {
                c.blend_pos = Vec2::new(p[0], p[1]);
                c.blend_height = h;
                c.blend_left = c.blend;
                blend_into = Some(c.blend);
            }
        } else if !draws {
            c.blend_left = 0.0;
        } else if moving && c.blend_left > 0.0 {
            c.blend_left = (c.blend_left - dt).max(0.0);
        }
        c.drew = draws;
        let (center, height) = crate::blended(&c, [pos[0] + off[0], pos[1] + off[1]], c.height);
        let hh = [height.max(0.01) * 0.5 * c.aspect, height.max(0.01) * 0.5];
        c.view_min = Vec2::new(center[0] - hh[0], center[1] - hh[1]);
        c.view_max = Vec2::new(center[0] + hh[0], center[1] + hh[1]);
        ctx.update::<Camera, _>(e, |x| *x = c)?;
    }
    drop(zones);

    let now = current(ctx.world());
    // Which camera drew before: the world's record, or one despawned since.
    let from = drew
        .map(|d| d.0)
        .or(st.drawing.filter(|d| !ctx.is_alive(*d)));
    if let Some(from) = from {
        if now != Some(from) {
            let mut f = vec![("from".to_string(), Value::Entity(from))];
            if let Some(to) = now {
                f.push(("to".to_string(), Value::Entity(to)));
            }
            if let Some(b) = blend_into {
                f.push(("blend".to_string(), Value::Float(b as f64)));
            }
            events.push(("camera.cut".into(), f));
        }
    }
    st.drawing = now;
    for (n, f) in events {
        ctx.emit(n, f);
    }
    Ok(())
}
