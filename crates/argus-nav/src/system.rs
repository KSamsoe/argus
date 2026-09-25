//! `nav.agents` (sim stage, last): every `NavAgent` walked a tick along its
//! path or down its field, in entity order.
//!
//! Three passes, so a crowd of ten thousand costs little more than its
//! arithmetic: the agents and their places are read column by column; each
//! is worked out in entity order, with the tick's grids and fields looked up
//! once; and what changed is written back column by column.

use crate::grid::{Grid, STRAIGHT};
use crate::search::{downhill, Field};
use crate::store::{position, waypoints, NavStore};
use crate::{NavAgent, NavCost, NavField, NavGrid};
use argus_ecs::{Entity, GlobalTransform, Parent, Result, StrId, Transform, Value, Vec2, World};
use argus_sim::{Scheduler, Stage, SystemDesc};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// The agents system's name.
pub const AGENTS: &str = "nav.agents";

/// The states an agent is in, interned once.
struct States {
    moving: StrId,
    arrived: StrId,
    blocked: StrId,
}

/// A field and the grid it is on.
type FieldOn = (Rc<Grid>, Rc<Field>);

/// What the tick has looked up already.
struct Tick<'s> {
    store: &'s mut NavStore,
    grids: Option<Vec<(Entity, Rc<Grid>)>>,
    named: HashMap<Entity, Option<Rc<Grid>>>,
    fields: HashMap<Entity, Option<FieldOn>>,
}

impl Tick<'_> {
    fn grid_for(
        &mut self,
        world: &World,
        at: [f32; 2],
        explicit: Entity,
    ) -> Result<Option<Rc<Grid>>> {
        if !explicit.is_null() {
            if let Some(g) = self.named.get(&explicit) {
                return Ok(g.clone());
            }
            let g = match self.store.grid(world, explicit, false) {
                Ok(g) => Some(g),
                Err(e) if e.code == crate::codes::NO_GRID => None,
                Err(e) => return Err(e),
            };
            self.named.insert(explicit, g.clone());
            return Ok(g);
        }
        if self.grids.is_none() {
            let mut all = Vec::new();
            for e in self.store.grids(world) {
                all.push((e, self.store.grid(world, e, false)?));
            }
            self.grids = Some(all);
        }
        Ok(self
            .grids
            .as_ref()
            .expect("just made")
            .iter()
            .find(|(_, g)| g.contains(at))
            .map(|(_, g)| g.clone()))
    }

    fn field(&mut self, world: &World, e: Entity) -> Result<Option<FieldOn>> {
        if let Some(f) = self.fields.get(&e) {
            return Ok(f.clone());
        }
        let f = match self.store.field_of(world, e, false) {
            Ok(f) => f,
            Err(err) if err.code == crate::codes::NO_GRID => None,
            Err(err) => return Err(err),
        };
        self.fields.insert(e, f.clone());
        Ok(f)
    }
}

pub(crate) fn install(
    world: &mut World,
    sched: &mut Scheduler,
    store: Rc<RefCell<NavStore>>,
) -> Result<()> {
    let agent = world.component_id_of::<NavAgent>().expect("registered");
    let mut desc = SystemDesc::new(AGENTS, Stage::Sim)
        .last()
        .writes(agent)
        .source("argus-nav:agents")
        .doc("Walks every NavAgent a tick: finds its path with A* when its goal's cell or the grid changes (from the cells kept in the component, so a seek walks the same way), or steps it down its NavField; moves it at speed toward the next waypoint; writes state, next, and remaining; emits nav.arrived and nav.blocked when it gets there or cannot.");
    for id in [
        world.component_id_of::<NavGrid>(),
        world.component_id_of::<NavCost>(),
        world.component_id_of::<NavField>(),
        world.component_id_of::<Parent>(),
        world.component_id_of::<GlobalTransform>(),
        world.component_id("Tilemap"),
    ]
    .into_iter()
    .flatten()
    {
        desc = desc.reads(id);
    }
    if let Some(tr) = world.component_id_of::<Transform>() {
        desc = desc.reads(tr).writes(tr);
    }
    let s = world.strings();
    let states = States {
        moving: s.intern("moving"),
        arrived: s.intern("arrived"),
        blocked: s.intern("blocked"),
    };
    sched.add_system(
        desc,
        Box::new(move |ctx| {
            let Some(id) = ctx.world().component_id_of::<NavAgent>() else {
                return Ok(());
            };
            // 1. The agents and where they are, in the views' order.
            let mut list: Vec<(Entity, NavAgent, [f32; 2], bool)> = Vec::new();
            ctx.query().has(id).for_each_view(|v| {
                let a = v.column::<NavAgent>()?;
                let places = v.column::<Transform>().ok();
                let child = v.column::<Parent>().is_ok();
                let globals = v.column::<GlobalTransform>().ok();
                for (k, e) in v.entities().iter().enumerate() {
                    let p = match (child, &globals, &places) {
                        (true, Some(g), _) => [g[k].pos.x, g[k].pos.y],
                        (_, _, Some(t)) => [t[k].pos.x, t[k].pos.y],
                        // An agent with no place is left alone.
                        _ => [f32::NAN, f32::NAN],
                    };
                    list.push((*e, a[k], p, child));
                }
                Ok(())
            })?;
            if list.is_empty() {
                return Ok(());
            }
            // 2. Each worked out, in entity order.
            let mut order: Vec<usize> = (0..list.len()).collect();
            order.sort_by_key(|&i| list[i].0);
            let dt = ctx.dt as f32;
            let mut after: Vec<Option<(NavAgent, Option<[f32; 2]>)>> = vec![None; list.len()];
            let mut events: Vec<(&'static str, Entity, Option<String>)> = Vec::new();
            {
                let world = ctx.world();
                let mut st = store.borrow_mut();
                let mut tick = Tick {
                    store: &mut st,
                    grids: None,
                    named: HashMap::new(),
                    fields: HashMap::new(),
                };
                for i in order {
                    let (e, a, pos, child) = list[i];
                    if pos[0].is_nan() {
                        continue;
                    }
                    let (na, moved, why) = walk_one(&mut tick, world, &states, a, pos, dt)?;
                    if na.state != a.state {
                        if na.state == states.arrived {
                            events.push(("nav.arrived", e, None));
                        } else if na.state == states.blocked {
                            events.push(("nav.blocked", e, why));
                        }
                    }
                    // A child is walked by its parent; its own place stays.
                    let moved = (moved != pos && !child).then_some(moved);
                    if na != a || moved.is_some() {
                        after[i] = Some((na, moved));
                    }
                }
            }
            // 3. What changed, written back in the views' order.
            if after.iter().any(|x| x.is_some()) {
                let mut k0 = 0usize;
                ctx.query().has(id).for_each_view(|v| {
                    let n = v.entities().len();
                    if after[k0..k0 + n].iter().any(|x| x.is_some()) {
                        let mut a = v.column_mut::<NavAgent>()?;
                        let mut t = v.column_mut::<Transform>().ok();
                        for k in 0..n {
                            if let Some((na, moved)) = after[k0 + k] {
                                a[k] = na;
                                if let (Some(p), Some(t)) = (moved, t.as_mut()) {
                                    t[k].pos.x = p[0];
                                    t[k].pos.y = p[1];
                                }
                            }
                        }
                    }
                    k0 += n;
                    Ok(())
                })?;
            }
            for (name, e, why) in events {
                let mut fields = vec![("entity".to_string(), Value::Entity(e))];
                if let Some(w) = why {
                    fields.push(("why".to_string(), Value::Str(w)));
                }
                ctx.emit(name, fields);
            }
            Ok(())
        }),
    )
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

/// Move from `pos` toward `to` by at most `budget`; returns where it got
/// and the budget left.
fn toward(pos: [f32; 2], to: [f32; 2], budget: f32) -> ([f32; 2], f32) {
    let d = dist(pos, to);
    if d <= budget || d == 0.0 {
        return (to, budget - d);
    }
    let k = budget / d;
    (
        [pos[0] + (to[0] - pos[0]) * k, pos[1] + (to[1] - pos[1]) * k],
        0.0,
    )
}

/// One agent's tick: its component after, where it is, and why it is
/// blocked when it is.
fn walk_one(
    tick: &mut Tick<'_>,
    world: &World,
    states: &States,
    mut a: NavAgent,
    pos: [f32; 2],
    dt: f32,
) -> Result<(NavAgent, [f32; 2], Option<String>)> {
    let blocked = |mut a: NavAgent, why: &str| {
        a.state = states.blocked;
        a.remaining = 0.0;
        Ok((a, pos, Some(why.to_string())))
    };
    let budget = a.speed.max(0.0) * dt;

    // A member of a crowd: down the field.
    if !a.field.is_null() {
        let Some((g, f)) = tick.field(world, a.field)? else {
            return blocked(a, "its field is on no grid");
        };
        let Some(at) = g.locate(pos) else {
            return blocked(a, "it is off the field's grid");
        };
        let Some(d) = f.cost(at) else {
            return blocked(a, "the field does not reach it");
        };
        let (goal, last) = if d == 0 {
            (position(world, a.field).unwrap_or(pos), true)
        } else {
            match downhill(&g, &f, at) {
                Some(n) => (g.centre(n), false),
                None => return blocked(a, "no way down the field"),
            }
        };
        let (moved, _) = if budget > 0.0 {
            toward(pos, goal, budget)
        } else {
            (pos, 0.0)
        };
        a.next = Vec2::new(goal[0], goal[1]);
        let left = dist(moved, goal);
        a.remaining = if last {
            left
        } else {
            d as f32 / STRAIGHT as f32 * g.cell
        };
        a.state = if last && left <= a.arrive {
            states.arrived
        } else {
            states.moving
        };
        return Ok((a, moved, None));
    }

    // An individual: its own A* path.
    let goal = if a.target.is_null() {
        [a.goal.x, a.goal.y]
    } else {
        match position(world, a.target) {
            Some(p) => p,
            None => return blocked(a, "its target is gone"),
        }
    };
    let Some(g) = tick.grid_for(world, pos, a.grid)? else {
        return blocked(a, "it is on no grid");
    };
    let Some(to) = g.locate(goal) else {
        return blocked(a, "the goal is off the grid");
    };
    if a.from == u32::MAX || a.to != to || a.grid_hash != g.hash as u32 {
        let Some(from) = g.locate(pos) else {
            return blocked(a, "it is off the grid");
        };
        a.from = from;
        a.to = to;
        a.grid_hash = g.hash as u32;
        a.step = 1;
    }
    let Some(route) = tick.store.path(&g, a.from, a.to) else {
        return blocked(a, "no path to the goal");
    };
    let pts = waypoints(&g, &route.cells, goal);
    let mut i = (a.step.max(1) as usize - 1).min(pts.len() - 1);
    let mut at = pos;
    let mut budget = budget;
    loop {
        let d = dist(at, pts[i]);
        // A waypoint on the way is passed once close enough; the goal is
        // walked to exactly, as far as the tick allows.
        if d <= a.arrive && i + 1 < pts.len() {
            i += 1;
            continue;
        }
        if budget <= 0.0 || d == 0.0 {
            break;
        }
        let (m, left) = toward(at, pts[i], budget);
        at = m;
        budget = left;
    }
    a.step = i as u32 + 1;
    a.next = Vec2::new(pts[i][0], pts[i][1]);
    let mut remaining = dist(at, pts[i]);
    for w in pts[i..].windows(2) {
        remaining += dist(w[0], w[1]);
    }
    a.remaining = remaining;
    a.state = if i + 1 == pts.len() && dist(at, pts[i]) <= a.arrive {
        states.arrived
    } else {
        states.moving
    };
    Ok((a, at, None))
}
