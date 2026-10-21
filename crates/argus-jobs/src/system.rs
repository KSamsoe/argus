//! `jobs.update` (sim stage): the job system's one system. Each tick, in
//! entity order: jobs posted since the last tick are opened (and idle
//! workers look at once); claims whose worker, site, or cargo went are
//! undone; a job only for a worker that outranks its current one takes it
//! off that; idle workers whose turn it is take the best job they can do;
//! and workers with jobs walk, work, and carry. Jobs and workers are read
//! column by column and written back the same way, so two hundred
//! colonists cost little more than their arithmetic.

use crate::why::{self, Kind, Verdict};
use crate::{Job, JobKind, Worker, LOOK_TICKS};
use argus_ecs::{Entity, GlobalTransform, Parent, Result, StrId, Transform, Value, Vec2, World};
use argus_nav::search::NO_ISLAND;
use argus_nav::{Grid, NavAgent, NavStore};
use argus_sim::{Scheduler, Stage, SystemDesc};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// The system's name.
pub const UPDATE: &str = "jobs.update";

/// A job's stages, interned once.
struct Stages {
    open: StrId,
    going: StrId,
    working: StrId,
    carrying: StrId,
    arrived: StrId,
    blocked: StrId,
}

/// Where a worker can get to: its grid, the grid's islands, and the island
/// it stands on. A worker without a `NavAgent` does its jobs where it
/// stands, so everything is in its reach.
pub(crate) struct Reach {
    walks: bool,
    pos: Option<[f32; 2]>,
    grid: Option<Rc<Grid>>,
    islands: Option<Rc<Vec<u32>>>,
    island: u32,
}

/// The eight cells around a cell, in the fixed order (E, W, N, S, then the
/// diagonals) the navigation breaks ties by.
const AROUND: [(i64, i64); 8] = [
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (-1, 1),
    (1, -1),
    (-1, -1),
];

impl Reach {
    pub(crate) fn of(
        world: &World,
        nav: &mut NavStore,
        pos: Option<[f32; 2]>,
        agent_grid: Option<Entity>,
    ) -> Result<Reach> {
        let Some(explicit) = agent_grid else {
            return Ok(Reach {
                walks: false,
                pos,
                grid: None,
                islands: None,
                island: NO_ISLAND,
            });
        };
        let mut r = Reach {
            walks: true,
            pos,
            grid: None,
            islands: None,
            island: NO_ISLAND,
        };
        let Some(p) = pos else {
            return Ok(r);
        };
        let g = match nav.grid_for(world, p, explicit, false) {
            Ok(Some((_, g))) => g,
            Ok(None) => return Ok(r),
            Err(e) if e.code == argus_nav::codes::NO_GRID => return Ok(r),
            Err(e) => return Err(e),
        };
        let islands = nav.islands(&g);
        if let Some(i) = g.locate(p) {
            r.island = islands[i as usize];
            // Standing in a blocked cell: the island it would walk out to.
            if r.island == NO_ISLAND {
                let (c, row) = g.col_row(i);
                for (dc, dr) in AROUND {
                    if let Some(j) = g.index(c + dc, row + dr) {
                        if islands[j as usize] != NO_ISLAND {
                            r.island = islands[j as usize];
                            break;
                        }
                    }
                }
            }
        }
        r.grid = Some(g);
        r.islands = Some(islands);
        Ok(r)
    }

    /// Where to stand to work at `site`: the site itself when its cell is
    /// open and in reach, else the first open cell around it in reach; none
    /// when it is out of reach.
    pub(crate) fn spot(&self, site: [f32; 2]) -> Option<[f32; 2]> {
        if !self.walks {
            return Some(site);
        }
        let (g, islands) = (self.grid.as_ref()?, self.islands.as_ref()?);
        if self.island == NO_ISLAND {
            return None;
        }
        let i = g.locate(site)?;
        if islands[i as usize] == self.island {
            return Some(site);
        }
        let (c, r) = g.col_row(i);
        for (dc, dr) in AROUND {
            if let Some(j) = g.index(c + dc, r + dr) {
                if islands[j as usize] == self.island {
                    return Some(g.centre(j));
                }
            }
        }
        None
    }

    fn distance(&self, to: [f32; 2]) -> f32 {
        self.pos.map_or(0.0, |p| dist(p, to))
    }
}

/// Where to stand to work at `site` for a worker at `from` walking on
/// `grid` (none: the grid under it), or none when it cannot get there.
pub fn spot_near(
    world: &World,
    nav: &mut NavStore,
    from: [f32; 2],
    grid: Entity,
    site: [f32; 2],
) -> Result<Option<[f32; 2]>> {
    Ok(Reach::of(world, nav, Some(from), Some(grid))?.spot(site))
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    let (dx, dy) = (a[0] - b[0], a[1] - b[1]);
    (dx * dx + dy * dy).sqrt()
}

/// The entity a job is worked at: its site, else itself.
fn site_of(e: Entity, job: &Job) -> Entity {
    if job.site.is_null() {
        e
    } else {
        job.site
    }
}

/// What a worker makes of a job, and how far it would walk to it.
pub(crate) fn judge(
    world: &World,
    reach: &Reach,
    kinds: &HashMap<StrId, Kind>,
    we: Entity,
    w: &Worker,
    je: Entity,
    job: &Job,
) -> (Verdict, Option<f32>) {
    let site = argus_nav::position(world, site_of(je, job));
    verdict(world, reach, kinds, we, w, job, site)
}

fn verdict(
    world: &World,
    reach: &Reach,
    kinds: &HashMap<StrId, Kind>,
    we: Entity,
    w: &Worker,
    job: &Job,
    site: Option<[f32; 2]>,
) -> (Verdict, Option<f32>) {
    if job.worker == we {
        return (Verdict::Has, None);
    }
    if !job.worker.is_null() {
        return (Verdict::Taken(job.worker), None);
    }
    if !job.only.is_null() && job.only != we {
        return (Verdict::Only(job.only), None);
    }
    let need = why::requires(job, kinds);
    if !why::has_skill(world, w.skills, need) {
        return (Verdict::Skill(need), None);
    }
    let Some(site) = site else {
        return (Verdict::Nowhere, None);
    };
    match reach.spot(site) {
        Some(spot) => (Verdict::Could, Some(reach.distance(spot))),
        None => (Verdict::OutOfReach, None),
    }
}

/// One worker as the tick sees it.
struct WorkerRow {
    e: Entity,
    w: Worker,
    pos: Option<[f32; 2]>,
    agent: Option<NavAgent>,
    dirty: bool,
    agent_dirty: bool,
    /// Its agent was pointed somewhere new this tick, so what the agent
    /// says (arrived, blocked) is about the old goal until it walks.
    fresh: bool,
}

/// One job as the tick sees it.
struct JobRow {
    e: Entity,
    job: Job,
    dirty: bool,
    gone: bool,
}

pub(crate) fn install(
    world: &mut World,
    sched: &mut Scheduler,
    nav: Rc<RefCell<NavStore>>,
) -> Result<()> {
    let job = world.component_id_of::<Job>().expect("registered");
    let worker = world.component_id_of::<Worker>().expect("registered");
    let mut desc = SystemDesc::new(UPDATE, Stage::Sim)
        .writes(job)
        .writes(worker)
        .source("argus-jobs:update")
        .doc("The job system (section 12): opens jobs posted since the last tick, gives each idle Worker the best open Job it can do (highest priority, its JobKind's included, then nearest; never one it lacks the skill for or cannot reach), walks it there with its NavAgent, counts the work, carries carry to to, and when a job is finished emits job.done and despawns it. Emits job.claimed, job.started, job.carrying, job.released (with why), job.cancelled, and job.idle (with why) whenever a worker's reason for having no job changes.");
    for id in [
        world.component_id_of::<JobKind>(),
        world.component_id_of::<argus_nav::NavGrid>(),
        world.component_id_of::<argus_nav::NavCost>(),
        world.component_id_of::<Parent>(),
        world.component_id_of::<GlobalTransform>(),
        world.component_id("Tilemap"),
    ]
    .into_iter()
    .flatten()
    {
        desc = desc.reads(id);
    }
    // It points workers' agents at their jobs, and keeps what they carry
    // at their side.
    for id in [
        world.component_id_of::<NavAgent>(),
        world.component_id_of::<Transform>(),
    ]
    .into_iter()
    .flatten()
    {
        desc = desc.reads(id).writes(id);
    }
    let s = world.strings();
    let st = Stages {
        open: s.intern("open"),
        going: s.intern("going"),
        working: s.intern("working"),
        carrying: s.intern("carrying"),
        arrived: s.intern("arrived"),
        blocked: s.intern("blocked"),
    };
    sched.add_system(
        desc,
        Box::new(move |ctx| {
            let (Some(jid), Some(wid)) = (
                ctx.world().component_id_of::<Job>(),
                ctx.world().component_id_of::<Worker>(),
            ) else {
                return Ok(());
            };
            // A scene without jobs or workers pays nothing, not even a query.
            let any = |c| {
                ctx.world()
                    .archetypes()
                    .iter()
                    .any(|a| !a.is_empty() && a.has(c))
            };
            if !any(jid) && !any(wid) {
                return Ok(());
            }
            // 1. Read: jobs and workers, in the views' order.
            let mut jobs: Vec<JobRow> = Vec::new();
            ctx.query().has(jid).for_each_view(|v| {
                let j = v.column::<Job>()?;
                for (k, e) in v.entities().iter().enumerate() {
                    jobs.push(JobRow {
                        e: *e,
                        job: j[k],
                        dirty: false,
                        gone: false,
                    });
                }
                Ok(())
            })?;
            let mut workers: Vec<WorkerRow> = Vec::new();
            ctx.query().has(wid).for_each_view(|v| {
                let w = v.column::<Worker>()?;
                let agents = v.column::<NavAgent>().ok();
                let places = v.column::<Transform>().ok();
                let child = v.column::<Parent>().is_ok();
                let globals = v.column::<GlobalTransform>().ok();
                for (k, e) in v.entities().iter().enumerate() {
                    let pos = match (child, &globals, &places) {
                        (true, Some(g), _) => Some([g[k].pos.x, g[k].pos.y]),
                        (_, _, Some(t)) => Some([t[k].pos.x, t[k].pos.y]),
                        _ => None,
                    };
                    workers.push(WorkerRow {
                        e: *e,
                        w: w[k],
                        pos,
                        agent: agents.as_ref().map(|a| a[k]),
                        dirty: false,
                        agent_dirty: false,
                        fresh: false,
                    });
                }
                Ok(())
            })?;
            if jobs.is_empty() && workers.is_empty() {
                return Ok(());
            }
            let mut jorder: Vec<usize> = (0..jobs.len()).collect();
            jorder.sort_by_key(|&i| jobs[i].e);
            let mut worder: Vec<usize> = (0..workers.len()).collect();
            worder.sort_by_key(|&i| workers[i].e);
            let jat: HashMap<Entity, usize> =
                jobs.iter().enumerate().map(|(i, j)| (j.e, i)).collect();
            let wat: HashMap<Entity, usize> =
                workers.iter().enumerate().map(|(i, w)| (w.e, i)).collect();
            let dt = ctx.dt as f32;
            let mut events: Vec<(&'static str, Vec<(String, Value)>)> = Vec::new();
            let mut carried: Vec<(Entity, [f32; 2])> = Vec::new();
            {
                let world = ctx.world();
                let kinds = why::kinds(world);
                let mut nav = nav.borrow_mut();
                let name = |k: StrId| world.strings().resolve(k).to_string();
                let ev = |kind: &'static str, fields: Vec<(&str, Value)>| {
                    (
                        kind,
                        fields
                            .into_iter()
                            .map(|(k, v)| (k.to_string(), v))
                            .collect::<Vec<_>>(),
                    )
                };

                // 2. Jobs posted since the last tick open, and wake the idle.
                let mut posted = false;
                for &i in &jorder {
                    let j = &mut jobs[i];
                    if j.job.stage == StrId::EMPTY {
                        j.job.stage = st.open;
                        j.job.worker = Entity::NULL;
                        j.dirty = true;
                        posted = true;
                    }
                }

                // 3. Claims that no longer hold.
                for &i in &jorder {
                    let (e, job) = (jobs[i].e, jobs[i].job);
                    let lost = [
                        (job.site, "its site is gone"),
                        (job.to, "where it goes is gone"),
                        (job.carry, "what it carries is gone"),
                        (job.only, "the one it was for is gone"),
                    ]
                    .into_iter()
                    .find(|(x, _)| !x.is_null() && !world.is_alive(*x));
                    if let Some((_, why)) = lost {
                        jobs[i].gone = true;
                        events.push(ev(
                            "job.cancelled",
                            vec![
                                ("job", Value::Entity(e)),
                                ("kind", Value::Str(name(job.kind))),
                                ("worker", Value::Entity(job.worker)),
                                ("why", Value::Str(why.to_string())),
                            ],
                        ));
                        continue;
                    }
                    if job.worker.is_null() {
                        continue;
                    }
                    let holds = wat.get(&job.worker).is_some_and(|&k| workers[k].w.job == e);
                    if !holds {
                        let why = if wat.contains_key(&job.worker) {
                            "its worker has another job"
                        } else {
                            "its worker is gone"
                        };
                        release(&mut jobs[i], &st);
                        events.push(ev(
                            "job.released",
                            vec![
                                ("job", Value::Entity(e)),
                                ("kind", Value::Str(name(job.kind))),
                                ("worker", Value::Entity(job.worker)),
                                ("why", Value::Str(why.to_string())),
                            ],
                        ));
                        posted = true;
                    }
                }
                for &k in &worder {
                    let wr = &mut workers[k];
                    if wr.w.job.is_null() {
                        continue;
                    }
                    let mine = jat
                        .get(&wr.w.job)
                        .is_some_and(|&i| !jobs[i].gone && jobs[i].job.worker == wr.e);
                    if !mine {
                        free(wr);
                    }
                }

                // 4. A job only for a worker that outranks the one it has.
                let mut mine: HashMap<Entity, f32> = HashMap::new();
                for &i in &jorder {
                    let j = &jobs[i];
                    if !j.gone && j.job.worker.is_null() && !j.job.only.is_null() {
                        let p = why::priority(&j.job, &kinds);
                        let best = mine.entry(j.job.only).or_insert(f32::NEG_INFINITY);
                        *best = best.max(p);
                    }
                }
                for &k in &worder {
                    let (we, cur) = (workers[k].e, workers[k].w.job);
                    let (Some(&top), Some(&i)) = (mine.get(&we), jat.get(&cur)) else {
                        continue;
                    };
                    if top > why::priority(&jobs[i].job, &kinds) {
                        let job = jobs[i].job;
                        release(&mut jobs[i], &st);
                        free(&mut workers[k]);
                        events.push(ev(
                            "job.released",
                            vec![
                                ("job", Value::Entity(jobs[i].e)),
                                ("kind", Value::Str(name(job.kind))),
                                ("worker", Value::Entity(we)),
                                (
                                    "why",
                                    Value::Str("a job only for it outranks it".to_string()),
                                ),
                            ],
                        ));
                    }
                }

                // 5. Idle workers whose turn it is take the best they can do.
                let mut open: Vec<usize> = jorder
                    .iter()
                    .copied()
                    .filter(|&i| !jobs[i].gone && jobs[i].job.worker.is_null())
                    .collect();
                let taken = jorder
                    .iter()
                    .filter(|&&i| !jobs[i].gone && !jobs[i].job.worker.is_null())
                    .count();
                open.sort_by(|&a, &b| {
                    why::priority(&jobs[b].job, &kinds)
                        .partial_cmp(&why::priority(&jobs[a].job, &kinds))
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then(jobs[a].e.cmp(&jobs[b].e))
                });
                let mut sites: HashMap<Entity, Option<[f32; 2]>> = HashMap::new();
                for &k in &worder {
                    if !workers[k].w.job.is_null() {
                        continue;
                    }
                    if workers[k].w.look > 0 && !posted {
                        workers[k].w.look -= 1;
                        workers[k].dirty = true;
                        continue;
                    }
                    let wr = &workers[k];
                    let reach = Reach::of(world, &mut nav, wr.pos, wr.agent.map(|a| a.grid))?;
                    let mut best: Option<(usize, f32, [f32; 2], f32)> = None;
                    let mut verdicts: Vec<Verdict> = Vec::new();
                    for &i in &open {
                        let job = &jobs[i].job;
                        if !job.worker.is_null() {
                            // Taken earlier this tick.
                            verdicts.push(Verdict::Taken(job.worker));
                            continue;
                        }
                        let p = why::priority(job, &kinds);
                        if let Some((_, bp, _, _)) = best {
                            if p < bp {
                                break;
                            }
                        }
                        let s = *sites
                            .entry(site_of(jobs[i].e, job))
                            .or_insert_with_key(|&x| argus_nav::position(world, x));
                        let (v, _) = verdict(world, &reach, &kinds, wr.e, &wr.w, job, s);
                        if v == Verdict::Could {
                            let spot = reach.spot(s.expect("could")).expect("could");
                            let d = reach.distance(spot);
                            if best.is_none_or(|(_, _, _, bd)| d < bd) {
                                best = Some((i, p, spot, d));
                            }
                        }
                        verdicts.push(v);
                    }
                    let wr = &mut workers[k];
                    match best {
                        Some((i, _, spot, _)) => {
                            let j = &mut jobs[i];
                            j.job.worker = wr.e;
                            j.job.stage = st.going;
                            j.job.spot = Vec2::new(spot[0], spot[1]);
                            j.dirty = true;
                            wr.w.job = j.e;
                            wr.w.idle = StrId::EMPTY;
                            wr.w.look = 0;
                            wr.dirty = true;
                            go(wr, spot);
                            events.push(ev(
                                "job.claimed",
                                vec![
                                    ("job", Value::Entity(j.e)),
                                    ("kind", Value::Str(name(j.job.kind))),
                                    ("worker", Value::Entity(wr.e)),
                                ],
                            ));
                        }
                        None => {
                            verdicts
                                .extend(std::iter::repeat_n(Verdict::Taken(Entity::NULL), taken));
                            let text = why::summary(world, &verdicts);
                            let id = world.strings().intern(&text);
                            if wr.w.idle != id {
                                wr.w.idle = id;
                                events.push(ev(
                                    "job.idle",
                                    vec![
                                        ("worker", Value::Entity(wr.e)),
                                        ("why", Value::Str(text)),
                                    ],
                                ));
                            }
                            wr.w.look = LOOK_TICKS;
                            wr.dirty = true;
                        }
                    }
                }
                // 6. Workers with jobs walk, work, and carry.
                for &k in &worder {
                    let cur = workers[k].w.job;
                    let Some(&i) = jat.get(&cur) else {
                        continue;
                    };
                    if jobs[i].gone {
                        continue;
                    }
                    let wr = &mut workers[k];
                    let j = &mut jobs[i];
                    let spot = [j.job.spot.x, j.job.spot.y];
                    let at = |wr: &WorkerRow| -> (bool, bool) {
                        match wr.agent {
                            None => (true, false),
                            Some(a) => {
                                let goal_here = a.goal == Vec2::new(spot[0], spot[1]);
                                let there = goal_here
                                    && a.state == st.arrived
                                    && wr.pos.is_some_and(|p| dist(p, spot) <= a.arrive + 1e-3);
                                (there, goal_here && a.state == st.blocked)
                            }
                        }
                    };
                    let stage = j.job.stage;
                    if (stage == st.going || stage == st.carrying) && wr.fresh {
                        continue;
                    }
                    if stage == st.going || stage == st.carrying {
                        if stage == st.carrying && !j.job.carry.is_null() {
                            if let Some(p) = wr.pos {
                                carried.push((j.job.carry, p));
                            }
                        }
                        let (there, blocked) = at(wr);
                        if blocked {
                            let job = j.job;
                            release(j, &st);
                            free(wr);
                            wr.w.look = LOOK_TICKS;
                            events.push(ev(
                                "job.released",
                                vec![
                                    ("job", Value::Entity(j.e)),
                                    ("kind", Value::Str(name(job.kind))),
                                    ("worker", Value::Entity(wr.e)),
                                    ("why", Value::Str("its way there is blocked".to_string())),
                                ],
                            ));
                            continue;
                        }
                        if !there {
                            continue;
                        }
                        if stage == st.going {
                            j.job.stage = st.working;
                            j.dirty = true;
                            events.push(ev(
                                "job.started",
                                vec![
                                    ("job", Value::Entity(j.e)),
                                    ("kind", Value::Str(name(j.job.kind))),
                                    ("worker", Value::Entity(wr.e)),
                                ],
                            ));
                            // Work begins the tick after it gets there.
                            continue;
                        }
                        // Carried there: done.
                        done(j, wr, &mut events, &name);
                        continue;
                    }
                    if stage == st.working {
                        let need = why::work(&j.job, &kinds);
                        j.job.progress += dt * wr.w.rate.max(0.0);
                        j.dirty = true;
                        if j.job.progress + 1e-6 < need {
                            continue;
                        }
                        j.job.progress = j.job.progress.max(need);
                        if j.job.to.is_null() {
                            done(j, wr, &mut events, &name);
                            continue;
                        }
                        // The work is done; now the second leg.
                        let to = argus_nav::position(world, j.job.to);
                        let reach = Reach::of(world, &mut nav, wr.pos, wr.agent.map(|a| a.grid))?;
                        match to.and_then(|p| reach.spot(p)) {
                            Some(s) => {
                                j.job.stage = st.carrying;
                                j.job.spot = Vec2::new(s[0], s[1]);
                                go(wr, s);
                                if !j.job.carry.is_null() {
                                    if let Some(p) = wr.pos {
                                        carried.push((j.job.carry, p));
                                    }
                                }
                                events.push(ev(
                                    "job.carrying",
                                    vec![
                                        ("job", Value::Entity(j.e)),
                                        ("kind", Value::Str(name(j.job.kind))),
                                        ("worker", Value::Entity(wr.e)),
                                        ("carry", Value::Entity(j.job.carry)),
                                    ],
                                ));
                            }
                            None => {
                                let job = j.job;
                                release(j, &st);
                                free(wr);
                                wr.w.look = LOOK_TICKS;
                                events.push(ev(
                                    "job.released",
                                    vec![
                                        ("job", Value::Entity(j.e)),
                                        ("kind", Value::Str(name(job.kind))),
                                        ("worker", Value::Entity(wr.e)),
                                        (
                                            "why",
                                            Value::Str("where it goes is out of reach".to_string()),
                                        ),
                                    ],
                                ));
                            }
                        }
                    }
                }
            }

            // 7. Write back, in the views' order; then what moved, what went.
            if jobs.iter().any(|j| j.dirty) {
                let mut k0 = 0usize;
                ctx.query().has(jid).for_each_view(|v| {
                    let n = v.entities().len();
                    if jobs[k0..k0 + n].iter().any(|j| j.dirty) {
                        let mut col = v.column_mut::<Job>()?;
                        for k in 0..n {
                            if jobs[k0 + k].dirty {
                                col[k] = jobs[k0 + k].job;
                            }
                        }
                    }
                    k0 += n;
                    Ok(())
                })?;
            }
            if workers.iter().any(|w| w.dirty || w.agent_dirty) {
                let mut k0 = 0usize;
                ctx.query().has(wid).for_each_view(|v| {
                    let n = v.entities().len();
                    let rows = &workers[k0..k0 + n];
                    if rows.iter().any(|w| w.dirty) {
                        let mut col = v.column_mut::<Worker>()?;
                        for k in 0..n {
                            if rows[k].dirty {
                                col[k] = rows[k].w;
                            }
                        }
                    }
                    if rows.iter().any(|w| w.agent_dirty) {
                        let mut col = v.column_mut::<NavAgent>()?;
                        for k in 0..n {
                            if let (true, Some(a)) = (rows[k].agent_dirty, rows[k].agent) {
                                col[k] = a;
                            }
                        }
                    }
                    k0 += n;
                    Ok(())
                })?;
            }
            for (e, p) in carried {
                if ctx.world().is_alive(e) {
                    let _ = ctx.update::<Transform, _>(e, |t| {
                        t.pos = Vec2::new(p[0], p[1]);
                    });
                }
            }
            let mut gone: Vec<Entity> = jobs.iter().filter(|j| j.gone).map(|j| j.e).collect();
            gone.sort();
            for e in gone {
                ctx.despawn(e)?;
            }
            for (name, fields) in events {
                ctx.emit(name, fields);
            }
            Ok(())
        }),
    )
}

/// Point a worker's agent at a spot (and at nothing else).
fn go(wr: &mut WorkerRow, spot: [f32; 2]) {
    if let Some(a) = wr.agent.as_mut() {
        let goal = Vec2::new(spot[0], spot[1]);
        if a.goal != goal || !a.target.is_null() || !a.field.is_null() {
            a.goal = goal;
            a.target = Entity::NULL;
            a.field = Entity::NULL;
            wr.agent_dirty = true;
            wr.fresh = true;
        }
    }
}

/// A worker without its job: it stops where it is and looks at once.
fn free(wr: &mut WorkerRow) {
    wr.w.job = Entity::NULL;
    wr.w.look = 0;
    wr.dirty = true;
    if let Some(p) = wr.pos {
        go(wr, p);
    }
}

/// A job back to open. One whose work was done and was being carried now
/// starts where what it carries was left.
fn release(j: &mut JobRow, st: &Stages) {
    if j.job.stage == st.carrying && !j.job.carry.is_null() {
        j.job.site = j.job.carry;
    }
    j.job.worker = Entity::NULL;
    j.job.stage = st.open;
    j.dirty = true;
}

/// A job finished: `job.done` with everything a game needs to act on it,
/// the job to be despawned, the worker free.
fn done(
    j: &mut JobRow,
    wr: &mut WorkerRow,
    events: &mut Vec<(&'static str, Vec<(String, Value)>)>,
    name: &impl Fn(StrId) -> String,
) {
    j.gone = true;
    events.push((
        "job.done",
        vec![
            ("job".to_string(), Value::Entity(j.e)),
            ("kind".to_string(), Value::Str(name(j.job.kind))),
            ("worker".to_string(), Value::Entity(wr.e)),
            ("site".to_string(), Value::Entity(j.job.site)),
            ("to".to_string(), Value::Entity(j.job.to)),
            ("carry".to_string(), Value::Entity(j.job.carry)),
        ],
    ));
    free(wr);
}
