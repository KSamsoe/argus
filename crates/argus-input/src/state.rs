//! Injected input, the per-tick action values, and the replayable log
//! (§6 "the input log records actions by tick or commands by turn").
//!
//! Every value change is scheduled for a tick, applied when that tick's
//! `input` stage runs, and appended to the log as `tick, slot, action,
//! value`. Replaying a log schedules the same changes at the same ticks,
//! so the same scene, seed, and code give the same run.
//!
//! The commands themselves are journaled too, each stamped with the tick it
//! came before (phase 2, seek). A snapshot of this state plus the journal
//! after it rebuilds the state at any later tick exactly, which the value log
//! cannot: a toggle's recorded release would flip it again, and an inject's
//! own release is scheduled by the inject, not logged until it happens.

use crate::codes;
use crate::map::{ActionKind, ActionMap};
use argus_ecs::codec::{Reader, Writer};
use argus_ecs::{ArgusError, Result, Value, Vec2};
use serde_json::{json, Value as Json};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};

/// A value an action can hold.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ActionValue {
    /// Button held or not.
    Button(bool),
    /// One axis.
    Axis(f32),
    /// Two axes.
    Axis2(f32, f32),
}

impl ActionValue {
    /// The rest value of a kind.
    pub fn default_for(kind: ActionKind) -> ActionValue {
        match kind {
            ActionKind::Button => ActionValue::Button(false),
            ActionKind::Axis => ActionValue::Axis(0.0),
            ActionKind::Axis2 | ActionKind::Pointer => ActionValue::Axis2(0.0, 0.0),
        }
    }

    /// The "active" value of a kind, for an inject without a value: a
    /// button down, an axis at 1, a stick pushed right.
    pub fn active_for(kind: ActionKind) -> ActionValue {
        match kind {
            ActionKind::Button => ActionValue::Button(true),
            ActionKind::Axis => ActionValue::Axis(1.0),
            ActionKind::Axis2 => ActionValue::Axis2(1.0, 0.0),
            ActionKind::Pointer => ActionValue::Axis2(0.0, 0.0),
        }
    }

    /// Parse a protocol value for a kind: `true`/`false`, a number, `[x, y]`.
    pub fn from_json(kind: ActionKind, j: &Json, action: &str) -> Result<ActionValue> {
        let bad = |got: &str| {
            ArgusError::new(
                codes::BAD_VALUE,
                format!(
                    "{action} is a {} and takes {}; got {got}",
                    kind.name(),
                    match kind {
                        ActionKind::Button => "true or false",
                        ActionKind::Axis if action == crate::map::DEVICE_ACTION => {
                            "a kind of device: 0 keyboard and mouse, 1 Xbox, 2 PlayStation, 3 Switch, 4 another gamepad"
                        }
                        ActionKind::Axis => "a number in [-1, 1]",
                        ActionKind::Axis2 => "[x, y] with each in [-1, 1]",
                        ActionKind::Pointer => "[x, y] in canvas pixels",
                    }
                ),
            )
        };
        let clamp = |x: f64| -> Result<f32> {
            if !(-1.0..=1.0).contains(&x) {
                return Err(bad(&format!("{x}")));
            }
            Ok(x as f32)
        };
        Ok(match (kind, j) {
            (ActionKind::Button, Json::Bool(b)) => ActionValue::Button(*b),
            (ActionKind::Button, Json::Number(n)) => {
                ActionValue::Button(n.as_f64().unwrap_or(0.0) != 0.0)
            }
            (ActionKind::Axis, Json::Number(n)) if action == crate::map::DEVICE_ACTION => {
                let v = n.as_f64().unwrap_or(-1.0);
                if !(0.0..=4.0).contains(&v) || v.fract() != 0.0 {
                    return Err(bad(&format!("{v}")));
                }
                ActionValue::Axis(v as f32)
            }
            (ActionKind::Axis, Json::Number(n)) => {
                ActionValue::Axis(clamp(n.as_f64().unwrap_or(0.0))?)
            }
            (ActionKind::Pointer, Json::Array(a)) if a.len() == 2 => {
                let x = a[0].as_f64().ok_or_else(|| bad("a non-number"))?;
                let y = a[1].as_f64().ok_or_else(|| bad("a non-number"))?;
                ActionValue::Axis2(x as f32, y as f32)
            }
            (ActionKind::Axis2, Json::Array(a)) if a.len() == 2 => {
                let x = a[0].as_f64().ok_or_else(|| bad("a non-number"))?;
                let y = a[1].as_f64().ok_or_else(|| bad("a non-number"))?;
                ActionValue::Axis2(clamp(x)?, clamp(y)?)
            }
            (_, other) => return Err(bad(&other.to_string())),
        })
    }

    /// Protocol spelling.
    pub fn to_json(self) -> Json {
        match self {
            ActionValue::Button(b) => json!(b),
            ActionValue::Axis(x) => json!(x),
            ActionValue::Axis2(x, y) => json!([x, y]),
        }
    }

    /// Does this count as "down" (for edges and toggles)?
    pub fn is_active(self) -> bool {
        match self {
            ActionValue::Button(b) => b,
            ActionValue::Axis(x) => x != 0.0,
            ActionValue::Axis2(x, y) => x != 0.0 || y != 0.0,
        }
    }

    /// Whether this value is one an action of `kind` takes (a pointer's is
    /// two numbers, like a stick's).
    fn fits(self, kind: ActionKind) -> bool {
        matches!(
            (self, kind),
            (ActionValue::Button(_), ActionKind::Button)
                | (ActionValue::Axis(_), ActionKind::Axis)
                | (
                    ActionValue::Axis2(..),
                    ActionKind::Axis2 | ActionKind::Pointer
                )
        )
    }
}

/// One line of the input log.
#[derive(Clone, Debug, PartialEq)]
pub struct LogEntry {
    /// Tick (continuous) or turn (discrete) the value took effect.
    pub tick: u64,
    /// Player slot.
    pub slot: u8,
    /// Action name.
    pub action: String,
    /// The value from that tick on.
    pub value: ActionValue,
}

impl LogEntry {
    /// The log line: tab-separated tick, slot, action, JSON value.
    pub fn line(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}",
            self.tick,
            self.slot,
            self.action,
            self.value.to_json()
        )
    }

    /// Parse a line against the map (the value's shape comes from the action).
    pub fn parse(line: &str, map: &ActionMap) -> Result<LogEntry> {
        let bad = || {
            ArgusError::new(codes::BAD_LOG, format!("bad input log line {line:?}"))
                .hint("Lines are: tick<TAB>slot<TAB>action<TAB>value, as input.record writes them.")
        };
        let mut parts = line.splitn(4, '\t');
        let tick = parts.next().and_then(|s| s.parse().ok()).ok_or_else(bad)?;
        let slot = parts.next().and_then(|s| s.parse().ok()).ok_or_else(bad)?;
        let action = parts.next().ok_or_else(bad)?.to_string();
        let value: Json = parts
            .next()
            .and_then(|s| serde_json::from_str(s).ok())
            .ok_or_else(bad)?;
        let kind = map.action(&action)?.kind;
        let value = ActionValue::from_json(kind, &value, &action)?;
        Ok(LogEntry {
            tick,
            slot,
            action,
            value,
        })
    }

    /// Protocol row: `[tick, slot, action, value]`.
    pub fn to_json(&self) -> Json {
        json!([self.tick, self.slot, self.action, self.value.to_json()])
    }
}

/// What an inject scheduled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InjectReport {
    /// Tick the value takes effect.
    pub tick: u64,
    /// Tick the action returns to rest (`None` for toggles and explicit holds).
    pub until: Option<u64>,
}

/// The channel typed text waits on among the messages until its tick: no
/// game's channel can be it (a channel is letters, digits, `.`, `_`, `-`).
pub const TYPED_CHANNEL: &str = "\u{1}typed";

/// The channel a script's read of the host's save games is recorded on
/// (`q:saves`; 2D staples item 4): on a live tick the host answers from
/// its list and the answer is journaled for that tick, so a seek, a
/// replay, or a report reads the same list. Never a `host.message`.
pub const SAVES_CHANNEL: &str = "\u{1}saves";

/// A command the input state was given.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// `input.inject`.
    Inject {
        /// Player slot.
        slot: u8,
        /// Action name.
        action: String,
        /// The value as given (`None`: the action's active value).
        value: Option<Json>,
        /// Hold for this many ticks.
        frames: Option<u64>,
        /// Hold until another inject.
        hold: bool,
    },
    /// `input.script`: steps relative to the tick it came before.
    Script {
        /// The steps as given.
        steps: Vec<Json>,
    },
    /// Device values that changed, resolved through the bindings.
    Devices {
        /// Slot, action, and the value the devices now give it.
        changes: Vec<(u8, String, ActionValue)>,
    },
    /// `input.replay`: a value log scheduled at its ticks.
    Replay {
        /// The log.
        entries: Vec<LogEntry>,
    },
    /// `input.bind`: `action` on `device` bound to `names` from now on
    /// (phase 5 item 2: journaled, so a replay binds it too).
    Bind {
        /// Action name.
        action: String,
        /// Device: keyboard, mouse, gamepad.
        device: String,
        /// What it is bound to.
        names: Vec<String>,
    },
    /// The press an action waiting for a key took, between ticks (`None`:
    /// Escape): what a player bound in a controls menu, which a replay
    /// binds again (phase 5 item 2).
    Took {
        /// The name bound.
        binding: Option<String>,
    },
    /// `input.message`: text from the host (the page, the daemon, a
    /// scenario) for the next tick's scripts, as a `host.message` event
    /// (host channel item 0).
    Message {
        /// Player slot.
        slot: u8,
        /// The channel, a short name the game chose.
        channel: String,
        /// The text.
        text: String,
    },
    /// Text a slot typed while typing (issue 23).
    Text {
        /// Player slot.
        slot: u8,
        /// What was typed, control characters included.
        text: String,
    },
}

/// A message from the host: slot, channel, text.
pub type Message = (u8, String, String);

/// The longest channel name a message may have.
pub const MAX_CHANNEL: usize = 64;
/// The longest text a message may carry.
pub const MAX_MESSAGE: usize = 64 * 1024;

/// A command and the tick it came before (the next tick to run).
#[derive(Clone, Debug, PartialEq)]
pub struct JournalEntry {
    /// The next tick to run when the command came.
    pub now: u64,
    /// The command.
    pub command: Command,
}

/// The input state at a tick, for a keyframe. The journal and the log are
/// not copied: the snapshot holds how far into the journal it was.
#[derive(Clone, Debug)]
pub struct InputSnapshot {
    scheduled: BTreeMap<u64, Vec<(u8, String, ActionValue)>>,
    current: BTreeMap<(u8, String), ActionValue>,
    pressed: Vec<(u8, String)>,
    released: Vec<(u8, String)>,
    last_tick: Option<u64>,
    device_last: BTreeMap<(u8, String), ActionValue>,
    releases: BTreeMap<(u8, String), Vec<(u64, u64)>>,
    cursor: usize,
    /// The bindings and toggles, and what waits for a key (phase 5 item
    /// 2): a seek back before a rebind undoes it. `None` in a keyframe
    /// read from a file older than that, which leaves the map as it is.
    map: Option<MapState>,
    /// Messages sent and not yet due, by the tick they are for.
    messages: BTreeMap<u64, Vec<Message>>,
}

/// What a run can change of the map: each action's bindings by device and
/// whether a press toggles it, and what waits for a key.
#[derive(Clone, Debug, PartialEq)]
struct MapState {
    actions: Vec<(String, BTreeMap<String, Vec<String>>, bool)>,
    listening: Option<Listen>,
}

/// All input state of a session.
pub struct InputState {
    map: ActionMap,
    discrete: bool,
    scheduled: BTreeMap<u64, Vec<(u8, String, ActionValue)>>,
    current: BTreeMap<(u8, String), ActionValue>,
    pressed: Vec<(u8, String)>,
    released: Vec<(u8, String)>,
    log: Vec<LogEntry>,
    recorder: Option<(PathBuf, std::fs::File)>,
    last_tick: Option<u64>,
    /// What devices last resolved to, per slot and action: only a change
    /// is scheduled, so a device and an inject never fight over a value
    /// neither is changing.
    device_last: BTreeMap<(u8, String), ActionValue>,
    /// The releases injects scheduled for themselves, per slot and action:
    /// (tick the inject took hold, tick it lets go).
    releases: BTreeMap<(u8, String), Vec<(u64, u64)>>,
    /// Every command, in order, on this branch of the run. Entries from
    /// `cursor` on are the recorded future after a seek back: re-issued as
    /// their ticks come, dropped when a live command branches the run.
    journal: Vec<JournalEntry>,
    cursor: usize,
    /// The last tick whose changes are in the log (a replayed tick's are
    /// already there).
    log_through: Option<u64>,
    /// Set when a live command dropped a recorded future: the tick it came
    /// before.
    branched: Option<u64>,
    /// A live command came since the last `take_live`.
    live: bool,
    /// Waiting for a key (or a button) to bind to an action (phase 3 item
    /// 1): the next press on the device is taken for it.
    listening: Option<Listen>,
    /// What listening came to, until `take_rebound`.
    rebound: Vec<Rebound>,
    /// Messages sent and not yet due, by the tick they are for.
    messages: BTreeMap<u64, Vec<Message>>,
    /// The messages due this tick, for `input.actions` to hand the scripts.
    inbox: Vec<Message>,
    /// What each slot typed for this tick (issue 23). Text not yet due
    /// waits among the messages, on [`TYPED_CHANNEL`], so a keyframe keeps
    /// it as it keeps them.
    typed_now: BTreeMap<u8, String>,
    /// The slots typing (their `Actions.typing`): their keyboards move no
    /// action, and what they type is theirs.
    typing: BTreeSet<u8>,
    /// The slots typing when the devices were last applied.
    typing_applied: BTreeSet<u8>,
    /// This tick's input was only the UI's: characters typed (no line
    /// sent) or the wheel turned. In a turn-based game such a tick takes
    /// no turn (issue 23).
    ui_only: bool,
    /// This tick's recorded read of the host's saves, on [`SAVES_CHANNEL`].
    saves_now: Option<String>,
    /// This tick runs for the first time (not a re-run after a seek).
    fresh_now: bool,
}

/// An action waiting for a key.
#[derive(Clone, Debug, PartialEq)]
pub struct Listen {
    /// The action.
    pub action: String,
    /// The device whose next press it takes: keyboard, mouse, gamepad.
    pub device: String,
    /// Who asked (the button of a controls menu).
    pub entity: argus_ecs::Entity,
}

/// What listening came to.
#[derive(Clone, Debug, PartialEq)]
pub enum Rebound {
    /// Bound to the press.
    Bound {
        /// What was listening.
        listen: Listen,
        /// The name bound.
        binding: String,
    },
    /// Escape: nothing changed.
    Cancelled(Listen),
}

impl InputState {
    /// Fresh state for a map; `discrete` picks turn semantics for injects.
    pub fn new(map: ActionMap, discrete: bool) -> InputState {
        InputState {
            map,
            discrete,
            scheduled: BTreeMap::new(),
            releases: BTreeMap::new(),
            journal: Vec::new(),
            cursor: 0,
            log_through: None,
            branched: None,
            live: false,
            listening: None,
            rebound: Vec::new(),
            messages: BTreeMap::new(),
            inbox: Vec::new(),
            typed_now: BTreeMap::new(),
            typing: BTreeSet::new(),
            typing_applied: BTreeSet::new(),
            ui_only: false,
            saves_now: None,
            fresh_now: false,
            current: BTreeMap::new(),
            pressed: Vec::new(),
            released: Vec::new(),
            log: Vec::new(),
            recorder: None,
            last_tick: None,
            device_last: BTreeMap::new(),
        }
    }

    /// Feed resolved device state in at `now` (the next tick to run).
    /// Continuous: every value that changed since the last call is
    /// scheduled for `now`, exactly like an inject, so the log and a
    /// replay see actions, never keys (section 6). Discrete: a value that
    /// became active is a one-turn command (like an inject) and a release
    /// is nothing; the return is the number of commands, which is how
    /// many turns the caller should advance.
    pub fn apply_devices(&mut self, now: u64, devices: &mut crate::devices::Devices) -> usize {
        let slots: Vec<u8> = self.map.slots.iter().map(|s| s.index).collect();
        // A slot stopped typing: the keys still down (the Enter that sent
        // the line) move nothing until they are let go.
        if self.typing_applied.iter().any(|s| !self.typing.contains(s)) {
            devices.mute_held();
        }
        self.typing_applied = self.typing.clone();
        let mut commands = self.apply_typed(now, devices, &slots);
        let mut changes = Vec::new();
        for slot in slots {
            let typing = self.typing.contains(&slot);
            for (action, v) in devices.resolve_typing(&self.map, slot, typing) {
                let Ok(a) = self.map.action(&action) else {
                    continue;
                };
                let before = self
                    .device_last
                    .get(&(slot, action.clone()))
                    .copied()
                    .unwrap_or(ActionValue::default_for(a.kind));
                if v != before {
                    changes.push((slot, action, v));
                }
            }
        }
        if changes.is_empty() {
            return commands;
        }
        commands += self.apply_device_changes(now, &changes);
        self.journal_live(now, Command::Devices { changes });
        commands
    }

    /// The wheel and the text since the last poll (issue 23): a wheel's
    /// turn is a one-tick push of each axis action bound to it, journaled
    /// as an inject; text goes to each slot with a keyboard that is
    /// typing, and is dropped for one that is not.
    fn apply_typed(
        &mut self,
        now: u64,
        devices: &mut crate::devices::Devices,
        slots: &[u8],
    ) -> usize {
        let (wheel, text) = devices.take_typed();
        let mut commands = 0;
        if wheel != 0.0 {
            let v = Json::from(f64::from(wheel.clamp(-1.0, 1.0)));
            let wheeled: Vec<String> = self
                .map
                .actions
                .iter()
                .filter(|a| {
                    a.kind == ActionKind::Axis
                        && a.bindings
                            .get("mouse")
                            .is_some_and(|b| b.iter().any(|n| n == "wheel"))
                })
                .map(|a| a.name.clone())
                .collect();
            for &slot in slots {
                let mouse = self
                    .map
                    .slot(slot)
                    .is_ok_and(|s| s.devices.iter().any(|d| d == "mouse"));
                if !mouse {
                    continue;
                }
                for a in &wheeled {
                    if self.inject(now, slot, a, Some(&v), Some(1), false).is_ok() {
                        commands += 1;
                    }
                }
            }
        }
        if !text.is_empty() {
            for &slot in slots {
                let keyboard = self
                    .map
                    .slot(slot)
                    .is_ok_and(|s| s.devices.iter().any(|d| d == "keyboard"));
                if keyboard && self.typing.contains(&slot) && self.text(now, slot, &text).is_ok() {
                    commands += 1;
                }
            }
        }
        commands
    }

    /// Text a slot typed, for the tick `now` (issue 23): it is that tick's
    /// `Actions.typed`.
    pub fn text(&mut self, now: u64, slot: u8, text: &str) -> Result<()> {
        self.text_raw(now, slot, text)?;
        self.journal_live(
            now,
            Command::Text {
                slot,
                text: text.to_string(),
            },
        );
        Ok(())
    }

    fn text_raw(&mut self, now: u64, slot: u8, text: &str) -> Result<()> {
        self.map.slot(slot)?;
        if text.len() > crate::devices::MAX_TYPED {
            return Err(ArgusError::new(
                codes::BAD_VALUE,
                format!("{} bytes typed at once is too many", text.len()),
            )
            .hint(format!(
                "Text typed comes {} bytes at a time at most.",
                crate::devices::MAX_TYPED
            )));
        }
        self.messages.entry(now).or_default().push((
            slot,
            TYPED_CHANNEL.to_string(),
            text.to_string(),
        ));
        Ok(())
    }

    /// This tick's recorded read of the host's save games (`q:saves`; 2D
    /// staples item 4): what the tick read the first time it ran, when it
    /// read them.
    pub fn saves_read(&self) -> Option<&str> {
        self.saves_now.as_deref()
    }

    /// Record this tick's read of the host's save games: journaled on a
    /// tick that runs for the first time, so a re-run of it reads the
    /// same. A re-run that did not read them the first time is not
    /// recorded (it would branch the run).
    pub fn record_saves(&mut self, text: &str) {
        self.saves_now = Some(text.to_string());
        let Some(tick) = self.last_tick else { return };
        if self.fresh_now {
            self.journal_live(
                tick,
                Command::Message {
                    slot: 0,
                    channel: SAVES_CHANNEL.to_string(),
                    text: text.to_string(),
                },
            );
        }
    }

    /// What `slot` typed for this tick: its `Actions.typed`.
    pub fn typed(&self, slot: u8) -> &str {
        self.typed_now.get(&slot).map(String::as_str).unwrap_or("")
    }

    /// Whether `slot` is typing: its keyboard types, and moves no action
    /// (its `Actions.typing`, which `input.actions` reads each tick).
    pub fn set_typing(&mut self, slot: u8, on: bool) {
        if on {
            self.typing.insert(slot);
        } else {
            self.typing.remove(&slot);
        }
    }

    /// Whether `slot` is typing.
    pub fn is_typing(&self, slot: u8) -> bool {
        self.typing.contains(&slot)
    }

    /// Device values that changed, scheduled from `now`.
    fn apply_device_changes(&mut self, now: u64, changes: &[(u8, String, ActionValue)]) -> usize {
        let mut commands = 0;
        for (slot, action, v) in changes {
            let (slot, v) = (*slot, *v);
            let Ok(a) = self.map.action(action) else {
                continue;
            };
            let (kind, toggle) = (a.kind, a.toggle);
            let action = action.clone();
            {
                let key = (slot, action.clone());
                let before = self
                    .device_last
                    .get(&key)
                    .copied()
                    .unwrap_or(ActionValue::default_for(kind));
                if v == before {
                    continue;
                }
                self.device_last.insert(key, v);
                if self.discrete && kind == ActionKind::Pointer {
                    // A place, not a press: it holds where it was left and
                    // takes no turn, so a click's turn reads where the mouse
                    // is. As a press it lasted one tick and went back to
                    // 0, 0, and later moves changed nothing, so every click
                    // in a discrete game landed at 0, 0 (issue 11).
                    self.schedule(now, slot, &action, v);
                } else if self.discrete {
                    if v.is_active() && !before.is_active() {
                        self.schedule(now, slot, &action, v);
                        if !toggle {
                            self.schedule(now + 1, slot, &action, ActionValue::default_for(kind));
                        }
                        commands += 1;
                    }
                } else {
                    self.schedule(now, slot, &action, v);
                }
            }
        }
        commands
    }

    /// The map.
    pub fn map(&self) -> &ActionMap {
        &self.map
    }

    /// The map, to set a run's starting bindings and toggles (a player's
    /// settings, applied before the first tick and kept with the run). A
    /// change during a run goes through [`InputState::rebind`], a
    /// controls menu, or a toggle button, which a replay repeats.
    pub fn map_mut(&mut self) -> &mut ActionMap {
        &mut self.map
    }

    /// Bind `action` on `device` to `names` from `now` on, the next tick
    /// to run (§12: "Rebinding at runtime is a UI over the same table").
    /// The log records actions, not keys; the journal records the rebind,
    /// so a replay binds it too.
    pub fn rebind(
        &mut self,
        now: u64,
        action: &str,
        device: &str,
        names: Vec<String>,
    ) -> Result<()> {
        self.bind_raw(action, device, names.clone())?;
        self.journal_live(
            now,
            Command::Bind {
                action: action.to_string(),
                device: device.to_string(),
                names,
            },
        );
        Ok(())
    }

    fn bind_raw(&mut self, action: &str, device: &str, names: Vec<String>) -> Result<()> {
        self.map.set_bindings(action, device, names)?;
        // A wait for this action on this device is answered.
        if self
            .listening
            .as_ref()
            .is_some_and(|l| l.action == action && l.device == device)
        {
            self.listening = None;
        }
        Ok(())
    }

    /// Take the next press on `device` for `action` (a controls menu's
    /// button asked, `entity`); Escape cancels.
    pub fn listen(&mut self, action: &str, device: &str, entity: argus_ecs::Entity) -> Result<()> {
        self.map.action(action)?;
        self.listening = Some(Listen {
            action: action.to_string(),
            device: device.to_string(),
            entity,
        });
        Ok(())
    }

    /// What is waiting for a key, if anything.
    pub fn listening(&self) -> Option<&Listen> {
        self.listening.as_ref()
    }

    /// A device event while listening, at `now` (the next tick to run): a
    /// press on the device binds it (and is taken, so the game does not
    /// hear it); Escape cancels. True when the event was taken. What it
    /// took is journaled, so a replay binds the same.
    pub fn capture(&mut self, now: u64, ev: &crate::devices::DeviceInput) -> bool {
        use crate::devices::{key_name, DeviceInput};
        let Some(l) = self.listening.clone() else {
            return false;
        };
        let pressed = match ev {
            DeviceInput::Key {
                name,
                pressed: true,
            } => {
                let k = key_name(name);
                if k == "escape" {
                    self.took(None);
                    self.journal_live(now, Command::Took { binding: None });
                    return true;
                }
                (l.device == "keyboard").then_some(k)
            }
            DeviceInput::Mouse {
                button,
                pressed: true,
            } => (l.device == "mouse").then(|| button.to_ascii_lowercase()),
            DeviceInput::PadButton {
                button,
                pressed: true,
                ..
            } => (l.device == "gamepad").then(|| button.clone()),
            _ => None,
        };
        let Some(name) = pressed else {
            return false;
        };
        self.took(Some(name.clone()));
        self.journal_live(
            now,
            Command::Took {
                binding: Some(name),
            },
        );
        true
    }

    /// What waits for a key takes `binding` (`None`: cancelled).
    fn took(&mut self, binding: Option<String>) {
        let Some(l) = self.listening.take() else {
            return;
        };
        match binding {
            None => self.rebound.push(Rebound::Cancelled(l)),
            Some(name) => match self
                .map
                .set_bindings(&l.action, &l.device, vec![name.clone()])
            {
                Ok(()) => self.rebound.push(Rebound::Bound {
                    listen: l,
                    binding: name,
                }),
                Err(_) => self.rebound.push(Rebound::Cancelled(l)),
            },
        }
    }

    /// What listening came to since the last call.
    pub fn take_rebound(&mut self) -> Vec<Rebound> {
        std::mem::take(&mut self.rebound)
    }

    /// Schedule a value change at an absolute tick.
    fn schedule(&mut self, tick: u64, slot: u8, action: &str, value: ActionValue) {
        self.scheduled
            .entry(tick)
            .or_default()
            .push((slot, action.to_string(), value));
    }

    /// `input.inject {action, slot, value, frames}` at `now` (the next tick
    /// to run). Continuous: the value holds for `frames` ticks (default 1)
    /// then returns to rest; `hold: true` keeps it until another inject.
    /// Discrete: a command for the next turn. Toggles flip on the press.
    pub fn inject(
        &mut self,
        now: u64,
        slot: u8,
        action: &str,
        value: Option<&Json>,
        frames: Option<u64>,
        hold: bool,
    ) -> Result<InjectReport> {
        let report = self.inject_raw(now, slot, action, value, frames, hold)?;
        self.journal_live(
            now,
            Command::Inject {
                slot,
                action: action.to_string(),
                value: value.cloned(),
                frames,
                hold,
            },
        );
        Ok(report)
    }

    fn inject_raw(
        &mut self,
        now: u64,
        slot: u8,
        action: &str,
        value: Option<&Json>,
        frames: Option<u64>,
        hold: bool,
    ) -> Result<InjectReport> {
        let a = self.map.action(action)?.clone();
        self.map.slot(slot)?;
        let v = match value {
            Some(j) => ActionValue::from_json(a.kind, j, action)?,
            None => ActionValue::active_for(a.kind),
        };
        if !v.fits(a.kind) {
            return Err(ArgusError::new(
                codes::BAD_VALUE,
                format!("{action} is a {}", a.kind.name()),
            ));
        }
        // A newer inject replaces an older one of the same action: the
        // older one's own release, still to come, would cut this one short.
        // Only releases of injects already holding by now go; a script's
        // later steps keep theirs.
        let key = (slot, action.to_string());
        if let Some(rs) = self.releases.get_mut(&key) {
            let mut i = 0;
            while i < rs.len() {
                let (start, end) = rs[i];
                if start <= now && end > now {
                    rs.remove(i);
                    if let Some(at) = self.scheduled.get_mut(&end) {
                        if let Some(j) = at
                            .iter()
                            .position(|(s, n, val)| *s == slot && n == action && !val.is_active())
                        {
                            at.remove(j);
                        }
                        if at.is_empty() {
                            self.scheduled.remove(&end);
                        }
                    }
                } else {
                    i += 1;
                }
            }
            rs.retain(|r| r.1 > now);
        }
        self.schedule(now, slot, action, v);
        // A pointer stays where it was put.
        let until = if a.toggle || hold || !v.is_active() || a.kind == ActionKind::Pointer {
            None
        } else {
            let n = if self.discrete {
                1
            } else {
                frames.unwrap_or(1).max(1)
            };
            let t = now + n;
            self.schedule(t, slot, action, ActionValue::default_for(a.kind));
            self.releases.entry(key).or_default().push((now, t));
            Some(t)
        };
        Ok(InjectReport { tick: now, until })
    }

    /// `input.script`: injects at ticks relative to `now`.
    pub fn script(&mut self, now: u64, steps: &[Json]) -> Result<Vec<InjectReport>> {
        let out = self.script_raw(now, steps)?;
        self.journal_live(
            now,
            Command::Script {
                steps: steps.to_vec(),
            },
        );
        Ok(out)
    }

    fn script_raw(&mut self, now: u64, steps: &[Json]) -> Result<Vec<InjectReport>> {
        let mut out = Vec::new();
        for (i, s) in steps.iter().enumerate() {
            let at = s.get("at").and_then(Json::as_u64).unwrap_or(0);
            let action = s.get("action").and_then(Json::as_str).ok_or_else(|| {
                ArgusError::new(codes::BAD_VALUE, format!("steps[{i}] needs an action")).hint(
                    "Each step is {at, action, slot, value, frames, hold}; at is ticks from now.",
                )
            })?;
            let slot = s.get("slot").and_then(Json::as_u64).unwrap_or(1) as u8;
            let frames = s.get("frames").and_then(Json::as_u64);
            let hold = s.get("hold").and_then(Json::as_bool).unwrap_or(false);
            out.push(self.inject_raw(now + at, slot, action, s.get("value"), frames, hold)?);
        }
        Ok(out)
    }

    /// Replay a log: every entry becomes a scheduled change at its tick.
    /// Journaled as coming before `now`.
    pub fn replay(&mut self, now: u64, entries: &[LogEntry]) -> Result<usize> {
        let n = self.replay_raw(entries)?;
        self.journal_live(
            now,
            Command::Replay {
                entries: entries.to_vec(),
            },
        );
        Ok(n)
    }

    fn replay_raw(&mut self, entries: &[LogEntry]) -> Result<usize> {
        for e in entries {
            let a = self.map.action(&e.action)?;
            self.map.slot(e.slot)?;
            if !e.value.fits(a.kind) {
                return Err(ArgusError::new(
                    codes::BAD_LOG,
                    format!(
                        "{} is a {}, the log has {}",
                        e.action,
                        a.kind.name(),
                        e.value.to_json()
                    ),
                ));
            }
            self.schedule(e.tick, e.slot, &e.action, e.value);
        }
        Ok(entries.len())
    }

    /// Read a log file.
    pub fn read_log(path: &Path, map: &ActionMap) -> Result<Vec<LogEntry>> {
        let text = argus_os::fs::read_to_string(path).map_err(|e| {
            ArgusError::new(
                codes::BAD_LOG,
                format!("cannot read {}: {e}", path.display()),
            )
        })?;
        text.lines()
            .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
            .map(|l| LogEntry::parse(l, map))
            .collect()
    }

    /// `input.message {channel, text}` at `now` (the next tick to run): the
    /// text reaches that tick's scripts as a `host.message` event. Journaled,
    /// so a seek or a replay brings it in again (host channel item 0).
    pub fn message(&mut self, now: u64, slot: u8, channel: &str, text: &str) -> Result<()> {
        self.message_raw(now, slot, channel, text)?;
        self.journal_live(
            now,
            Command::Message {
                slot,
                channel: channel.to_string(),
                text: text.to_string(),
            },
        );
        Ok(())
    }

    fn message_raw(&mut self, now: u64, slot: u8, channel: &str, text: &str) -> Result<()> {
        self.map.slot(slot)?;
        let ok = !channel.is_empty()
            && channel.len() <= MAX_CHANNEL
            && channel
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
        if !ok {
            return Err(ArgusError::new(
                codes::BAD_VALUE,
                format!("bad message channel {channel:?}"),
            )
            .hint(format!(
                "A channel is a short name the game chose: letters, digits, '.', '_' and '-', up to {MAX_CHANNEL}."
            )));
        }
        if text.len() > MAX_MESSAGE {
            return Err(ArgusError::new(
                codes::BAD_VALUE,
                format!("a message of {} bytes is too long", text.len()),
            )
            .hint(format!(
                "A message carries up to {MAX_MESSAGE} bytes of text."
            )));
        }
        self.messages
            .entry(now)
            .or_default()
            .push((slot, channel.to_string(), text.to_string()));
        Ok(())
    }

    /// The messages due this tick, oldest first: what `input.actions` hands
    /// the scripts as `host.message` events.
    pub fn inbox(&self) -> &[Message] {
        &self.inbox
    }

    /// Apply everything scheduled up to and including `tick`; called by the
    /// `input.actions` system at the start of each tick. Returns the
    /// entries applied this tick.
    pub fn begin_tick(&mut self, tick: u64) -> Vec<LogEntry> {
        // The recorded future after a seek back: its commands come in again
        // before the ticks they came before the first time.
        while self.cursor < self.journal.len() && self.journal[self.cursor].now <= tick {
            let e = self.journal[self.cursor].clone();
            self.reissue(&e);
            self.cursor += 1;
        }
        let fresh = self.log_through.is_none_or(|t| tick > t);
        self.pressed.clear();
        self.released.clear();
        self.last_tick = Some(tick);
        self.inbox.clear();
        self.typed_now.clear();
        self.saves_now = None;
        self.fresh_now = fresh;
        let due: Vec<u64> = self.messages.range(..=tick).map(|(t, _)| *t).collect();
        for t in due {
            for (slot, channel, text) in self.messages.remove(&t).unwrap_or_default() {
                if channel == TYPED_CHANNEL {
                    self.typed_now.entry(slot).or_default().push_str(&text);
                } else if channel == SAVES_CHANNEL {
                    self.saves_now = Some(text);
                } else {
                    self.inbox.push((slot, channel, text));
                }
            }
        }
        let due: Vec<u64> = self.scheduled.range(..=tick).map(|(t, _)| *t).collect();
        let mut applied = Vec::new();
        for t in due {
            let changes = self.scheduled.remove(&t).unwrap_or_default();
            for (slot, action, value) in changes {
                let Ok(a) = self.map.action(&action) else {
                    continue;
                };
                let key = (slot, action.clone());
                let before = self
                    .current
                    .get(&key)
                    .copied()
                    .unwrap_or(ActionValue::default_for(a.kind));
                let after = if a.toggle {
                    match value {
                        ActionValue::Button(true) => ActionValue::Button(!before.is_active()),
                        _ => continue,
                    }
                } else {
                    value
                };
                if after == before {
                    continue;
                }
                if a.kind == ActionKind::Button {
                    if after.is_active() {
                        self.pressed.push(key.clone());
                    } else {
                        self.released.push(key.clone());
                    }
                }
                self.current.insert(key, after);
                let entry = LogEntry {
                    tick,
                    slot,
                    action,
                    value: after,
                };
                if fresh {
                    if let Some((_, f)) = self.recorder.as_mut() {
                        let _ = writeln!(f, "{}", entry.line());
                    }
                    self.log.push(entry.clone());
                }
                applied.push(entry);
            }
        }
        if fresh {
            self.log_through = Some(tick);
        }
        // Only the UI's input: text typed with no line sent, and the
        // wheel (the pointer moving with it, which is a place, not a
        // press). A turn-based game's tick for it takes no turn.
        let wheeled = |name: &str| {
            self.map.action(name).is_ok_and(|a| {
                a.bindings
                    .get("mouse")
                    .is_some_and(|b| b.iter().any(|n| n == "wheel"))
            })
        };
        let ui = |name: &str| {
            wheeled(name)
                || self
                    .map
                    .action(name)
                    .is_ok_and(|a| a.kind == ActionKind::Pointer)
        };
        let typed_line = self.typed_now.values().any(|t| t.contains(['\r', '\n']));
        let something = !self.typed_now.is_empty()
            || applied
                .iter()
                .any(|e| wheeled(&e.action) && e.value.is_active());
        // An action going back to rest (the last turn's tap let go) is no
        // command; any other change is the game's.
        self.ui_only = self.discrete
            && something
            && !typed_line
            && self.inbox.is_empty()
            && applied
                .iter()
                .all(|e| ui(&e.action) || !e.value.is_active());
        applied
    }

    /// Whether this tick's input was only the UI's (characters typed with
    /// no line sent, the wheel) in a turn-based game: `input.actions`
    /// freezes such a tick, so it takes no turn, as a menu's press takes
    /// none (issue 23).
    pub fn ui_only(&self) -> bool {
        self.ui_only
    }

    // ----- the journal: seek and branch (phase 2) ----------------------------

    /// A live command came: if a recorded future was waiting, the run
    /// branches here and that future is dropped, with the log after it.
    fn journal_live(&mut self, now: u64, command: Command) {
        if self.cursor < self.journal.len() {
            self.journal.truncate(self.cursor);
            self.log.retain(|e| e.tick < now);
            self.log_through = self.log.last().map(|e| e.tick).or(now.checked_sub(1));
            self.rewrite_recording();
            self.branched = Some(now);
        }
        self.journal.push(JournalEntry { now, command });
        self.cursor = self.journal.len();
        self.live = true;
    }

    /// Issue a journaled command again, without journaling it.
    fn reissue(&mut self, e: &JournalEntry) {
        let now = e.now;
        match &e.command {
            Command::Inject {
                slot,
                action,
                value,
                frames,
                hold,
            } => {
                let _ = self.inject_raw(now, *slot, action, value.as_ref(), *frames, *hold);
            }
            Command::Script { steps } => {
                let _ = self.script_raw(now, steps);
            }
            Command::Devices { changes } => {
                self.apply_device_changes(now, changes);
            }
            Command::Replay { entries } => {
                let _ = self.replay_raw(entries);
            }
            Command::Bind {
                action,
                device,
                names,
            } => {
                let _ = self.bind_raw(action, device, names.clone());
            }
            Command::Took { binding } => self.took(binding.clone()),
            Command::Message {
                slot,
                channel,
                text,
            } if channel == SAVES_CHANNEL => {
                self.messages
                    .entry(now)
                    .or_default()
                    .push((*slot, channel.clone(), text.clone()));
            }
            Command::Message {
                slot,
                channel,
                text,
            } => {
                let _ = self.message_raw(now, *slot, channel, text);
            }
            Command::Text { slot, text } => {
                let _ = self.text_raw(now, *slot, text);
            }
        }
    }

    fn rewrite_recording(&mut self) {
        let Some((path, _)) = self.recorder.take() else {
            return;
        };
        if self.start_recording(&path).is_ok() {
            if let Some((_, f)) = self.recorder.as_mut() {
                for e in &self.log {
                    let _ = writeln!(f, "{}", e.line());
                }
            }
        }
    }

    /// A copy for a fork (phase 2): the same state, journal, and log, so a
    /// fork replays the same inputs; no recorder, since the file is the
    /// original's.
    pub fn fork_copy(&self) -> InputState {
        InputState {
            map: self.map.clone(),
            discrete: self.discrete,
            scheduled: self.scheduled.clone(),
            current: self.current.clone(),
            pressed: self.pressed.clone(),
            released: self.released.clone(),
            log: self.log.clone(),
            recorder: None,
            last_tick: self.last_tick,
            device_last: self.device_last.clone(),
            releases: self.releases.clone(),
            journal: self.journal.clone(),
            cursor: self.cursor,
            log_through: self.log_through,
            branched: None,
            live: false,
            listening: None,
            rebound: Vec::new(),
            messages: self.messages.clone(),
            inbox: self.inbox.clone(),
            typed_now: self.typed_now.clone(),
            typing: self.typing.clone(),
            typing_applied: self.typing_applied.clone(),
            ui_only: self.ui_only,
            saves_now: self.saves_now.clone(),
            fresh_now: self.fresh_now,
        }
    }

    /// The state now, for a keyframe.
    pub fn snapshot(&self) -> InputSnapshot {
        InputSnapshot {
            scheduled: self.scheduled.clone(),
            current: self.current.clone(),
            pressed: self.pressed.clone(),
            released: self.released.clone(),
            last_tick: self.last_tick,
            device_last: self.device_last.clone(),
            releases: self.releases.clone(),
            cursor: self.cursor,
            map: Some(MapState {
                actions: self
                    .map
                    .actions
                    .iter()
                    .map(|a| (a.name.clone(), a.bindings.clone(), a.toggle))
                    .collect(),
                listening: self.listening.clone(),
            }),
            messages: self.messages.clone(),
        }
    }

    /// Back to a keyframe's state. The commands journaled after it, on this
    /// branch, are issued again as their ticks come, so stepping on
    /// replays the run; a live command branches it. The log keeps what it
    /// has: a replayed tick's changes are already in it.
    pub fn restore(&mut self, s: &InputSnapshot) {
        self.scheduled = s.scheduled.clone();
        self.current = s.current.clone();
        self.pressed = s.pressed.clone();
        self.released = s.released.clone();
        self.last_tick = s.last_tick;
        self.device_last = s.device_last.clone();
        self.releases = s.releases.clone();
        self.messages = s.messages.clone();
        self.inbox.clear();
        self.cursor = s.cursor.min(self.journal.len());
        if let Some(m) = &s.map {
            for (name, bindings, toggle) in &m.actions {
                if let Some(a) = self.map.actions.iter_mut().find(|a| &a.name == name) {
                    a.bindings = bindings.clone();
                    a.toggle = *toggle;
                }
            }
            self.listening = m.listening.clone();
        }
    }

    /// Replace the journal with one read from a replay file and go back to
    /// the snapshot at its start (its journal position counted from the
    /// file's first command): stepping on replays the file's run.
    pub fn load_journal(&mut self, entries: Vec<JournalEntry>, s: &InputSnapshot) {
        self.journal = entries;
        self.cursor = 0;
        self.restore(s);
        self.branched = None;
        self.live = false;
    }

    /// Forget the first `n` journaled commands and the logged changes
    /// before `tick`: a release build's recorder keeps only its window
    /// (§15, phase 2 item 7). The recorded future is never forgotten. A
    /// snapshot taken before counts from the old start; move it with
    /// [`InputSnapshot::forget`] and the same `n`, which this returns.
    pub fn forget(&mut self, n: usize, tick: u64) -> usize {
        let n = n.min(self.cursor);
        self.journal.drain(..n);
        self.cursor -= n;
        self.log.retain(|e| e.tick >= tick);
        n
    }

    /// Take on commands another world was given (an A/B trial's worlds
    /// share their input, phase 2 item 4): they join this journal's
    /// recorded future and are issued as their ticks come.
    pub fn extend_journal(&mut self, entries: &[JournalEntry]) {
        self.journal.extend_from_slice(entries);
    }

    /// Issue now the recorded commands due by `tick`, the next tick to run,
    /// as the tick would: a world switched to mid-frame then takes a live
    /// command without branching away the one that came just before it.
    pub fn issue_due(&mut self, tick: u64) {
        while self.cursor < self.journal.len() && self.journal[self.cursor].now <= tick {
            let e = self.journal[self.cursor].clone();
            self.reissue(&e);
            self.cursor += 1;
        }
    }

    /// Commands recorded after the current point, waiting to be issued again.
    pub fn future(&self) -> &[JournalEntry] {
        &self.journal[self.cursor..]
    }

    /// Every command on this branch, issued or waiting.
    pub fn journal(&self) -> &[JournalEntry] {
        &self.journal
    }

    /// The tick a live command branched the run at, once; `None` after.
    pub fn take_branch(&mut self) -> Option<u64> {
        self.branched.take()
    }

    /// Whether a live command came since the last call. The caller knows
    /// whether the run already went past it (a seek back), which branches
    /// the run even with no recorded command to drop.
    pub fn take_live(&mut self) -> bool {
        std::mem::take(&mut self.live)
    }

    /// Drop the recorded future, as a live command would: an edit or a
    /// room transition between ticks changes what happens next.
    pub fn branch(&mut self, now: u64) {
        if self.cursor < self.journal.len() {
            self.journal.truncate(self.cursor);
            self.log.retain(|e| e.tick < now);
            self.log_through = self.log.last().map(|e| e.tick).or(now.checked_sub(1));
            self.rewrite_recording();
        }
    }

    /// The `Actions` fields for a slot this tick, as `(field, value)`.
    pub fn fields_for(&self, slot: u8) -> Vec<(String, Value)> {
        let mut out = Vec::new();
        for a in &self.map.actions {
            let key = (slot, a.name.clone());
            let v = self
                .current
                .get(&key)
                .copied()
                .unwrap_or(ActionValue::default_for(a.kind));
            match v {
                ActionValue::Button(b) => {
                    out.push((a.name.clone(), Value::Bool(b)));
                    out.push((
                        format!("{}_pressed", a.name),
                        Value::Bool(self.pressed.contains(&key)),
                    ));
                    out.push((
                        format!("{}_released", a.name),
                        Value::Bool(self.released.contains(&key)),
                    ));
                }
                ActionValue::Axis(x) => out.push((a.name.clone(), Value::Float(x as f64))),
                ActionValue::Axis2(x, y) => {
                    out.push((a.name.clone(), Value::Vec2(Vec2::new(x, y))))
                }
            }
        }
        out
    }

    /// An action's value for a slot this tick (its rest value when nothing
    /// set it); `None` for an action the map lacks.
    pub fn value(&self, slot: u8, action: &str) -> Option<ActionValue> {
        let kind = self.map.action(action).ok()?.kind;
        Some(
            self.current
                .get(&(slot, action.to_string()))
                .copied()
                .unwrap_or(ActionValue::default_for(kind)),
        )
    }

    /// Current values of a slot, for `input.state`.
    pub fn state_json(&self, slot: u8) -> Json {
        let mut o = serde_json::Map::new();
        for a in &self.map.actions {
            let v = self
                .current
                .get(&(slot, a.name.clone()))
                .copied()
                .unwrap_or(ActionValue::default_for(a.kind));
            o.insert(a.name.clone(), v.to_json());
        }
        json!({ "slot": slot, "tick": self.last_tick, "actions": o, "pending": self.scheduled.len() })
    }

    /// The log so far.
    pub fn log(&self) -> &[LogEntry] {
        &self.log
    }

    /// Start appending applied changes to `path` (created, truncated).
    pub fn start_recording(&mut self, path: &Path) -> Result<()> {
        if let Some(d) = path.parent() {
            argus_os::fs::create_dir_all(d).map_err(|e| {
                ArgusError::new(
                    codes::BAD_LOG,
                    format!("cannot create {}: {e}", d.display()),
                )
            })?;
        }
        let mut f = std::fs::File::create(path).map_err(|e| {
            ArgusError::new(
                codes::BAD_LOG,
                format!("cannot write {}: {e}", path.display()),
            )
        })?;
        let _ = writeln!(f, "# argus input log: tick\tslot\taction\tvalue");
        self.recorder = Some((path.to_path_buf(), f));
        Ok(())
    }

    /// Stop recording; returns the path written.
    pub fn stop_recording(&mut self) -> Option<PathBuf> {
        self.recorder.take().map(|(p, _)| p)
    }

    /// Where the recorder writes, if on.
    pub fn recording(&self) -> Option<&Path> {
        self.recorder.as_ref().map(|(p, _)| p.as_path())
    }

    /// Whether injects use turn semantics.
    pub fn is_discrete(&self) -> bool {
        self.discrete
    }
}

// ----- on disk (phase 2 item 2: a keyframe in a replay file) ---------------

fn put_value(w: &mut Writer, v: &ActionValue) {
    match *v {
        ActionValue::Button(b) => {
            w.u8(0);
            w.bool(b);
        }
        ActionValue::Axis(x) => {
            w.u8(1);
            w.f32(x);
        }
        ActionValue::Axis2(x, y) => {
            w.u8(2);
            w.f32(x);
            w.f32(y);
        }
    }
}

fn get_value(r: &mut Reader) -> Result<ActionValue> {
    Ok(match r.u8()? {
        0 => ActionValue::Button(r.bool()?),
        1 => ActionValue::Axis(r.f32()?),
        2 => ActionValue::Axis2(r.f32()?, r.f32()?),
        k => return Err(r.bad(format!("action value kind {k}"))),
    })
}

fn put_key(w: &mut Writer, (slot, action): &(u8, String)) {
    w.u8(*slot);
    w.str(action);
}

fn get_key(r: &mut Reader) -> Result<(u8, String)> {
    let slot = r.u8()?;
    Ok((slot, r.str()?))
}

fn put_json(w: &mut Writer, v: &Json) {
    w.str(&v.to_string());
}

fn get_json(r: &mut Reader) -> Result<Json> {
    let s = r.str()?;
    serde_json::from_str(&s).map_err(|e| r.bad(format!("a JSON value: {e}")))
}

impl InputSnapshot {
    /// How far into the journal it was.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// The journal it counts in lost its first `n` commands
    /// ([`InputState::forget`]).
    pub fn forget(&mut self, n: usize) {
        self.cursor = self.cursor.saturating_sub(n);
    }

    /// Write it for a keyframe on disk, its journal position counted from
    /// `base` (the first command the file keeps).
    pub fn encode(&self, w: &mut Writer, base: usize) {
        w.count(self.scheduled.len());
        for (tick, changes) in &self.scheduled {
            w.u64(*tick);
            w.count(changes.len());
            for (slot, action, v) in changes {
                w.u8(*slot);
                w.str(action);
                put_value(w, v);
            }
        }
        w.count(self.current.len());
        for (k, v) in &self.current {
            put_key(w, k);
            put_value(w, v);
        }
        for list in [&self.pressed, &self.released] {
            w.count(list.len());
            for k in list {
                put_key(w, k);
            }
        }
        w.opt(self.last_tick, |w, t| w.u64(t));
        w.count(self.device_last.len());
        for (k, v) in &self.device_last {
            put_key(w, k);
            put_value(w, v);
        }
        w.count(self.releases.len());
        for (k, spans) in &self.releases {
            put_key(w, k);
            w.count(spans.len());
            for (a, b) in spans {
                w.u64(*a);
                w.u64(*b);
            }
        }
        w.count(self.cursor.saturating_sub(base));
        w.opt(self.map.as_ref(), |w, m| {
            w.count(m.actions.len());
            for (name, bindings, toggle) in &m.actions {
                w.str(name);
                w.count(bindings.len());
                for (device, names) in bindings {
                    w.str(device);
                    w.count(names.len());
                    for n in names {
                        w.str(n);
                    }
                }
                w.bool(*toggle);
            }
            w.opt(m.listening.as_ref(), |w, l| {
                w.str(&l.action);
                w.str(&l.device);
                w.u64(l.entity.to_bits());
            });
        });
        w.count(self.messages.len());
        for (tick, list) in &self.messages {
            w.u64(*tick);
            w.count(list.len());
            for (slot, channel, text) in list {
                w.u8(*slot);
                w.str(channel);
                w.str(text);
            }
        }
    }

    /// Read what [`InputSnapshot::encode`] wrote; `with_map` for a file
    /// that has the map's state (replay files from version 4),
    /// `with_messages` for one that has the messages not yet due (from
    /// version 7).
    pub fn decode_as(r: &mut Reader, with_map: bool, with_messages: bool) -> Result<InputSnapshot> {
        let mut scheduled = BTreeMap::new();
        for _ in 0..r.count()? {
            let tick = r.u64()?;
            let mut changes = Vec::new();
            for _ in 0..r.count()? {
                let slot = r.u8()?;
                let action = r.str()?;
                changes.push((slot, action, get_value(r)?));
            }
            scheduled.insert(tick, changes);
        }
        let mut current = BTreeMap::new();
        for _ in 0..r.count()? {
            let k = get_key(r)?;
            current.insert(k, get_value(r)?);
        }
        let mut lists = [Vec::new(), Vec::new()];
        for list in &mut lists {
            for _ in 0..r.count()? {
                list.push(get_key(r)?);
            }
        }
        let [pressed, released] = lists;
        let last_tick = r.opt(|r| r.u64())?;
        let mut device_last = BTreeMap::new();
        for _ in 0..r.count()? {
            let k = get_key(r)?;
            device_last.insert(k, get_value(r)?);
        }
        let mut releases = BTreeMap::new();
        for _ in 0..r.count()? {
            let k = get_key(r)?;
            let mut spans = Vec::new();
            for _ in 0..r.count()? {
                spans.push((r.u64()?, r.u64()?));
            }
            releases.insert(k, spans);
        }
        let cursor = r.u32()? as usize;
        let map = if with_map {
            r.opt(|r| {
                let mut actions = Vec::new();
                for _ in 0..r.count()? {
                    let name = r.str()?;
                    let mut bindings = BTreeMap::new();
                    for _ in 0..r.count()? {
                        let device = r.str()?;
                        let names = (0..r.count()?).map(|_| r.str()).collect::<Result<_>>()?;
                        bindings.insert(device, names);
                    }
                    actions.push((name, bindings, r.bool()?));
                }
                let listening = r.opt(|r| {
                    Ok(Listen {
                        action: r.str()?,
                        device: r.str()?,
                        entity: argus_ecs::Entity::from_bits(r.u64()?),
                    })
                })?;
                Ok(MapState { actions, listening })
            })?
        } else {
            None
        };
        let mut messages = BTreeMap::new();
        if with_messages {
            for _ in 0..r.count()? {
                let tick = r.u64()?;
                let mut list = Vec::new();
                for _ in 0..r.count()? {
                    let slot = r.u8()?;
                    let channel = r.str()?;
                    list.push((slot, channel, r.str()?));
                }
                messages.insert(tick, list);
            }
        }
        Ok(InputSnapshot {
            scheduled,
            current,
            pressed,
            released,
            last_tick,
            device_last,
            releases,
            cursor,
            map,
            messages,
        })
    }
}

impl JournalEntry {
    /// Write it for a replay file.
    pub fn encode(&self, w: &mut Writer) {
        w.u64(self.now);
        match &self.command {
            Command::Inject {
                slot,
                action,
                value,
                frames,
                hold,
            } => {
                w.u8(0);
                w.u8(*slot);
                w.str(action);
                w.opt(value.as_ref(), put_json);
                w.opt(*frames, |w, f| w.u64(f));
                w.bool(*hold);
            }
            Command::Script { steps } => {
                w.u8(1);
                w.count(steps.len());
                for s in steps {
                    put_json(w, s);
                }
            }
            Command::Devices { changes } => {
                w.u8(2);
                w.count(changes.len());
                for (slot, action, v) in changes {
                    w.u8(*slot);
                    w.str(action);
                    put_value(w, v);
                }
            }
            Command::Replay { entries } => {
                w.u8(3);
                w.count(entries.len());
                for e in entries {
                    w.u64(e.tick);
                    w.u8(e.slot);
                    w.str(&e.action);
                    put_value(w, &e.value);
                }
            }
            Command::Bind {
                action,
                device,
                names,
            } => {
                w.u8(4);
                w.str(action);
                w.str(device);
                w.count(names.len());
                for n in names {
                    w.str(n);
                }
            }
            Command::Took { binding } => {
                w.u8(5);
                w.opt(binding.as_deref(), |w, b| w.str(b));
            }
            Command::Message {
                slot,
                channel,
                text,
            } => {
                w.u8(6);
                w.u8(*slot);
                w.str(channel);
                w.str(text);
            }
            Command::Text { slot, text } => {
                w.u8(7);
                w.u8(*slot);
                w.str(text);
            }
        }
    }

    /// Read what [`JournalEntry::encode`] wrote.
    pub fn decode(r: &mut Reader) -> Result<JournalEntry> {
        let now = r.u64()?;
        let command = match r.u8()? {
            0 => {
                let slot = r.u8()?;
                let action = r.str()?;
                Command::Inject {
                    slot,
                    action,
                    value: r.opt(get_json)?,
                    frames: r.opt(|r| r.u64())?,
                    hold: r.bool()?,
                }
            }
            1 => {
                let mut steps = Vec::new();
                for _ in 0..r.count()? {
                    steps.push(get_json(r)?);
                }
                Command::Script { steps }
            }
            2 => {
                let mut changes = Vec::new();
                for _ in 0..r.count()? {
                    let slot = r.u8()?;
                    let action = r.str()?;
                    changes.push((slot, action, get_value(r)?));
                }
                Command::Devices { changes }
            }
            3 => {
                let mut entries = Vec::new();
                for _ in 0..r.count()? {
                    let tick = r.u64()?;
                    let slot = r.u8()?;
                    let action = r.str()?;
                    entries.push(LogEntry {
                        tick,
                        slot,
                        action,
                        value: get_value(r)?,
                    });
                }
                Command::Replay { entries }
            }
            4 => Command::Bind {
                action: r.str()?,
                device: r.str()?,
                names: (0..r.count()?).map(|_| r.str()).collect::<Result<_>>()?,
            },
            5 => Command::Took {
                binding: r.opt(|r| r.str())?,
            },
            6 => Command::Message {
                slot: r.u8()?,
                channel: r.str()?,
                text: r.str()?,
            },
            7 => Command::Text {
                slot: r.u8()?,
                text: r.str()?,
            },
            k => return Err(r.bad(format!("input command kind {k}"))),
        };
        Ok(JournalEntry { now, command })
    }
}
