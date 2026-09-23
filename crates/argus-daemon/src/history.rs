//! What `obs.diff`, `obs.trace`, and `obs.why` read (§8 L3 and its two
//! helpers).
//!
//! Two bounded records, both cheap enough to keep always:
//!
//! - **Baselines.** A snapshot of the world at each frame the agent looked
//!   (any `world.*` or `obs.*` call), a few at a time. `obs.diff {since}`
//!   compares the world now with the baseline at or before `since`.
//! - **Traces.** For every named entity (and any the agent asks to
//!   watch), the rows of its components each time they change, frame by
//!   frame, for the last `window` frames. The world is read at the end of
//!   every tick by a scheduler observer; a row that did not change costs a
//!   comparison. `obs.trace` rebuilds one entity's fields over a window
//!   from it.
//!
//! The snapshot ring proper, with age thinning and seeking, is phase 2.

use argus_ecs::{ComponentId, Entity, Snapshot, Value, World};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

/// Baselines kept, and the bytes they may hold together.
const BASELINES: usize = 8;
const BASELINE_BYTES: usize = 256 << 20;

/// One change to a traced entity, in frame order.
#[derive(Clone, Debug)]
enum Change {
    /// A component's row as it became (or appeared) this frame.
    Row {
        frame: u64,
        comp: ComponentId,
        row: Vec<u8>,
    },
    /// A component went.
    Removed { frame: u64, comp: ComponentId },
}

impl Change {
    fn frame(&self) -> u64 {
        match self {
            Change::Row { frame, .. } | Change::Removed { frame, .. } => *frame,
        }
    }
}

/// One entity's history.
#[derive(Clone, Debug, Default)]
pub struct Trace {
    /// Its id when it was traced: `@player` or `e:12`.
    pub id: String,
    /// The first frame recorded.
    pub since: u64,
    /// The frame it was found gone, if it has gone.
    pub gone: Option<u64>,
    /// Its rows at the start of what is kept.
    base: BTreeMap<ComponentId, Vec<u8>>,
    /// Its rows as last recorded.
    last: BTreeMap<ComponentId, Vec<u8>>,
    /// Every change since, oldest first.
    changes: VecDeque<Change>,
    bytes: usize,
}

/// The daemon's history of the world.
pub struct History {
    baselines: VecDeque<(u64, Snapshot)>,
    epoch: u64,
    /// The frame the last `obs.diff` answered up to.
    pub last_diff: Option<u64>,
    traces: HashMap<Entity, Trace>,
    watch: HashSet<Entity>,
    /// Frames of changes kept per entity.
    pub window: u64,
    /// Named entities traced at most; past it they are counted in `skipped`.
    pub max_traced: usize,
    /// Named entities not traced at the last tick for the cap.
    pub skipped: usize,
    scratch: Vec<ComponentId>,
}

impl Default for History {
    fn default() -> Self {
        History {
            baselines: VecDeque::new(),
            epoch: 0,
            last_diff: None,
            traces: HashMap::new(),
            watch: HashSet::new(),
            window: 3600,
            max_traced: 1024,
            skipped: 0,
            scratch: Vec::new(),
        }
    }
}

/// An entity's protocol spelling.
pub fn id_of(world: &World, e: Entity) -> String {
    match world.string_id(e) {
        Some(id) => format!("@{id}"),
        None => e.to_string(),
    }
}

impl History {
    // ----- baselines ------------------------------------------------------

    /// The agent looked at the world: keep a baseline for this frame, or
    /// replace this frame's if the world was edited since (`epoch` counts
    /// changes made outside ticks).
    pub fn looked(&mut self, world: &World, epoch: u64) {
        let frame = world.frame();
        match self.baselines.back() {
            Some(b) if b.0 == frame && self.epoch == epoch => return,
            Some(b) if b.0 == frame => {
                self.baselines.pop_back();
            }
            _ => {}
        }
        self.epoch = epoch;
        self.baselines.push_back((frame, world.snapshot()));
        let mut bytes: usize = self.baselines.iter().map(|b| b.1.bytes()).sum();
        while self.baselines.len() > BASELINES
            || (bytes > BASELINE_BYTES && self.baselines.len() > 1)
        {
            if let Some((_, s)) = self.baselines.pop_front() {
                bytes -= s.bytes();
            }
        }
    }

    /// Frames with a baseline, oldest first.
    pub fn baseline_frames(&self) -> Vec<u64> {
        self.baselines.iter().map(|b| b.0).collect()
    }

    /// The newest baseline at or before `frame`.
    pub fn baseline(&self, frame: u64) -> Option<(u64, &World)> {
        self.baselines
            .iter()
            .rev()
            .find(|b| b.0 <= frame)
            .map(|b| (b.0, b.1.world()))
    }

    /// Back to frame `to` (a seek): what was recorded from `to` on is the
    /// future, gone until its ticks run again. What led up to `to` stays,
    /// which is what a trace after a seek to a mark is for.
    pub fn rewind(&mut self, to: u64) {
        self.baselines.retain(|b| b.0 <= to);
        if self.last_diff.is_some_and(|f| f > to) {
            self.last_diff = None;
        }
        self.traces.retain(|_, t| t.since < to);
        for t in self.traces.values_mut() {
            while t.changes.back().is_some_and(|c| c.frame() >= to) {
                if let Some(Change::Row { row, .. }) = t.changes.pop_back() {
                    t.bytes -= row.len();
                }
            }
            if t.gone.is_some_and(|g| g >= to) {
                t.gone = None;
            }
            let mut last = t.base.clone();
            for c in &t.changes {
                match c {
                    Change::Row { comp, row, .. } => {
                        last.insert(*comp, row.clone());
                    }
                    Change::Removed { comp, .. } => {
                        last.remove(comp);
                    }
                }
            }
            t.last = last;
        }
    }

    /// Forget everything: a new scene is a new world.
    pub fn clear(&mut self) {
        *self = History {
            window: self.window,
            max_traced: self.max_traced,
            ..Default::default()
        };
    }

    // ----- traces ---------------------------------------------------------

    /// Trace an entity from now on, named or not.
    pub fn watch(&mut self, e: Entity) {
        self.watch.insert(e);
    }

    /// A traced entity by the id it was traced under (a gone one included).
    pub fn find(&self, id: &str) -> Option<Entity> {
        self.traces
            .iter()
            .find(|(_, t)| t.id == id)
            .map(|(e, _)| *e)
    }

    /// Whether an entity is traced.
    pub fn traced(&self, e: Entity) -> bool {
        self.traces.contains_key(&e)
    }

    /// One entity's history.
    pub fn trace(&self, e: Entity) -> Option<&Trace> {
        self.traces.get(&e)
    }

    /// Entities traced now.
    pub fn traced_count(&self) -> usize {
        self.traces.values().filter(|t| t.gone.is_none()).count()
    }

    /// Read the world at the end of the tick that ran as `frame`: every
    /// named or watched entity's rows that changed.
    pub fn record(&mut self, world: &World, frame: u64) {
        self.skipped = 0;
        if world.named().count() <= self.max_traced {
            for (_, e) in world.named() {
                if world.is_alive(e) {
                    self.record_one(world, e, frame);
                }
            }
        } else {
            // Past the cap, the lowest entities, so the choice is stable.
            let mut named: Vec<Entity> = world.named().map(|(_, e)| e).collect();
            named.sort();
            self.skipped = named.len() - self.max_traced;
            for &e in &named[..self.max_traced] {
                if world.is_alive(e) {
                    self.record_one(world, e, frame);
                }
            }
        }
        let watched: Vec<Entity> = self.watch.iter().copied().collect();
        for e in watched {
            if world.is_alive(e) && world.string_id(e).is_none() {
                self.record_one(world, e, frame);
            }
        }
        // The gone, and what is past the window.
        // Exactly `window` frames: this one and the window - 1 before it.
        let cutoff = (frame + 1).saturating_sub(self.window);
        self.traces.retain(|e, t| {
            if t.gone.is_none() && !world.is_alive(*e) {
                t.gone = Some(frame);
            }
            while t.changes.front().is_some_and(|c| c.frame() < cutoff) {
                let c = t.changes.pop_front().expect("front");
                match c {
                    Change::Row { comp, row, .. } => {
                        t.bytes -= row.len();
                        t.base.insert(comp, row);
                    }
                    Change::Removed { comp, .. } => {
                        t.base.remove(&comp);
                    }
                }
                t.since = t.since.max(cutoff);
            }
            t.gone.is_none_or(|g| g >= cutoff)
        });
        self.watch.retain(|e| world.is_alive(*e));
    }

    fn record_one(&mut self, world: &World, e: Entity, frame: u64) {
        let Ok(comps) = world.components_of(e) else {
            return;
        };
        self.scratch.clear();
        self.scratch.extend_from_slice(comps);
        let t = self.traces.entry(e).or_insert_with(|| Trace {
            id: id_of(world, e),
            since: frame,
            ..Default::default()
        });
        let fresh = t.last.is_empty() && t.changes.is_empty() && t.base.is_empty();
        for &c in &self.scratch {
            let _ = world.with_row(e, c, |row| {
                if t.last.get(&c).is_some_and(|l| l.as_slice() == row) {
                    return;
                }
                if fresh {
                    t.base.insert(c, row.to_vec());
                } else {
                    t.bytes += row.len();
                    t.changes.push_back(Change::Row {
                        frame,
                        comp: c,
                        row: row.to_vec(),
                    });
                }
                t.last.insert(c, row.to_vec());
            });
        }
        let gone: Vec<ComponentId> = t
            .last
            .keys()
            .filter(|c| !self.scratch.contains(c))
            .copied()
            .collect();
        for c in gone {
            t.last.remove(&c);
            t.changes.push_back(Change::Removed { frame, comp: c });
        }
    }
}

// ----- answers --------------------------------------------------------------

/// One field's story over a window.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldStory {
    /// `Component.field`.
    pub path: String,
    /// At the window's start (none if the component was not there).
    pub start: Option<Value>,
    /// At its end.
    pub end: Option<Value>,
    /// Every change: (frame, value), none for the component going.
    pub changes: Vec<(u64, Option<Value>)>,
}

/// What `obs.trace` reads: every field that changed in `[from, to]`, and
/// which components came and went.
pub struct Story {
    /// Fields that changed.
    pub fields: Vec<FieldStory>,
    /// Fields that held still.
    pub steady: usize,
    /// (frame, component, added?)
    pub components: Vec<(u64, String, bool)>,
    /// Components whose fields changed.
    pub changed: Vec<ComponentId>,
}

impl Trace {
    /// Rebuild the fields over `[from, to]`, keeping only those whose
    /// component or path `keep` accepts.
    pub fn story(
        &self,
        world: &World,
        from: u64,
        to: u64,
        keep: &dyn Fn(&str, &str) -> bool,
    ) -> Story {
        // The rows at `from`: the base, then every change up to it.
        let mut rows: BTreeMap<ComponentId, Vec<u8>> = self.base.clone();
        let mut i = 0;
        while let Some(c) = self.changes.get(i) {
            if c.frame() > from {
                break;
            }
            match c {
                Change::Row { comp, row, .. } => {
                    rows.insert(*comp, row.clone());
                }
                Change::Removed { comp, .. } => {
                    rows.remove(comp);
                }
            }
            i += 1;
        }
        let start = rows.clone();
        let reg = world.registry();
        let mut stories: BTreeMap<(ComponentId, usize), FieldStory> = BTreeMap::new();
        let mut components = Vec::new();
        let mut changed: Vec<ComponentId> = Vec::new();
        let value = |c: ComponentId, fi: usize, row: Option<&Vec<u8>>| {
            row.and_then(|r| world.decode_field(&reg.schema(c).fields[fi], r))
        };
        while let Some(ch) = self.changes.get(i) {
            if ch.frame() > to {
                break;
            }
            let (frame, comp, new) = match ch {
                Change::Row { frame, comp, row } => (*frame, *comp, Some(row)),
                Change::Removed { frame, comp } => (*frame, *comp, None),
            };
            let schema = reg.schema(comp);
            let old = rows.get(&comp).cloned();
            if old.is_none() != new.is_none() {
                components.push((frame, schema.name.clone(), new.is_some()));
            }
            for (fi, f) in schema.fields.iter().enumerate() {
                if !keep(&schema.name, &format!("{}.{}", schema.name, f.name)) {
                    continue;
                }
                let (a, b) = (value(comp, fi, old.as_ref()), value(comp, fi, new));
                if a == b {
                    continue;
                }
                let s = stories.entry((comp, fi)).or_insert_with(|| FieldStory {
                    path: format!("{}.{}", schema.name, f.name),
                    start: value(comp, fi, start.get(&comp)),
                    end: None,
                    changes: Vec::new(),
                });
                s.changes.push((frame, b));
                if !changed.contains(&comp) {
                    changed.push(comp);
                }
            }
            match new {
                Some(r) => rows.insert(comp, r.clone()),
                None => rows.remove(&comp),
            };
            i += 1;
        }
        let mut total = 0;
        let mut seen: HashSet<ComponentId> = HashSet::new();
        for &c in rows.keys().chain(start.keys()) {
            if seen.insert(c) {
                total += reg.schema(c).fields.len();
            }
        }
        for ((c, fi), s) in stories.iter_mut() {
            s.end = value(*c, *fi, rows.get(c));
        }
        let steady = total.saturating_sub(stories.len());
        Story {
            fields: stories.into_values().collect(),
            steady,
            components,
            changed,
        }
    }
}

/// What changed between two worlds (§8 L3).
pub struct Diff {
    /// Entities in `now` and not `then`.
    pub appeared: Vec<Entity>,
    /// Entities in `then` and not `now`.
    pub vanished: Vec<Entity>,
    /// Entities in both that changed.
    pub changed: Vec<Changed>,
}

/// One entity that changed between two worlds.
#[derive(Clone, Debug, PartialEq)]
pub struct Changed {
    /// The entity.
    pub entity: Entity,
    /// Each field that changed: path, from, to.
    pub fields: Vec<(String, Value, Value)>,
    /// Components it gained.
    pub added: Vec<String>,
    /// Components it lost.
    pub removed: Vec<String>,
}

/// Compare two worlds entity by entity and field by field.
pub fn diff(then: &World, now: &World) -> Diff {
    let mut appeared = Vec::new();
    let mut changed = Vec::new();
    for e in now.live() {
        if !then.is_alive(e) {
            appeared.push(e);
            continue;
        }
        let (Ok(a), Ok(b)) = (then.components_of(e), now.components_of(e)) else {
            continue;
        };
        let reg = now.registry();
        let mut fields = Vec::new();
        let added: Vec<String> = b
            .iter()
            .filter(|c| !a.contains(c))
            .map(|&c| reg.schema(c).name.clone())
            .collect();
        let removed: Vec<String> = a
            .iter()
            .filter(|c| !b.contains(c))
            .map(|&c| reg.schema(c).name.clone())
            .collect();
        for &c in b.iter().filter(|c| a.contains(c)) {
            let old = then.with_row(e, c, |r| r.to_vec()).unwrap_or_default();
            let same = now.with_row(e, c, |r| r == old.as_slice()).unwrap_or(true);
            if same {
                continue;
            }
            let schema = reg.schema(c);
            let new = now.with_row(e, c, |r| r.to_vec()).unwrap_or_default();
            for f in &schema.fields {
                let range = f.offset..f.offset + f.ty.size();
                if old.get(range.clone()) != new.get(range) {
                    if let (Some(x), Some(y)) =
                        (then.decode_field(f, &old), now.decode_field(f, &new))
                    {
                        fields.push((format!("{}.{}", schema.name, f.name), x, y));
                    }
                }
            }
        }
        if !fields.is_empty() || !added.is_empty() || !removed.is_empty() {
            changed.push(Changed {
                entity: e,
                fields,
                added,
                removed,
            });
        }
    }
    let vanished = then.live().filter(|e| !now.is_alive(*e)).collect();
    Diff {
        appeared,
        vanished,
        changed,
    }
}
