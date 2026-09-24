//! `anim.update`: every animator, every tick, first in the late stage.

use crate::compile::{BState, CNum, COperand, CState, CTrans, Compiled, FieldRef};
use crate::{AnimStore, Animator};
use argus_ecs::{ComponentId, DynColumn, Entity, Result, StrId, Value, World};
use argus_sim::{Scheduler, Stage, SystemCtx, SystemDesc};
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

/// The animator system's name.
pub const SYSTEM: &str = "anim.update";

#[derive(Default)]
struct State {
    compiled: Compiled,
    reported: HashSet<(String, String)>,
    built: bool,
}

pub(crate) fn install(
    world: &mut World,
    sched: &mut Scheduler,
    store: Rc<RefCell<AnimStore>>,
) -> Result<()> {
    let anim = world.component_id_of::<Animator>().expect("registered");
    let sprite = world.component_id("Sprite");
    // On load too: every animator enters its first state before the first
    // tick, so the first frame (and a HUD naming the state) shows it.
    let mut desc = SystemDesc::new(SYSTEM, Stage::Late)
        .on_load()
        .writes(anim)
        .source("argus-anim:update")
        .doc("Runs every Animator's state machine: transitions as field comparisons, clip frames at their rates with anticipation and holds, events by frame, crossfades; writes Sprite.frame. It declares a read of each component its animator files' conditions name once it has compiled them.");
    if let Some(s) = sprite {
        desc = desc.writes(s);
    }
    let mut st = State::default();
    sched.add_system(
        desc,
        Box::new(move |ctx| run(ctx, &mut st, &store, anim, sprite)),
    )
}

type Events = Vec<(String, Vec<(String, Value)>)>;

/// Per-row reads: the archetype's columns for every component a condition
/// names (None where the archetype lacks it).
struct Cols<'a> {
    cols: Vec<Option<DynColumn<'a>>>,
}

impl Cols<'_> {
    fn read(&self, r: &FieldRef, row: usize) -> f32 {
        match self.cols.get(r.slot).and_then(|c| c.as_ref()) {
            Some(c) => c.num_by(row, &r.field, r.axis).unwrap_or(0.0) as f32,
            None => 0.0,
        }
    }
    fn num(&self, n: &Option<CNum>, row: usize) -> Option<f32> {
        match n {
            None => None,
            Some(CNum::Value(v)) => Some(*v),
            Some(CNum::Field(f)) => Some(self.read(f, row)),
        }
    }
}

fn holds(t: &CTrans, a: &Animator, cols: &Cols, row: usize) -> bool {
    if let Some([lo, hi]) = t.window {
        if a.frame < lo || a.frame > hi {
            return false;
        }
    }
    t.terms.iter().all(|term| {
        let v = match &term.what {
            COperand::Field(f, abs) => {
                let v = cols.read(f, row);
                if *abs {
                    v.abs()
                } else {
                    v
                }
            }
            COperand::End => a.ended as u8 as f32,
            COperand::Time => a.time,
            COperand::Frame => a.frame as f32,
            COperand::Loops => a.loops as f32,
        };
        let b = match term.cmp {
            Some((op, x)) => op.test(v, x),
            None => v != 0.0,
        };
        b != term.not
    })
}

/// A frame's duration in seconds: its base, the state's fps, the wind-up
/// stretched to its anticipation, the hold frame held longer.
fn duration(s: &CState, b: &BState, i: usize, cols: &Cols, row: usize) -> f32 {
    let mut d = match cols.num(&s.fps, row) {
        Some(fps) if fps > 0.0 => 1.0 / fps,
        _ => b.durs[i],
    };
    let unit = d;
    let n = b.frames.len();
    let windup = (s.windup as usize).min(n);
    if i < windup {
        if let Some(a) = cols.num(&s.anticipation, row) {
            d *= a.max(0.0) / windup as f32;
        }
    }
    let hold_at = s
        .hold_frame
        .map(|h| (h as usize).min(n - 1))
        .unwrap_or(n - 1);
    if i == hold_at {
        if let Some(h) = cols.num(&s.hold, row) {
            d += h.max(0.0) * unit;
        }
    }
    d.max(1e-4)
}

fn fire(e: Entity, s: &CState, frame: u32, out: &mut Events) {
    for ev in s.events.iter().filter(|ev| ev.frame == frame) {
        out.push((
            ev.name.clone(),
            vec![
                ("entity".into(), Value::Entity(e)),
                ("frame".into(), Value::Int(frame as i64)),
                ("state".into(), Value::Str(s.name.clone())),
            ],
        ));
        if let Some(snd) = &ev.sound {
            out.push((
                "sound.play".into(),
                vec![
                    ("entity".into(), Value::Entity(e)),
                    ("sound".into(), Value::Str(snd.clone())),
                ],
            ));
        }
    }
}

fn run(
    ctx: &mut SystemCtx<'_>,
    st: &mut State,
    store: &Rc<RefCell<AnimStore>>,
    anim: ComponentId,
    sprite: Option<ComponentId>,
) -> Result<()> {
    if !ctx
        .world()
        .archetypes()
        .iter()
        .any(|a| !a.is_empty() && a.has(anim))
    {
        return Ok(());
    }
    let dt = ctx.dt as f32;
    let store = store.borrow();
    if !st.built || st.compiled.generation != store.generation {
        st.compiled = Compiled::build(&store, ctx.world(), &[Some(anim), sprite]);
        st.built = true;
    }
    // The components the machines' conditions read join the declared
    // reads, so who_reads names this system (§3).
    for i in 0..st.compiled.used.len() {
        ctx.declare(st.compiled.used[i], false);
    }
    let mut events: Events = Vec::new();
    let world: &World = ctx.world();
    let compiled = &mut st.compiled;
    let (atlas_f, frame_f, bframe_f, blend_f) = match sprite.map(|s| world.registry().schema(s)) {
        Some(sc) => (
            sc.field("atlas").cloned(),
            sc.field("frame").cloned(),
            sc.field("blend_frame").cloned(),
            sc.field("blend").cloned(),
        ),
        None => (None, None, None, None),
    };
    world.query().has(anim).for_each_view(|view| {
        let arch = view.archetype();
        let mut anims = view.column_mut::<Animator>()?;
        let sprites = match sprite.filter(|s| arch.has(*s)) {
            Some(s) => Some(view.dyn_column_mut(s)?),
            None => None,
        };
        let cols = Cols {
            cols: compiled
                .used
                .iter()
                .map(|&c| {
                    if arch.has(c) && Some(c) != sprite {
                        view.dyn_column(c).ok()
                    } else {
                        None
                    }
                })
                .collect(),
        };
        let entities = view.entities();
        // Which machine and sprite each row plays, bound first, so the row
        // loop below only reads (rows of an archetype mostly share one).
        let atlas_of = |a: &Animator, row: usize| {
            if a.sprite != StrId::EMPTY {
                a.sprite
            } else {
                match (&sprites, &atlas_f) {
                    (Some(sc), Some(f)) => sc.str_id_by(row, f).unwrap_or(StrId::EMPTY),
                    _ => StrId::EMPTY,
                }
            }
        };
        let mut last: Option<(StrId, StrId)> = None;
        for row in 0..view.len() {
            let a = &anims[row];
            let key = (a.machine, atlas_of(a, row));
            if last != Some(key) {
                if let Some(&mi) = compiled.by_name.get(&key.0) {
                    compiled.bind(mi, key.1, world, &store);
                }
                last = Some(key);
            }
        }
        let compiled: &Compiled = compiled;
        let mut last: Option<(StrId, StrId, usize, Option<&crate::compile::Bound>)> = None;
        for row in 0..view.len() {
            let e = entities[row];
            let a = &mut anims[row];
            let atlas = atlas_of(a, row);
            let (mi, bound) = match last {
                Some((m, s, mi, b)) if m == a.machine && s == atlas => (mi, b),
                _ => {
                    let Some(&mi) = compiled.by_name.get(&a.machine) else {
                        continue;
                    };
                    let b = compiled.bound.get(&(mi, atlas)).and_then(|b| b.as_ref());
                    last = Some((a.machine, atlas, mi, b));
                    (mi, b)
                }
            };
            let Some(bound) = bound else {
                continue;
            };
            let m = &compiled.machines[mi];
            let cur = m.by_id.get(&a.state).copied();
            let atlas_frame = |a: &Animator, s: usize| {
                bound.states[s].frames[(a.frame as usize).min(bound.states[s].frames.len() - 1)]
            };

            // Where to go: a play request, a transition, or the first state.
            let mut next: Option<(usize, f32)> = None;
            if a.play != StrId::EMPTY {
                if let Some(&s) = m.by_id.get(&a.play) {
                    next = Some((s, 0.0));
                }
                a.play = StrId::EMPTY;
            } else if let Some(c) = cur {
                for t in m
                    .any
                    .iter()
                    .filter(|t| t.to != c)
                    .chain(m.states[c].trans.iter())
                {
                    if holds(t, a, &cols, row) {
                        next = Some((t.to, t.blend));
                        break;
                    }
                }
            } else {
                next = Some((m.initial, 0.0));
            }

            if let Some((s, blend)) = next {
                let from = cur.map(|c| (m.states[c].name.clone(), atlas_frame(a, c)));
                a.state = m.states[s].id;
                a.time = 0.0;
                a.frame = 0;
                a.phase = 0.0;
                a.loops = 0;
                a.ended = false;
                let mut f = vec![
                    ("entity".to_string(), Value::Entity(e)),
                    ("state".to_string(), Value::Str(m.states[s].name.clone())),
                ];
                if let Some((name, old_frame)) = &from {
                    f.push(("from".to_string(), Value::Str(name.clone())));
                    if blend > 0.0 {
                        a.blend_left = blend;
                        a.blend_total = blend;
                        if let (Some(sc), Some(bf)) = (&sprites, &bframe_f) {
                            sc.set_by(row, bf, Value::UInt(*old_frame as u64))?;
                        }
                    }
                }
                events.push(("anim.entered".into(), f));
                fire(e, &m.states[s], 0, &mut events);
            } else if let Some(c) = cur {
                // Play on: frames at their durations, events for every frame
                // passed, the clip's end.
                let (s, b) = (&m.states[c], &bound.states[c]);
                let step = dt * a.speed.max(0.0);
                a.time += step;
                let n = b.frames.len();
                if !(a.ended && !b.looping) {
                    a.phase += step;
                    let mut guard = 0;
                    loop {
                        let d = duration(s, b, a.frame as usize, &cols, row);
                        if a.phase < d || guard > 4 * n + 4 {
                            break;
                        }
                        guard += 1;
                        a.phase -= d;
                        if (a.frame as usize) + 1 < n {
                            a.frame += 1;
                            fire(e, s, a.frame, &mut events);
                        } else if b.looping {
                            a.frame = 0;
                            a.loops += 1;
                            a.ended = true;
                            fire(e, s, 0, &mut events);
                        } else {
                            a.ended = true;
                            a.phase = d;
                            break;
                        }
                    }
                }
                a.blend_left = (a.blend_left - step).max(0.0);
            }

            // The sprite shows the clip's frame, and a fading crossfade.
            if let (Some(sc), Some(ff)) = (&sprites, &frame_f) {
                let s = m.by_id[&a.state];
                sc.set_by(row, ff, Value::UInt(atlas_frame(a, s) as u64))?;
                if let Some(bw) = &blend_f {
                    let w = if a.blend_total > 0.0 {
                        a.blend_left / a.blend_total
                    } else {
                        0.0
                    };
                    sc.set_by(row, bw, Value::Float(w as f64))?;
                }
            }
        }
        Ok(())
    })?;
    for (machine, message, at) in std::mem::take(&mut st.compiled.errors) {
        if st.reported.insert((machine.clone(), message.clone())) {
            let mut f = vec![
                ("machine".to_string(), Value::Str(machine)),
                ("message".to_string(), Value::Str(message)),
            ];
            if !at.is_empty() {
                f.push(("at".to_string(), Value::Str(at)));
            }
            events.push(("anim.error".into(), f));
        }
    }
    drop(store);
    for (n, f) in events {
        ctx.emit(n, f);
    }
    Ok(())
}
