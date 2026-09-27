//! `rooms.stream` (phase 3 item 6, §4 "Rooms and streaming"): "the daemon
//! streams rooms in and out around the player by graph distance, so a
//! two-hundred-room game never has more than a handful of rooms live."
//!
//! The scene names the entity rooms stream around (`[rooms] follow`). At
//! the end of every tick, when that entity has left the current room's
//! bounds for another room's, the rooms within the radius of the new one
//! (and the always-loaded ones) are loaded and the rest unloaded, each
//! remembering what happened in it, and `room.transition {from, to}` is an
//! event. The game's hook is the event, or a timeline the scene names
//! (`[rooms] transition`), played with the followed entity as its actor.
//!
//! It is a system, not something the daemon does between ticks, so a
//! replay, a seek, and every build (the daemon, the executable, the page)
//! stream at the same tick. It is exclusive: it spawns and despawns whole
//! rooms, and declares the components it put on entities as it goes.

use crate::Scene;
use argus_ecs::{Entity, GlobalTransform, Result, Transform, Value, World};
use argus_sim::{Scheduler, Stage, SystemCtx, SystemDesc};
use std::cell::RefCell;
use std::rc::Rc;

/// The system's name.
pub const SYSTEM: &str = "rooms.stream";

/// Add `rooms.stream` to the late stage (last; install it after the other
/// late systems so it runs after the camera and the UI have seen the
/// tick). A scene without `[rooms] follow` costs a borrow and a lookup.
pub fn install(sched: &mut Scheduler, scene: Rc<RefCell<Scene>>) -> Result<()> {
    sched.add_system(
        SystemDesc::new(SYSTEM, Stage::Late)
            .last()
            .exclusive()
            .unfrozen()
            .source("argus-scene:stream")
            .doc("Streams rooms around the scene's followed entity ([rooms] follow): when it has left the current room's bounds for another room's, the rooms within the radius of that one and the always-loaded ones are loaded, the rest unloaded (each remembering what happened in it), room.transition {from, to} is emitted, and the scene's transition timeline ([rooms] transition) plays with the entity as its actor. Declares the components it puts on entities."),
        Box::new(move |ctx| run(ctx, &scene)),
    )
}

fn position(world: &World, e: Entity) -> Option<[f32; 2]> {
    if let Ok(g) = world.get::<GlobalTransform>(e) {
        return Some([g.pos.x, g.pos.y]);
    }
    world.get::<Transform>(e).ok().map(|t| [t.pos.x, t.pos.y])
}

/// Where the followed entity has gone, when it is a room change.
fn wanted(ctx: &SystemCtx<'_>, s: &Scene) -> Option<(String, Entity)> {
    let follow = s.follow()?;
    if !s.issues().is_empty() {
        return None;
    }
    let e = ctx.world().entity(follow).ok()?;
    let [x, y] = position(ctx.world(), e)?;
    let cur = s.current_room();
    if cur.is_some_and(|c| s.room_holds(c, x, y)) {
        return None;
    }
    // Between rooms, or outside every room: stay where it was.
    let to = s.room_at(x, y)?;
    (Some(to) != cur).then(|| (to.to_string(), e))
}

fn run(ctx: &mut SystemCtx<'_>, scene: &Rc<RefCell<Scene>>) -> Result<()> {
    let Some((to, who)) = wanted(ctx, &scene.borrow()) else {
        return Ok(());
    };
    let report = scene.borrow_mut().transition(ctx.world_mut()?, &to)?;
    let written: Vec<_> = report
        .components
        .iter()
        .filter_map(|n| ctx.world().component_id(n))
        .collect();
    for c in written {
        ctx.declare(c, true);
    }
    // The game's hook: a timeline played at every room change.
    let hook = scene.borrow().transition_timeline().map(str::to_string);
    if let Some(name) = hook {
        let world = ctx.world_mut()?;
        let Some(tid) = world.component_id("Timeline") else {
            return Ok(());
        };
        let t = world.spawn();
        world.insert_by_name(t, "Timeline")?;
        world.set_field(t, "Timeline.name", Value::Str(name))?;
        world.set_field(t, "Timeline.actor", Value::Entity(who))?;
        ctx.declare(tid, true);
    }
    Ok(())
}
