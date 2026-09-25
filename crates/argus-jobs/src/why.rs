//! What a worker makes of a job, and why it has none (§12: "obs.why can
//! explain 'why is this colonist idle' from the job system's declared
//! events"). The system decides with these, and `jobs.why` answers with
//! them, so the explanation is the decision.

use crate::{Job, JobKind, Worker};
use argus_ecs::{Entity, StrId, World};
use std::collections::{BTreeMap, HashMap};

/// A kind's weights, looked up once a tick.
#[derive(Clone, Copy, Default)]
pub struct Kind {
    /// Added to a job's own priority.
    pub priority: f32,
    /// The skill its jobs need when they name none.
    pub skill: StrId,
    /// Seconds of work of its jobs when they name none.
    pub work: f32,
}

/// Every `JobKind` by name (the first entity of a name wins).
pub fn kinds(world: &World) -> HashMap<StrId, Kind> {
    let mut out = HashMap::new();
    let Some(id) = world.component_id_of::<JobKind>() else {
        return out;
    };
    let mut all: Vec<(Entity, JobKind)> = Vec::new();
    let _ = world.query().has(id).for_each_view(|v| {
        let k = v.column::<JobKind>()?;
        for (i, e) in v.entities().iter().enumerate() {
            all.push((*e, k[i]));
        }
        Ok(())
    });
    all.sort_by_key(|a| a.0);
    for (_, k) in all {
        out.entry(k.name).or_insert(Kind {
            priority: k.priority,
            skill: k.skill,
            work: k.work,
        });
    }
    out
}

/// A job's priority: its own and its kind's.
pub fn priority(job: &Job, kinds: &HashMap<StrId, Kind>) -> f32 {
    job.priority + kinds.get(&job.kind).map_or(0.0, |k| k.priority)
}

/// The skill a job needs: its own, else its kind's (empty: anyone).
pub fn requires(job: &Job, kinds: &HashMap<StrId, Kind>) -> StrId {
    if job.requires != StrId::EMPTY {
        job.requires
    } else {
        kinds.get(&job.kind).map_or(StrId::EMPTY, |k| k.skill)
    }
}

/// Seconds of work a job takes: its own, else its kind's.
pub fn work(job: &Job, kinds: &HashMap<StrId, Kind>) -> f32 {
    if job.work > 0.0 {
        job.work
    } else {
        kinds.get(&job.kind).map_or(0.0, |k| k.work)
    }
}

/// Whether a worker's skills (words, or `*`) include `skill`.
pub fn has_skill(world: &World, skills: StrId, skill: StrId) -> bool {
    if skill == StrId::EMPTY || skills == skill {
        return true;
    }
    let s = world.strings();
    let have = s.resolve(skills);
    let want = s.resolve(skill);
    have.split_whitespace().any(|w| w == "*" || w == want)
}

/// What a worker makes of one job.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Verdict {
    /// It would take it (the best of these, by priority and distance).
    Could,
    /// It has it.
    Has,
    /// Another worker has it.
    Taken(Entity),
    /// It is only for another worker.
    Only(Entity),
    /// It needs a skill the worker lacks.
    Skill(StrId),
    /// Its site is somewhere the worker cannot walk to.
    OutOfReach,
    /// Its site has no place (no Transform).
    Nowhere,
}

/// An entity as the protocol spells it: `@id` when authored, else `e:n`.
fn spelled(world: &World, e: Entity) -> String {
    match world.string_id(e) {
        Some(s) => format!("@{s}"),
        None => e.to_string(),
    }
}

/// A verdict as words.
pub fn verdict_text(world: &World, v: Verdict) -> String {
    let s = world.strings();
    match v {
        Verdict::Could => "could take it".to_string(),
        Verdict::Has => "has it".to_string(),
        Verdict::Taken(w) => format!("{} has it", spelled(world, w)),
        Verdict::Only(w) => format!("only for {}", spelled(world, w)),
        Verdict::Skill(k) => format!("needs {}", s.resolve(k)),
        Verdict::OutOfReach => "out of reach".to_string(),
        Verdict::Nowhere => "its site has no place".to_string(),
    }
}

/// Why a worker took none of the jobs, from what it made of each: the
/// `Worker.idle` and `job.idle` text.
pub fn summary(world: &World, verdicts: &[Verdict]) -> String {
    if verdicts.is_empty() {
        return "no jobs are posted".to_string();
    }
    let mut taken = 0usize;
    let mut only = 0usize;
    let mut reach = 0usize;
    let mut nowhere = 0usize;
    let mut skills: BTreeMap<String, usize> = BTreeMap::new();
    for v in verdicts {
        match v {
            Verdict::Taken(_) => taken += 1,
            Verdict::Only(_) => only += 1,
            Verdict::OutOfReach => reach += 1,
            Verdict::Nowhere => nowhere += 1,
            Verdict::Skill(k) => {
                *skills
                    .entry(world.strings().resolve(*k).to_string())
                    .or_default() += 1
            }
            Verdict::Could | Verdict::Has => {}
        }
    }
    let open = verdicts.len() - taken;
    if open == 0 {
        return if taken == 1 {
            "the one job is taken".to_string()
        } else {
            format!("all {taken} jobs are taken")
        };
    }
    let mut parts: Vec<String> = Vec::new();
    for (k, n) in &skills {
        parts.push(format!("{n} need {k}"));
    }
    if only > 0 {
        parts.push(format!(
            "{only} {} someone else's",
            if only == 1 { "is" } else { "are" }
        ));
    }
    if reach > 0 {
        parts.push(format!("{reach} out of reach"));
    }
    if nowhere > 0 {
        parts.push(format!("{nowhere} with no place"));
    }
    let mut out = if open == 1 {
        format!("the one open job is not its to do: {}", parts.join(", "))
    } else {
        format!(
            "none of the {open} open jobs is its to do: {}",
            parts.join(", ")
        )
    };
    if taken > 0 {
        out.push_str(&format!("; {taken} more taken"));
    }
    out
}

/// One row of `jobs.why`: a job and what the worker makes of it.
#[derive(Clone, Debug)]
pub struct Row {
    /// The job.
    pub job: Entity,
    /// Its kind.
    pub kind: String,
    /// Its priority, its kind's included.
    pub priority: f32,
    /// Its stage.
    pub stage: String,
    /// From the worker to where it would stand, in world units.
    pub distance: Option<f32>,
    /// What the worker makes of it.
    pub verdict: String,
}

/// Every job and what `worker` makes of it, in the order it would weigh
/// them (priority, then distance), and why it has none when it has none:
/// `jobs.why` (the same rules the system decides by).
pub fn explain(
    world: &World,
    nav: &mut argus_nav::NavStore,
    worker: Entity,
) -> argus_ecs::Result<(Worker, Vec<Row>, String)> {
    let w = world.get::<Worker>(worker).map_err(|_| {
        argus_ecs::ArgusError::new(
            crate::codes::NOT_A_WORKER,
            format!("{worker} is not a Worker"),
        )
        .hint("world.query {where: {has: [\"Worker\"]}} lists the workers.")
    })?;
    let kinds = kinds(world);
    let pos = argus_nav::position(world, worker);
    let agent = world.get::<argus_nav::NavAgent>(worker).ok();
    let reach = crate::system::Reach::of(world, nav, pos, agent.map(|a| a.grid))?;
    let Some(id) = world.component_id_of::<Job>() else {
        return Ok((w, Vec::new(), "no jobs are posted".to_string()));
    };
    let mut jobs: Vec<(Entity, Job)> = Vec::new();
    world.query().has(id).for_each_view(|v| {
        let j = v.column::<Job>()?;
        for (i, e) in v.entities().iter().enumerate() {
            jobs.push((*e, j[i]));
        }
        Ok(())
    })?;
    jobs.sort_by_key(|j| j.0);
    let mut rows: Vec<(Row, f32, Verdict)> = Vec::new();
    for (e, job) in jobs {
        let (v, d) = crate::system::judge(world, &reach, &kinds, worker, &w, e, &job);
        let s = world.strings();
        rows.push((
            Row {
                job: e,
                kind: s.resolve(job.kind).to_string(),
                priority: priority(&job, &kinds),
                stage: s.resolve(job.stage).to_string(),
                distance: d,
                verdict: verdict_text(world, v),
            },
            d.unwrap_or(f32::INFINITY),
            v,
        ));
    }
    rows.sort_by(|a, b| {
        b.0.priority
            .partial_cmp(&a.0.priority)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .then(a.0.job.cmp(&b.0.job))
    });
    let why = if w.job.is_null() {
        let vs: Vec<Verdict> = rows.iter().map(|r| r.2).collect();
        if vs.contains(&Verdict::Could) {
            "it would take one when it next looks".to_string()
        } else {
            summary(world, &vs)
        }
    } else {
        String::new()
    };
    Ok((w, rows.into_iter().map(|r| r.0).collect(), why))
}
