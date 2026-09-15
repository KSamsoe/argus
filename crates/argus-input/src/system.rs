//! The `PlayerSlot` component, the generated `Actions` component, and the
//! `input.actions` system that writes one onto entities carrying the other
//! at the start of every tick (§3 "input" stage, §15 per-player-slot
//! actions).

use crate::map::ActionMap;
use crate::state::InputState;
use argus_ecs::{Component, ComponentId, Result, Value, World};
use argus_sim::{Scheduler, Stage, SystemDesc};
use std::cell::RefCell;
use std::rc::Rc;

/// Name of the generated per-slot actions component.
pub const ACTIONS: &str = "Actions";
/// Name of the input system.
pub const SYSTEM: &str = "input.actions";

/// Marks an entity as receiving a player slot's actions.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "Receives a player slot's actions: input.actions writes Actions onto this entity every tick."
)]
pub struct PlayerSlot {
    /// 1-based slot from input.toml.
    #[argus(doc = "Player slot, 1 to 8", range(1, 8))]
    pub slot: u8,
}

impl Default for PlayerSlot {
    fn default() -> Self {
        PlayerSlot { slot: 1 }
    }
}

/// Register `PlayerSlot` and the map's `Actions` schema. Returns their ids.
pub fn register(world: &mut World, map: &ActionMap) -> Result<(ComponentId, ComponentId)> {
    let slot = match world.component_id_of::<PlayerSlot>() {
        Some(id) => id,
        None => world.register::<PlayerSlot>()?,
    };
    let actions = match world.component_id(ACTIONS) {
        Some(id) => id,
        None => world.register_schema(map.schema()?)?,
    };
    Ok((slot, actions))
}

/// Register the components and add the `input.actions` system to the
/// scheduler. The state is shared with the daemon, which injects into it.
pub fn install(
    world: &mut World,
    sched: &mut Scheduler,
    state: Rc<RefCell<InputState>>,
) -> Result<()> {
    let map = state.borrow().map().clone();
    let (slot_id, actions_id) = register(world, &map)?;
    let desc = SystemDesc::new(SYSTEM, Stage::Input)
        .reads(slot_id)
        .writes(actions_id)
        .source(format!("{}:1", map.file))
        .doc("Applies injected, scripted, or replayed input for this tick and writes each player slot's Actions.");
    let st = state.clone();
    sched.add_system(
        desc,
        Box::new(move |ctx| {
            let tick = ctx.tick;
            st.borrow_mut().begin_tick(tick);
            let entities = ctx.query().has(slot_id).has(actions_id).entities();
            let st = st.borrow();
            for e in entities {
                let slot = match ctx.get_field(e, "PlayerSlot.slot")? {
                    Value::UInt(n) => n as u8,
                    Value::Int(n) => n as u8,
                    _ => 1,
                };
                for (field, v) in st.fields_for(slot) {
                    ctx.set_field(e, &format!("{ACTIONS}.{field}"), v)?;
                }
            }
            Ok(())
        }),
    )
}
