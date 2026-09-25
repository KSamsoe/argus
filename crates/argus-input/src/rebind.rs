//! Rebinding at runtime (§12 "Input actions": "Rebinding at runtime is a
//! UI over the same table"; phase 3 item 1).
//!
//! A controls menu is UI buttons each with an [`InputBinding`]: the action
//! and device it rebinds. `input.rebind` (sim stage, after the UI's input)
//! keeps each one's `keys` showing what the action is bound to; when one is
//! pressed (`ui.pressed`), it waits for the next press on its device, which
//! the host hands to the input state instead of the game (`capture`), and
//! binds it: `input.rebound {action, device, binding}`, or
//! `input.rebind_cancelled` on Escape. The log records actions, not keys,
//! so a replay survives a rebind.

use crate::state::{InputState, Rebound};
use argus_ecs::{Component, Result, StrId, Value, World};
use argus_sim::{Scheduler, Stage, SystemDesc};
use std::cell::RefCell;
use std::rc::Rc;

/// The rebinding system's name.
pub const REBIND: &str = "input.rebind";

/// A button that rebinds an action.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "A controls menu's button: pressed, it waits for the next press on device and binds action to it (Escape cancels); keys shows what the action is bound to, for a UiText hole ({InputBinding.keys})"
)]
pub struct InputBinding {
    /// The action.
    #[argus(doc = "The action it rebinds")]
    pub action: StrId,
    /// The device.
    #[argus(doc = "keyboard (default), mouse, or gamepad")]
    pub device: StrId,
    /// What it is bound to.
    #[argus(
        doc = "What the action is bound to on the device, or \"press a key\" while waiting (written by input.rebind)"
    )]
    pub keys: StrId,
    /// Waiting.
    #[argus(doc = "Waiting for a press (written by input.rebind)")]
    pub listening: bool,
}

impl Default for InputBinding {
    fn default() -> Self {
        InputBinding {
            action: StrId::EMPTY,
            device: StrId::EMPTY,
            keys: StrId::EMPTY,
            listening: false,
        }
    }
}

/// Register `InputBinding` and add `input.rebind` to the sim stage, after
/// `ui.input` when it runs (install after the UI).
pub fn install(
    world: &mut World,
    sched: &mut Scheduler,
    state: Rc<RefCell<InputState>>,
) -> Result<()> {
    let id = match world.component_id_of::<InputBinding>() {
        Some(id) => id,
        None => world.register::<InputBinding>()?,
    };
    let mut desc = SystemDesc::new(REBIND, Stage::Sim)
        .unfrozen()
        .reads(id)
        .writes(id)
        .source("argus-input:rebind")
        .doc("Rebinding: a pressed button with an InputBinding waits for the next press on its device and binds its action to it (input.rebound; Escape cancels, input.rebind_cancelled); every InputBinding's keys says what its action is bound to.");
    if sched.is_enabled("ui.input").is_ok() {
        desc = desc.after("ui.input");
    }
    sched.add_system(
        desc,
        Box::new(move |ctx| {
            let dev = |s: &str| {
                if s.is_empty() {
                    "keyboard".to_string()
                } else {
                    s.to_string()
                }
            };
            // A press on a rebinding button: listen.
            let pressed: Vec<argus_ecs::Entity> = ctx
                .events_now("ui.pressed")
                .filter_map(|r| match &r.event {
                    argus_ecs::Event::Game { fields, .. } => {
                        fields.iter().find_map(|(k, v)| match (k.as_str(), v) {
                            ("entity", Value::Entity(e)) => Some(*e),
                            _ => None,
                        })
                    }
                    _ => None,
                })
                .collect();
            for e in pressed {
                if let Ok(b) = ctx.get::<InputBinding>(e) {
                    let action = ctx.world().strings().resolve(b.action).to_string();
                    let device = dev(&ctx.world().strings().resolve(b.device));
                    let _ = state.borrow_mut().listen(&action, &device, e);
                }
            }
            // What listening came to.
            for r in state.borrow_mut().take_rebound() {
                match r {
                    Rebound::Bound { listen, binding } => ctx.emit(
                        "input.rebound",
                        vec![
                            ("action".into(), Value::Str(listen.action)),
                            ("device".into(), Value::Str(listen.device)),
                            ("binding".into(), Value::Str(binding)),
                            ("entity".into(), Value::Entity(listen.entity)),
                        ],
                    ),
                    Rebound::Cancelled(listen) => ctx.emit(
                        "input.rebind_cancelled",
                        vec![
                            ("action".into(), Value::Str(listen.action)),
                            ("entity".into(), Value::Entity(listen.entity)),
                        ],
                    ),
                }
            }
            // Every button's keys.
            let st = state.borrow();
            let waiting = st.listening().map(|l| l.entity);
            let mut writes = Vec::new();
            for e in ctx.query().has(id).entities() {
                let Ok(b) = ctx.get::<InputBinding>(e) else {
                    continue;
                };
                let listening = waiting == Some(e);
                let text = if listening {
                    "press a key".to_string()
                } else {
                    let action = ctx.world().strings().resolve(b.action).to_string();
                    let device = dev(&ctx.world().strings().resolve(b.device));
                    st.map().bindings(&action, &device).join(", ")
                };
                let keys = ctx.world().strings().intern(&text);
                if keys != b.keys || listening != b.listening {
                    writes.push((e, keys, listening));
                }
            }
            drop(st);
            for (e, keys, listening) in writes {
                ctx.update::<InputBinding, _>(e, |b| {
                    b.keys = keys;
                    b.listening = listening;
                })?;
            }
            Ok(())
        }),
    )
}
