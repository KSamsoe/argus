//! `rooms.stream` (phase 3 item 6, §4 "Rooms and streaming"): "the daemon
//! streams rooms in and out around the player by graph distance, so a
//! two-hundred-room game never has more than a handful of rooms live."
//!
//! The scene names the entity rooms stream around (`[rooms] follow`). At
//! the start of every tick, when that entity has left the current room's
//! bounds for another room's, the rooms within the radius of the new one
//! (and the always-loaded ones) are loaded and the rest unloaded, each
//! remembering what happened in it, and `room.transition {from, to}` is an
//! event. At the start, before the physics: an entity moved into a far
//! room between ticks (a teleport, a load) finds its floor there on the
//! tick it arrives (phase 3 item 7 found it falling through, when the
//! rooms streamed at the end of the tick). The game's hook is the event, or a timeline the scene names
//! (`[rooms] transition`), played with the followed entity as its actor.
//!
//! It is a system, not something the daemon does between ticks, so a
//! replay, a seek, and every build (the daemon, the executable, the page)
//! stream at the same tick. It is exclusive: it spawns and despawns whole
//! rooms, and declares the components it put on entities as it goes.
//!
//! The room the followed entity is in is also data on it, [`InRoom`], for
//! whatever follows the rooms: a room's music (phase 3 item 7), a
//! condition in a conversation or a timeline.

use crate::Scene;
use argus_ecs::{Component, Entity, GlobalTransform, Result, StrId, Transform, Value, World};
use argus_sim::{Scheduler, Stage, SystemCtx, SystemDesc};
use std::cell::RefCell;
use std::rc::Rc;

/// The system's name.
pub const SYSTEM: &str = "rooms.stream";

/// The room the entity a scene follows is in.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
#[argus(
    doc = "The room the entity the scene follows ([rooms] follow) is in (written by rooms.stream): what a room's music or a condition reads"
)]
pub struct InRoom {
    /// The room's name.
    #[argus(doc = "The room's name (its directory under rooms/)")]
    pub room: StrId,
}

/// Register [`InRoom`], before scripts load so a system may declare it
/// (registering twice is the same id).
pub fn register(world: &mut World) -> Result<()> {
    world.register::<InRoom>()?;
    Ok(())
}

/// Register [`InRoom`] and add `rooms.stream` first in the input stage,
/// before the physics moves anything. A scene without `[rooms] follow`
/// costs a borrow and a lookup.
pub fn install(world: &mut World, sched: &mut Scheduler, scene: Rc<RefCell<Scene>>) -> Result<()> {
    let in_room = world.register::<InRoom>()?;
    sched.add_system(
        SystemDesc::new(SYSTEM, Stage::Input)
            .first()
            .exclusive()
            .unfrozen()
            .writes(in_room)
            .source("argus-scene:stream")
            .doc("Streams rooms around the scene's followed entity ([rooms] follow): when it has left the current room's bounds for another room's, the rooms within the radius of that one and the always-loaded ones are loaded, the rest unloaded (each remembering what happened in it), room.transition {from, to} is emitted, and the scene's transition timeline ([rooms] transition) plays with the entity as its actor. Writes the entity's InRoom. Declares the components it puts on entities."),
        Box::new(move |ctx| run(ctx, &scene)),
    )
}

/// Where it is now: its own Transform, or, with a parent, its global one
/// (propagated at the end of the last tick).
fn position(world: &World, e: Entity) -> Option<[f32; 2]> {
    let parented = world.get::<argus_ecs::Parent>(e).is_ok();
    if parented {
        if let Ok(g) = world.get::<GlobalTransform>(e) {
            return Some([g.pos.x, g.pos.y]);
        }
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

/// The followed entity's `InRoom`, when it is not the current room.
fn mark_room(ctx: &mut SystemCtx<'_>, scene: &Rc<RefCell<Scene>>) -> Result<()> {
    let (e, room) = {
        let s = scene.borrow();
        let (Some(f), Some(cur)) = (s.follow(), s.current_room()) else {
            return Ok(());
        };
        let Ok(e) = ctx.world().entity(f) else {
            return Ok(());
        };
        (e, ctx.world().strings().intern(cur))
    };
    match ctx.world().get::<InRoom>(e) {
        Ok(r) if r.room == room => Ok(()),
        Ok(_) => ctx.world_mut()?.update::<InRoom, _>(e, |r| r.room = room),
        Err(_) => ctx.world_mut()?.insert(e, InRoom { room }),
    }
}

fn run(ctx: &mut SystemCtx<'_>, scene: &Rc<RefCell<Scene>>) -> Result<()> {
    let Some((to, who)) = wanted(ctx, &scene.borrow()) else {
        return mark_room(ctx, scene);
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
    mark_room(ctx, scene)?;
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
