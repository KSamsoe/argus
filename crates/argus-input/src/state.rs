//! Injected input, the per-tick action values, and the replayable log
//! (§6 "the input log records actions by tick or commands by turn").
//!
//! Every value change is scheduled for a tick, applied when that tick's
//! `input` stage runs, and appended to the log as `tick, slot, action,
//! value`. Replaying a log schedules the same changes at the same ticks,
//! so the same scene, seed, and code give the same run.

use crate::codes;
use crate::map::{ActionKind, ActionMap};
use argus_ecs::{ArgusError, Result, Value, Vec2};
use serde_json::{json, Value as Json};
use std::collections::BTreeMap;
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
            ActionKind::Axis2 => ActionValue::Axis2(0.0, 0.0),
        }
    }

    /// The "active" value of a kind, for an inject without a value: a
    /// button down, an axis at 1, a stick pushed right.
    pub fn active_for(kind: ActionKind) -> ActionValue {
        match kind {
            ActionKind::Button => ActionValue::Button(true),
            ActionKind::Axis => ActionValue::Axis(1.0),
            ActionKind::Axis2 => ActionValue::Axis2(1.0, 0.0),
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
                        ActionKind::Axis => "a number in [-1, 1]",
                        ActionKind::Axis2 => "[x, y] with each in [-1, 1]",
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
            (ActionKind::Axis, Json::Number(n)) => {
                ActionValue::Axis(clamp(n.as_f64().unwrap_or(0.0))?)
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

    fn kind(self) -> ActionKind {
        match self {
            ActionValue::Button(_) => ActionKind::Button,
            ActionValue::Axis(_) => ActionKind::Axis,
            ActionValue::Axis2(..) => ActionKind::Axis2,
        }
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
}

impl InputState {
    /// Fresh state for a map; `discrete` picks turn semantics for injects.
    pub fn new(map: ActionMap, discrete: bool) -> InputState {
        InputState {
            map,
            discrete,
            scheduled: BTreeMap::new(),
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
    pub fn apply_devices(&mut self, now: u64, devices: &crate::devices::Devices) -> usize {
        let slots: Vec<u8> = self.map.slots.iter().map(|s| s.index).collect();
        let mut commands = 0;
        for slot in slots {
            for (action, v) in devices.resolve(&self.map, slot) {
                let Ok(a) = self.map.action(&action) else {
                    continue;
                };
                let (kind, toggle) = (a.kind, a.toggle);
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
                if self.discrete {
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
        let a = self.map.action(action)?.clone();
        self.map.slot(slot)?;
        let v = match value {
            Some(j) => ActionValue::from_json(a.kind, j, action)?,
            None => ActionValue::active_for(a.kind),
        };
        if v.kind() != a.kind {
            return Err(ArgusError::new(
                codes::BAD_VALUE,
                format!("{action} is a {}", a.kind.name()),
            ));
        }
        self.schedule(now, slot, action, v);
        let until = if a.toggle || hold || !v.is_active() {
            None
        } else {
            let n = if self.discrete {
                1
            } else {
                frames.unwrap_or(1).max(1)
            };
            let t = now + n;
            self.schedule(t, slot, action, ActionValue::default_for(a.kind));
            Some(t)
        };
        Ok(InjectReport { tick: now, until })
    }

    /// `input.script`: injects at ticks relative to `now`.
    pub fn script(&mut self, now: u64, steps: &[Json]) -> Result<Vec<InjectReport>> {
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
            out.push(self.inject(now + at, slot, action, s.get("value"), frames, hold)?);
        }
        Ok(out)
    }

    /// Replay a log: every entry becomes a scheduled change at its tick.
    pub fn replay(&mut self, entries: &[LogEntry]) -> Result<usize> {
        for e in entries {
            let a = self.map.action(&e.action)?;
            self.map.slot(e.slot)?;
            if e.value.kind() != a.kind {
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
        let text = std::fs::read_to_string(path).map_err(|e| {
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

    /// Apply everything scheduled up to and including `tick`; called by the
    /// `input.actions` system at the start of each tick. Returns the
    /// entries applied this tick.
    pub fn begin_tick(&mut self, tick: u64) -> Vec<LogEntry> {
        self.pressed.clear();
        self.released.clear();
        self.last_tick = Some(tick);
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
                if let Some((_, f)) = self.recorder.as_mut() {
                    let _ = writeln!(f, "{}", entry.line());
                }
                self.log.push(entry.clone());
                applied.push(entry);
            }
        }
        applied
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
            std::fs::create_dir_all(d).map_err(|e| {
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
