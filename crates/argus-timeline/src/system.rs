//! `timeline.update` (first in the late stage): every timeline playing
//! fires the cues it has reached, holds for the ones that wait, ends the
//! camera cuts whose time is up, and moves on by sim time.

use crate::table::{Cue, Kind, Line, Timelines, Who};
use crate::{Timeline, TimelineCut, TimelinePart};
use argus_anim::Animator;
use argus_camera::Camera;
use argus_dialogue::graph::{holds, make_edits};
use argus_dialogue::Talk;
use argus_ecs::{ComponentId, Entity, Result, StrId, Value, Vec2};
use argus_sim::{Scheduler, Stage, SystemCtx, SystemDesc};
use argus_tween::Tween;
use std::cell::RefCell;
use std::rc::Rc;

/// The system's name.
pub const UPDATE: &str = "timeline.update";

/// A cue at `at` fires once the time is within this of it (sim time adds
/// up steps of a float).
const EPS: f32 = 1e-4;

#[derive(Clone, Copy)]
struct Ids {
    timeline: ComponentId,
    part: ComponentId,
    cut: ComponentId,
    tween: ComponentId,
    talk: ComponentId,
    animator: ComponentId,
    camera: ComponentId,
}

pub(crate) fn install(
    world: &mut argus_ecs::World,
    sched: &mut Scheduler,
    lines: Rc<RefCell<Timelines>>,
) -> Result<()> {
    let need = |id: Option<ComponentId>, what: &str| {
        id.ok_or_else(|| {
            argus_ecs::ArgusError::new(
                argus_ecs::codes::UNKNOWN_COMPONENT,
                format!("timelines need {what} registered first"),
            )
        })
    };
    let ids = Ids {
        timeline: need(world.component_id_of::<Timeline>(), "Timeline")?,
        part: need(world.component_id_of::<TimelinePart>(), "TimelinePart")?,
        cut: need(world.component_id_of::<TimelineCut>(), "TimelineCut")?,
        tween: need(world.component_id_of::<Tween>(), "Tween")?,
        talk: need(world.component_id_of::<Talk>(), "Talk")?,
        animator: need(world.component_id_of::<Animator>(), "Animator")?,
        camera: need(world.component_id_of::<Camera>(), "Camera")?,
    };
    let desc = SystemDesc::new(UPDATE, Stage::Late)
        .first()
        .writes(ids.timeline)
        .writes(ids.part)
        .writes(ids.cut)
        .writes(ids.tween)
        .writes(ids.talk)
        .writes(ids.animator)
        .writes(ids.camera)
        .source("argus-timeline:update")
        .doc("Plays every Timeline (section 13): fires the cues of its row of the timeline table as its time reaches them (a tween started, an animator sent to a state, a camera made to draw, a conversation begun, edits made and events emitted), holds for a cue that waits, ends camera cuts whose time is up, and moves on by sim time times its speed. A skip jumps to the end. Emits timeline.started, timeline.cue, timeline.held, timeline.resumed, timeline.looped, timeline.ended, and timeline.problem. Declares a write of each component an event's edits name as it makes them.");
    sched.add_system(desc, Box::new(move |ctx| run(ctx, &lines.borrow(), ids)))
}

fn run(ctx: &mut SystemCtx<'_>, lines: &Timelines, ids: Ids) -> Result<()> {
    // A world with no timelines and no cuts pays nothing, not even a query.
    let (any_tl, any_cut) = ctx
        .world()
        .archetypes()
        .iter()
        .filter(|a| !a.is_empty())
        .fold((false, false), |(t, c), a| {
            (t || a.has(ids.timeline), c || a.has(ids.cut))
        });
    if !any_tl && !any_cut {
        return Ok(());
    }
    // A cut whose timeline was despawned by someone else ends with it.
    if any_cut {
        let mut ends = Vec::new();
        for x in ctx.query().has(ids.cut).entities() {
            let owner = ctx.get::<TimelineCut>(x)?.timeline;
            if !ctx.is_alive(owner) || !ctx.has(owner, ids.timeline) {
                ends.push(x);
            }
        }
        ends.sort();
        for x in ends {
            close_cut(ctx, x, ids)?;
        }
    }
    if !any_tl {
        return Ok(());
    }
    let mut ents = ctx.query().has(ids.timeline).entities();
    ents.sort();
    for e in ents {
        play(ctx, lines, e, ids)?;
    }
    Ok(())
}

fn name_of(ctx: &SystemCtx<'_>, id: StrId) -> String {
    ctx.world().strings().resolve(id).to_string()
}

/// The parties, in the order of `table::ROLES`.
fn parties(ctx: &SystemCtx<'_>, t: &Timeline) -> [Entity; 3] {
    [
        t.actor,
        t.other,
        argus_camera::current(ctx.world()).unwrap_or(Entity::NULL),
    ]
}

fn resolve(ctx: &SystemCtx<'_>, t: &Timeline, who: &Who) -> Option<Entity> {
    let e = match who {
        Who::Role(i) => *parties(ctx, t).get(*i)?,
        Who::Id(id) => ctx.world().entity(&format!("@{id}")).ok()?,
    };
    (!e.is_null() && ctx.is_alive(e)).then_some(e)
}

fn problem(ctx: &mut SystemCtx<'_>, e: Entity, name: &str, cue: &str, why: &str) {
    ctx.emit(
        "timeline.problem",
        vec![
            ("timeline".into(), Value::Entity(e)),
            ("name".into(), Value::Str(name.to_string())),
            ("cue".into(), Value::Str(cue.to_string())),
            ("why".into(), Value::Str(why.to_string())),
        ],
    );
}

fn play(ctx: &mut SystemCtx<'_>, lines: &Timelines, e: Entity, ids: Ids) -> Result<()> {
    let mut t: Timeline = ctx.get(e)?;
    let name = name_of(ctx, t.name);
    let Some(line) = lines.line(&name) else {
        problem(
            ctx,
            e,
            &name,
            "",
            &format!("no row of the timeline table is named {name:?}"),
        );
        return finish(ctx, e, &name, &t, false, ids);
    };
    let key = line.key.clone();
    let fresh = !t.started;
    if fresh {
        t.started = true;
        t.began = ctx.tick;
        t.length = line.length;
        t.time = 0.0;
        t.next = 0;
        ctx.emit(
            "timeline.started",
            vec![
                ("timeline".into(), Value::Entity(e)),
                ("name".into(), Value::Str(key.clone())),
                ("actor".into(), Value::Entity(t.actor)),
                ("other".into(), Value::Entity(t.other)),
                ("length".into(), Value::Float(line.length as f64)),
            ],
        );
    }
    if t.skip {
        return skip(ctx, line, e, t, ids);
    }
    if t.paused || t.speed <= 0.0 {
        return ctx.update::<Timeline, _>(e, |x| *x = t);
    }
    if t.held {
        if still_waiting(ctx, line, &t, ids) {
            return ctx.update::<Timeline, _>(e, |x| *x = t);
        }
        let at = if t.waiting.is_null() {
            t.next as usize
        } else {
            (t.next as usize).saturating_sub(1)
        };
        t.held = false;
        t.waiting = Entity::NULL;
        let cue = line.cues.get(at).map(|c| c.key.clone()).unwrap_or_default();
        ctx.emit(
            "timeline.resumed",
            vec![
                ("timeline".into(), Value::Entity(e)),
                ("name".into(), Value::Str(key.clone())),
                ("cue".into(), Value::Str(cue)),
                ("time".into(), Value::Float(t.time as f64)),
            ],
        );
    }
    // Time moves on (from the tick after it began), then the cues it has
    // reached fire in order, so a frame shows everything up to its time
    // done; one that waits holds the rest.
    if !fresh && !t.held {
        t.time += ctx.dt as f32 * t.speed;
    }
    while let Some(c) = line.cues.get(t.next as usize) {
        if c.at > t.time + EPS {
            break;
        }
        if c.kind == Kind::Event && c.wait {
            let who = parties(ctx, &t);
            if !holds(ctx.world(), c.when.as_ref(), &who) {
                hold(ctx, e, &key, c, &mut t, Entity::NULL);
                break;
            }
        }
        let i = t.next as usize;
        t.next += 1;
        let waiting = fire(ctx, line, e, &t, i, ids)?;
        if c.wait && c.kind != Kind::Event {
            if let Some(w) = waiting {
                hold(ctx, e, &key, c, &mut t, w);
                break;
            }
        }
    }
    // Cuts whose time is up.
    end_cuts(ctx, e, Some(t.time + EPS), ids)?;
    if t.next as usize >= line.cues.len() && !t.held && t.time >= t.length - EPS {
        if t.looping {
            end_cuts(ctx, e, None, ids)?;
            t.plays += 1;
            t.time = 0.0;
            t.next = 0;
            ctx.emit(
                "timeline.looped",
                vec![
                    ("timeline".into(), Value::Entity(e)),
                    ("name".into(), Value::Str(key)),
                    ("plays".into(), Value::UInt(t.plays as u64)),
                ],
            );
        } else {
            return finish(ctx, e, &key, &t, false, ids);
        }
    }
    ctx.update::<Timeline, _>(e, |x| *x = t)
}

fn hold(ctx: &mut SystemCtx<'_>, e: Entity, name: &str, c: &Cue, t: &mut Timeline, on: Entity) {
    t.held = true;
    t.waiting = on;
    ctx.emit(
        "timeline.held",
        vec![
            ("timeline".into(), Value::Entity(e)),
            ("name".into(), Value::Str(name.to_string())),
            ("cue".into(), Value::Str(c.key.clone())),
            ("on".into(), Value::Entity(on)),
            ("time".into(), Value::Float(t.time as f64)),
        ],
    );
}

/// Whether a held timeline still waits: an event's condition (no entity),
/// or the conversation, tween, or animator state its last cue started.
fn still_waiting(ctx: &SystemCtx<'_>, line: &Line, t: &Timeline, ids: Ids) -> bool {
    if t.waiting.is_null() {
        let Some(c) = line.cues.get(t.next as usize) else {
            return false;
        };
        return !holds(ctx.world(), c.when.as_ref(), &parties(ctx, t));
    }
    let w = t.waiting;
    if !ctx.is_alive(w) {
        return false;
    }
    let Some(c) = (t.next as usize)
        .checked_sub(1)
        .and_then(|i| line.cues.get(i))
    else {
        return false;
    };
    match c.kind {
        Kind::Line => ctx.has(w, ids.talk),
        Kind::Tween => ctx.get::<Tween>(w).is_ok_and(|x| !x.done),
        Kind::Anim => {
            let s = ctx.world().strings().intern(&c.state);
            ctx.get::<Animator>(w)
                .is_ok_and(|a| a.play == s || (a.state == s && !a.ended))
        }
        _ => false,
    }
}

/// Fire cue `i`: what it does, its sound, and `timeline.cue`. Returns what
/// a wait would wait for.
fn fire(
    ctx: &mut SystemCtx<'_>,
    line: &Line,
    e: Entity,
    t: &Timeline,
    i: usize,
    ids: Ids,
) -> Result<Option<Entity>> {
    let c = &line.cues[i];
    let who = parties(ctx, t);
    let cue_event = |ctx: &mut SystemCtx<'_>, passed: bool| {
        ctx.emit(
            "timeline.cue",
            vec![
                ("timeline".into(), Value::Entity(e)),
                ("name".into(), Value::Str(line.key.clone())),
                ("cue".into(), Value::Str(c.key.clone())),
                ("kind".into(), Value::Str(c.kind.name().to_string())),
                ("track".into(), Value::Str(c.track.clone())),
                ("at".into(), Value::Float(c.at as f64)),
                ("time".into(), Value::Float(t.time as f64)),
                ("passed".into(), Value::Bool(passed)),
            ],
        );
    };
    // A condition that fails passes the cue over (an event that waits was
    // waited for before it got here).
    if !(c.kind == Kind::Event && c.wait) && !holds(ctx.world(), c.when.as_ref(), &who) {
        cue_event(ctx, true);
        return Ok(None);
    }
    let target = resolve(ctx, t, &c.who);
    let mut waiting = None;
    let nobody = |ctx: &mut SystemCtx<'_>, what: &str| {
        problem(
            ctx,
            e,
            &line.key,
            &c.key,
            &format!("{what}: its who ({:?}) is no one here", c.who),
        );
    };
    match c.kind {
        Kind::Tween => match target {
            Some(target) => {
                let s = ctx.world().strings();
                let tw = Tween {
                    target,
                    path: s.intern(&c.path),
                    from: c.from.unwrap_or(Vec2::ZERO),
                    from_current: c.from.is_none(),
                    to: c.to,
                    duration: c.duration / t.speed.max(1e-3),
                    easing: s.intern(&c.easing),
                    ..Tween::default()
                };
                let x = ctx.spawn();
                ctx.insert(x, tw)?;
                ctx.insert(
                    x,
                    TimelinePart {
                        timeline: e,
                        cue: i as u32,
                    },
                )?;
                waiting = Some(x);
            }
            None => nobody(ctx, "nothing to tween"),
        },
        Kind::Anim => match target.filter(|x| ctx.has(*x, ids.animator)) {
            Some(target) => {
                let s = ctx.world().strings().intern(&c.state);
                ctx.update::<Animator, _>(target, |a| a.play = s)?;
                waiting = Some(target);
            }
            None => nobody(ctx, "no Animator to send to a state"),
        },
        Kind::Camera => match target.filter(|x| ctx.has(*x, ids.camera)) {
            Some(cam) => {
                let was = ctx.get::<Camera>(cam)?.active;
                ctx.update::<Camera, _>(cam, |x| x.active = true)?;
                let x = ctx.spawn();
                ctx.insert(
                    x,
                    TimelineCut {
                        timeline: e,
                        cue: i as u32,
                        camera: cam,
                        until: if c.duration > 0.0 {
                            c.at + c.duration
                        } else {
                            -1.0
                        },
                        was_active: was,
                    },
                )?;
            }
            None => nobody(ctx, "no Camera to cut to"),
        },
        Kind::Line => {
            let speaker = target.unwrap_or(Entity::NULL);
            let listener = if c.who == Who::Role(0) {
                t.other
            } else {
                t.actor
            };
            let start = ctx
                .world()
                .strings()
                .intern(&format!("dialogue:{}", c.line));
            let x = ctx.spawn();
            ctx.insert(
                x,
                Talk {
                    start,
                    speaker,
                    listener,
                    ..Talk::default()
                },
            )?;
            ctx.insert(
                x,
                TimelinePart {
                    timeline: e,
                    cue: i as u32,
                },
            )?;
            waiting = Some(x);
        }
        Kind::Event => edits(ctx, line, e, t, c, &who),
    }
    if !c.sound.is_empty() {
        ctx.emit(
            "sound.play",
            vec![
                (
                    "entity".into(),
                    Value::Entity(target.unwrap_or(Entity::NULL)),
                ),
                ("sound".into(), Value::Str(c.sound.clone())),
            ],
        );
    }
    cue_event(ctx, false);
    Ok(waiting)
}

fn edits(ctx: &mut SystemCtx<'_>, line: &Line, e: Entity, t: &Timeline, c: &Cue, who: &[Entity]) {
    if c.effects.is_empty() {
        return;
    }
    let fields = [
        ("timeline".to_string(), Value::Entity(e)),
        ("name".to_string(), Value::Str(line.key.clone())),
        ("cue".to_string(), Value::Str(c.key.clone())),
        ("actor".to_string(), Value::Entity(t.actor)),
        ("other".to_string(), Value::Entity(t.other)),
    ];
    let mut problems = Vec::new();
    make_edits(ctx, &c.effects, who, &fields, &mut problems);
    for why in problems {
        problem(ctx, e, &line.key, &c.key, &why);
    }
}

/// Jump to the end: what it started finishes (tweens to their ends,
/// conversations stopped), the cues left are done at once (tweens to
/// their ends, animator states entered, edits made; lines are not said
/// and cuts would be over), and it ends.
fn skip(ctx: &mut SystemCtx<'_>, line: &Line, e: Entity, mut t: Timeline, ids: Ids) -> Result<()> {
    let mut started: Vec<Entity> = Vec::new();
    for x in ctx.query().has(ids.part).entities() {
        if ctx.get::<TimelinePart>(x)?.timeline == e {
            started.push(x);
        }
    }
    started.sort();
    for x in started {
        if ctx.has(x, ids.tween) {
            ctx.update::<Tween, _>(x, |w| {
                w.delay = 0.0;
                w.time = w.duration;
            })?;
        }
        if ctx.has(x, ids.talk) {
            ctx.update::<Talk, _>(x, |k| k.stop = true)?;
        }
    }
    while let Some(c) = line.cues.get(t.next as usize) {
        t.next += 1;
        let who = parties(ctx, &t);
        if !holds(ctx.world(), c.when.as_ref(), &who) {
            continue;
        }
        let target = resolve(ctx, &t, &c.who);
        match c.kind {
            Kind::Tween => {
                if let Some(target) = target {
                    let s = ctx.world().strings();
                    let tw = Tween {
                        target,
                        path: s.intern(&c.path),
                        from_current: true,
                        to: c.to,
                        duration: 0.0,
                        ..Tween::default()
                    };
                    let x = ctx.spawn();
                    ctx.insert(x, tw)?;
                }
            }
            Kind::Anim => {
                if let Some(target) = target.filter(|x| ctx.has(*x, ids.animator)) {
                    let s = ctx.world().strings().intern(&c.state);
                    ctx.update::<Animator, _>(target, |a| a.play = s)?;
                }
            }
            Kind::Event => edits(ctx, line, e, &t, c, &who),
            Kind::Camera | Kind::Line => {}
        }
    }
    t.held = false;
    t.waiting = Entity::NULL;
    let key = line.key.clone();
    finish(ctx, e, &key, &t, true, ids)
}

/// End a timeline: its cuts end, `timeline.ended`, and it goes.
fn finish(
    ctx: &mut SystemCtx<'_>,
    e: Entity,
    name: &str,
    t: &Timeline,
    skipped: bool,
    ids: Ids,
) -> Result<()> {
    end_cuts(ctx, e, None, ids)?;
    ctx.emit(
        "timeline.ended",
        vec![
            ("timeline".into(), Value::Entity(e)),
            ("name".into(), Value::Str(name.to_string())),
            ("skipped".into(), Value::Bool(skipped)),
            ("time".into(), Value::Float(t.time as f64)),
        ],
    );
    ctx.despawn(e)
}

/// End a timeline's cuts: those whose time is up by `by`, or all of them.
fn end_cuts(ctx: &mut SystemCtx<'_>, e: Entity, by: Option<f32>, ids: Ids) -> Result<()> {
    let mut ends = Vec::new();
    for x in ctx.query().has(ids.cut).entities() {
        let c = ctx.get::<TimelineCut>(x)?;
        if c.timeline != e {
            continue;
        }
        let due = match by {
            None => true,
            Some(time) => c.until >= 0.0 && c.until <= time,
        };
        if due {
            ends.push(x);
        }
    }
    ends.sort();
    for x in ends {
        close_cut(ctx, x, ids)?;
    }
    Ok(())
}

/// A cut ends: its camera is as active as it was before, and the cut goes.
fn close_cut(ctx: &mut SystemCtx<'_>, x: Entity, ids: Ids) -> Result<()> {
    let c = ctx.get::<TimelineCut>(x)?;
    if ctx.is_alive(c.camera) && ctx.has(c.camera, ids.camera) {
        ctx.update::<Camera, _>(c.camera, |k| k.active = c.was_active)?;
    }
    ctx.despawn(x)
}
