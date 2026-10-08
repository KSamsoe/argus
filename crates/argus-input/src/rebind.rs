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

/// A settings menu's button for an action a player may hold or toggle
/// (section 12: "input-hold and toggle alternatives are action-map
/// options"; phase 5 item 2): pressed, it switches the action between the
/// two. The switch happens in the tick, so a replay switches it too.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "A settings menu's button that switches a button action between holding and toggling: pressed, a press of the action then toggles it (or holds it again); toggle says which (input.toggled)"
)]
pub struct InputToggle {
    /// The action.
    #[argus(doc = "The action (a button) it switches")]
    pub action: StrId,
    /// Toggling.
    #[argus(doc = "A press toggles the action instead of holding it (written by input.rebind)")]
    pub toggle: bool,
}

impl Default for InputToggle {
    fn default() -> Self {
        InputToggle {
            action: StrId::EMPTY,
            toggle: false,
        }
    }
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

/// What a button waiting for a key says, in the world's language: the
/// assembly hands the string tables' (phase 3 item 4), since this crate
/// reads none.
pub type Prompt = Rc<dyn Fn(&World) -> String>;

/// Register `InputBinding` and add `input.rebind` to the sim stage, after
/// `ui.input` when it runs (install after the UI). A waiting button says
/// "press a key".
pub fn install(
    world: &mut World,
    sched: &mut Scheduler,
    state: Rc<RefCell<InputState>>,
) -> Result<()> {
    install_with(
        world,
        sched,
        state,
        Rc::new(|_: &World| "press a key".to_string()),
    )
}

/// [`install`], with what a waiting button says.
pub fn install_with(
    world: &mut World,
    sched: &mut Scheduler,
    state: Rc<RefCell<InputState>>,
    prompt: Prompt,
) -> Result<()> {
    let id = match world.component_id_of::<InputBinding>() {
        Some(id) => id,
        None => world.register::<InputBinding>()?,
    };
    let tid = match world.component_id_of::<InputToggle>() {
        Some(id) => id,
        None => world.register::<InputToggle>()?,
    };
    let mut desc = SystemDesc::new(REBIND, Stage::Sim)
        .unfrozen()
        .reads(id)
        .writes(id)
        .reads(tid)
        .writes(tid)
        .source("argus-input:rebind")
        .doc("Rebinding: a pressed button with an InputBinding waits for the next press on its device and binds its action to it (input.rebound; Escape cancels, input.rebind_cancelled); every InputBinding's keys says what its action is bound to. A pressed button with an InputToggle switches its action between holding and toggling (input.toggled); every InputToggle's toggle says which.");
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
                if let Ok(t) = ctx.get::<InputToggle>(e) {
                    let action = ctx.world().strings().resolve(t.action).to_string();
                    let mut st = state.borrow_mut();
                    let now = st.map().action(&action).is_ok_and(|a| a.toggle);
                    if st.map_mut().set_toggle(&action, !now).is_ok() {
                        ctx.emit(
                            "input.toggled",
                            vec![
                                ("action".into(), Value::Str(action)),
                                ("toggle".into(), Value::Bool(!now)),
                                ("entity".into(), Value::Entity(e)),
                            ],
                        );
                    }
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
                    prompt(ctx.world())
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
            // Every toggle button's state.
            let mut toggles = Vec::new();
            for e in ctx.query().has(tid).entities() {
                let Ok(t) = ctx.get::<InputToggle>(e) else {
                    continue;
                };
                let action = ctx.world().strings().resolve(t.action).to_string();
                let on = st.map().action(&action).is_ok_and(|a| a.toggle);
                if on != t.toggle {
                    toggles.push((e, on));
                }
            }
            drop(st);
            for (e, keys, listening) in writes {
                ctx.update::<InputBinding, _>(e, |b| {
                    b.keys = keys;
                    b.listening = listening;
                })?;
            }
            for (e, on) in toggles {
                ctx.update::<InputToggle, _>(e, |t| t.toggle = on)?;
            }
            Ok(())
        }),
    )
}
