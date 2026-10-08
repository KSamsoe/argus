//! Checking a plugin against the game and installing its systems.
//!
//! What can be checked before a system runs is checked here, so it is an
//! error at load rather than a fault later (§7): every component layout the
//! plugin reads columns as is the game's, field for field; every system's
//! stage and components exist; no system takes a name another system (a
//! Luau one, another plugin's) has. A plugin with any of these wrong
//! installs nothing.

use crate::host::{describe, fingerprint, Call, Command};
use crate::load::{Plugin, SystemSpec};
use crate::{codes, PluginError};
use argus_ecs::{ArgusError, ComponentId, Result, World};
use argus_plugin::abi;
use argus_sim::{ClockBinding, ParCtx, Scheduler, Stage, SystemCtx, SystemDesc};
use std::sync::Arc;

fn at(plugin: &str, source: &str, code: u32, message: String) -> PluginError {
    let (file, line) = match source.rsplit_once(':') {
        Some((f, l)) if l.parse::<u32>().is_ok() => (f.to_string(), l.parse().ok()),
        _ => (source.to_string(), None),
    };
    PluginError {
        plugin: plugin.to_string(),
        phase: "install",
        file,
        line,
        message,
        code,
    }
}

/// Everything wrong with installing `p` into this world, with `prev` the
/// version of it installed now (its systems may be replaced).
pub fn check(
    world: &World,
    sched: &Scheduler,
    p: &Plugin,
    prev: Option<&Plugin>,
) -> Vec<PluginError> {
    let mut out = Vec::new();
    let reg = world.registry();
    for l in &p.layouts {
        let Some(c) = reg.id(&l.name) else {
            out.push(at(
                &p.name,
                &l.source,
                codes::LAYOUT,
                format!(
                    "the plugin reads {}, which the game does not declare",
                    l.name
                ),
            ));
            continue;
        };
        let s = reg.schema(c);
        let theirs = abi::fingerprint(
            l.size,
            l.align,
            l.fields.iter().map(|(n, t, o)| (n.as_str(), *t, *o)),
        );
        if theirs != fingerprint(s) {
            let mine: Vec<String> = l
                .fields
                .iter()
                .map(|(n, t, o)| format!("{n} {} at {o}", abi::ty::name(*t)))
                .collect();
            out.push(at(
                &p.name,
                &l.source,
                codes::LAYOUT,
                format!(
                    "the plugin's {} is not the game's: the game's is {}; the plugin's is {} bytes: {}",
                    l.name,
                    describe(s),
                    l.size,
                    mine.join(", ")
                ),
            ));
        }
    }
    let own = |name: &str| prev.is_some_and(|q| q.systems.iter().any(|s| s.name == name));
    let mut seen: Vec<&str> = Vec::new();
    for s in &p.systems {
        if seen.contains(&s.name.as_str()) {
            out.push(at(
                &p.name,
                &s.source,
                codes::DECLARATION,
                format!("the plugin declares {} twice", s.name),
            ));
        }
        seen.push(&s.name);
        if Stage::parse(&s.stage).is_none() {
            out.push(at(
                &p.name,
                &s.source,
                codes::DECLARATION,
                format!(
                    "{}'s stage {:?} is not one of input, sim, late, hierarchy, present",
                    s.name, s.stage
                ),
            ));
        }
        for (what, list) in [
            ("reads", &s.reads),
            ("writes", &s.writes),
            ("spawns", &s.spawns),
        ] {
            for c in list {
                if reg.id(c).is_none() {
                    out.push(at(
                        &p.name,
                        &s.source,
                        codes::DECLARATION,
                        format!("{} {what} {c}, which the game does not declare", s.name),
                    ));
                }
            }
        }
        if let Some(src) = sched.source_of(&s.name) {
            if !own(&s.name) {
                out.push(at(
                    &p.name,
                    &s.source,
                    codes::DECLARATION,
                    format!(
                        "{} is declared twice: here and at {}; remove one (a system moved to Rust leaves Luau)",
                        s.name,
                        src.unwrap_or("the engine")
                    ),
                ));
            }
        }
    }
    out
}

/// Install `p`'s systems into the scheduler (replacing `prev`'s, which
/// keeps their places in the run order), after [`check`]. Returns what
/// is wrong, having installed nothing, when anything is.
pub fn install(
    world: &mut World,
    sched: &mut Scheduler,
    p: &Arc<Plugin>,
    prev: Option<&Plugin>,
) -> Vec<PluginError> {
    let errs = check(world, sched, p, prev);
    if !errs.is_empty() {
        return errs;
    }
    let mut out = Vec::new();
    for (i, s) in p.systems.iter().enumerate() {
        let r = match desc(world, s) {
            Ok(d) if s.flags & abi::flags::PARALLEL != 0 => {
                sched.replace_parallel_system(d, parallel(p.clone(), i, ids(world, &s.spawns)))
            }
            Ok(d) => sched.replace_system(d, serial(p.clone(), i)),
            Err(e) => Err(e),
        };
        if let Err(e) = r {
            out.push(at(&p.name, &s.source, codes::DECLARATION, e.message));
        }
    }
    if let Some(q) = prev {
        for s in &q.systems {
            if !p.systems.iter().any(|n| n.name == s.name) {
                let _ = sched.remove_system(&s.name);
            }
        }
    }
    out
}

/// Take `p`'s systems out of the scheduler.
pub fn remove(sched: &mut Scheduler, p: &Plugin) {
    for s in &p.systems {
        let _ = sched.remove_system(&s.name);
    }
}

fn ids(world: &World, names: &[String]) -> Vec<ComponentId> {
    names.iter().filter_map(|n| world.component_id(n)).collect()
}

fn desc(world: &World, s: &SystemSpec) -> Result<SystemDesc> {
    let stage = Stage::parse(&s.stage).unwrap_or(Stage::Sim);
    let mut d = SystemDesc::new(&s.name, stage)
        .source(&s.source)
        .doc(&s.doc)
        .clock(match s.clock {
            abi::clock::CONTINUOUS => ClockBinding::Continuous,
            abi::clock::DISCRETE => ClockBinding::Discrete,
            _ => ClockBinding::Driving,
        });
    for a in &s.after {
        d = d.after(a);
    }
    for c in ids(world, &s.reads) {
        d = d.reads(c);
    }
    for c in ids(world, &s.writes) {
        d = d.writes(c);
    }
    let parallel = s.flags & abi::flags::PARALLEL != 0;
    for c in ids(world, &s.spawns) {
        // A serial system's spawns are writes (it changes the world at the
        // end of its own run); a parallel one's wait for its batch.
        if parallel {
            d = d.spawns(c);
        } else if !d.access.writes.contains(&c) {
            d = d.writes(c);
        }
    }
    if let Some(r) = &s.rng {
        d = d.rng(r);
    }
    if s.flags & abi::flags::ON_LOAD != 0 {
        d = d.on_load();
    }
    if s.flags & abi::flags::FIRST != 0 {
        d = d.first();
    }
    if s.flags & abi::flags::LAST != 0 {
        d = d.last();
    }
    if s.flags & abi::flags::UNFROZEN != 0 {
        d = d.unfrozen();
    }
    Ok(d)
}

/// A run that did not end well, at the system's declaration (§2: the
/// system is switched off and the fault reported as an event).
fn fault(s: &SystemSpec, message: String) -> ArgusError {
    ArgusError::new(codes::FAULT, message).at(Some(s.source.clone()))
}

fn serial(p: Arc<Plugin>, i: usize) -> argus_sim::SystemFn {
    Box::new(move |ctx: &mut SystemCtx<'_>| {
        let s = &p.systems[i];
        let t = (ctx.tick, ctx.time, ctx.dt, ctx.speed, ctx.frozen);
        let name = ctx.name.to_string();
        let (message, events, commands) = {
            let (world, access, rng) = ctx.split();
            let call = Call::new(world, access, &[], rng, &name);
            let status = call.run(s.run, s.user, t);
            let message = (status != abi::status::OK).then(|| call.message(status));
            (
                message,
                call.events.into_inner(),
                call.commands.into_inner(),
            )
        };
        for (n, f) in events {
            ctx.emit(n, f);
        }
        for c in commands {
            match c {
                Command::Spawn { comps, fields } => {
                    let e = ctx.spawn_with(&comps)?;
                    for (path, v) in fields {
                        ctx.set_field(e, &path, v)?;
                    }
                }
                Command::Despawn(e) => {
                    if ctx.is_alive(e) {
                        ctx.despawn(e)?;
                    }
                }
            }
        }
        match message {
            None => Ok(()),
            Some(m) => Err(fault(s, m)),
        }
    })
}

fn parallel(p: Arc<Plugin>, i: usize, spawns: Vec<ComponentId>) -> argus_sim::ParSystemFn {
    Box::new(move |ctx: &mut ParCtx<'_>| {
        let s = &p.systems[i];
        let t = (ctx.tick, ctx.time, ctx.dt, ctx.speed, ctx.frozen);
        let name = ctx.name.to_string();
        let (message, events, commands) = {
            let (world, access, rng) = ctx.split();
            let call = Call::new(world, access, &spawns, rng, &name);
            let status = call.run(s.run, s.user, t);
            let message = (status != abi::status::OK).then(|| call.message(status));
            (
                message,
                call.events.into_inner(),
                call.commands.into_inner(),
            )
        };
        for (n, f) in events {
            ctx.emit(n, f);
        }
        for c in commands {
            match c {
                Command::Spawn { comps, fields } => {
                    let f: Vec<(&str, argus_ecs::Value)> = fields
                        .iter()
                        .map(|(p, v)| (p.as_str(), v.clone()))
                        .collect();
                    ctx.spawn(&comps, f)?;
                }
                Command::Despawn(e) => {
                    if ctx.is_alive(e) {
                        ctx.despawn(e)?;
                    }
                }
            }
        }
        match message {
            None => Ok(()),
            Some(m) => Err(fault(s, m)),
        }
    })
}
