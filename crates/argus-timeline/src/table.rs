//! The `timeline` def table: its rows read once into timelines and their
//! cues, sorted by time.
//!
//! A row without `timeline` is a timeline (its `length`, or the end of its
//! last cue). A row with `timeline = "timeline:outro"` is a cue on it:
//! `at` seconds in, a `kind`, on a `track` (the lane an editor draws it in;
//! its kind when left out), about `who` (`actor`, the default; `other`;
//! `camera`, the one drawing; or an authored `@id`).
//!
//! | kind | does | fields |
//! |---|---|---|
//! | `tween` | starts a tween of who's field | `path`, `to`, `from` (left out: from where it is), `duration`, `easing` |
//! | `anim` | sends who's animator to a state | `state` |
//! | `camera` | makes who, a camera, draw | `duration` (0: to the end) |
//! | `line` | starts a conversation, who speaking to the actor (or the other, when who is the actor) | `line`, a dialogue node |
//! | `event` | makes edits and emits events | `effects` |
//!
//! Any cue may have a `when` (it is passed over unless the condition
//! holds) and a `sound` (played from who when it fires); `wait` holds the
//! timeline until the cue is done (a line's conversation ended, a tween
//! finished, an anim's state played through) or, on an event, until its
//! `when` holds. Conditions and effects are the dialogue's language, their
//! parties `actor`, `other`, and `camera`.

use argus_dialogue::graph::{parse_cond_with, parse_effects_with, Cond, Effect, Test};
use argus_ecs::{Registry, Value, Vec2};
use argus_scene::{DefSchema, DefTable, Defs};
use std::collections::BTreeMap;

/// The table's name and reference prefix.
pub const TABLE: &str = "timeline";

/// The parties a timeline's conditions and effects name, in order: the
/// actor (the default), the other, and the camera drawing.
pub const ROLES: &[&str] = &["actor", "other", "camera"];

/// The `timeline` def table's schema: declared by the engine, so every
/// project has the table and `world.defs {table: "timeline"}` answers it.
pub fn schema() -> DefSchema {
    use argus_ecs::{FieldType, SchemaBuilder};
    let f = |b: SchemaBuilder,
             name: &str,
             ty: FieldType,
             default: Value,
             range: Option<(f64, f64)>,
             doc: &str| { b.field_full(name, ty, default, range, doc, false) };
    let s = |b: SchemaBuilder, name: &str, doc: &str| {
        f(
            b,
            name,
            FieldType::Str,
            Value::Str(String::new()),
            None,
            doc,
        )
    };
    let mut b = SchemaBuilder::new(TABLE)
        .doc("Timelines (section 13): a row without timeline is a timeline; a row with one is a cue on it, at seconds in, of a kind (tween, anim, camera, line, event), about who (actor, other, camera, or @id)")
        .source("argus-timeline");
    b = s(b, "id", "The row's key (its header)");
    b = f(
        b,
        "timeline",
        FieldType::Ref,
        Value::Ref(String::new()),
        None,
        "The timeline this cue is on (timeline:outro); empty: this row is a timeline",
    );
    b = f(
        b,
        "length",
        FieldType::F32,
        Value::Float(0.0),
        Some((0.0, 3600.0)),
        "A timeline's length in seconds; 0: when its last cue ends",
    );
    b = s(
        b,
        "track",
        "The lane the cue is drawn in (an editor's); empty: its kind",
    );
    b = f(
        b,
        "at",
        FieldType::F32,
        Value::Float(0.0),
        Some((0.0, 3600.0)),
        "Seconds from the timeline's start when the cue fires",
    );
    b = s(
        b,
        "kind",
        "What the cue does: tween, anim, camera, line, or event",
    );
    b = s(
        b,
        "who",
        "Whose: actor (the default), other, camera (the one drawing), or an authored @id",
    );
    b = s(
        b,
        "path",
        "A tween's field: Component.field (a number or vec2) or Component.vec.x",
    );
    b = f(
        b,
        "from",
        FieldType::Vec2,
        Value::Vec2(Vec2::ZERO),
        None,
        "A tween's start value (x for a number); left out, from where the field is when the cue fires",
    );
    b = f(
        b,
        "to",
        FieldType::Vec2,
        Value::Vec2(Vec2::ZERO),
        None,
        "A tween's end value (x for a number)",
    );
    b = f(
        b,
        "duration",
        FieldType::F32,
        Value::Float(0.0),
        Some((0.0, 3600.0)),
        "A tween's seconds; a camera cut's (0: until the timeline ends)",
    );
    b = s(b, "easing", "A tween's easing curve (linear when empty)");
    b = s(b, "state", "An anim cue's animator state");
    b = f(
        b,
        "line",
        FieldType::Ref,
        Value::Ref(String::new()),
        None,
        "A line cue's dialogue node, where the conversation starts (dialogue:hermit.hello)",
    );
    b = s(
        b,
        "when",
        "A condition on the world: the cue is passed over unless it holds (with wait, waited for)",
    );
    b = s(
        b,
        "effects",
        "An event cue's edits: path = value; path += n; emit event (parties actor, other, camera)",
    );
    b = s(b, "sound", "A sound played from who when the cue fires");
    b = f(
        b,
        "wait",
        FieldType::Bool,
        Value::Bool(false),
        None,
        "Hold the timeline until the cue is done: its conversation ended, its tween finished, its animator state played through; on an event, until its when holds",
    );
    DefSchema::new(b.build().expect("the timeline schema is valid"))
        .key("id")
        .prefix(TABLE)
}

/// What a cue does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Starts a tween.
    Tween,
    /// Sends an animator to a state.
    Anim,
    /// Makes a camera draw.
    Camera,
    /// Starts a conversation.
    Line,
    /// Makes edits and emits events.
    Event,
}

impl Kind {
    /// Its name in the table.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Tween => "tween",
            Kind::Anim => "anim",
            Kind::Camera => "camera",
            Kind::Line => "line",
            Kind::Event => "event",
        }
    }
    fn parse(s: &str) -> Option<Kind> {
        Some(match s {
            "tween" => Kind::Tween,
            "anim" => Kind::Anim,
            "camera" => Kind::Camera,
            "line" => Kind::Line,
            "event" => Kind::Event,
            _ => return None,
        })
    }
}

/// Whom a cue is about.
#[derive(Clone, Debug, PartialEq)]
pub enum Who {
    /// One of the timeline's parties, by its place in [`ROLES`].
    Role(usize),
    /// An authored entity (`@id`).
    Id(String),
}

/// One cue.
#[derive(Clone, Debug)]
pub struct Cue {
    /// Its row's key.
    pub key: String,
    /// Its lane.
    pub track: String,
    /// Seconds in.
    pub at: f32,
    /// What it does.
    pub kind: Kind,
    /// Whom.
    pub who: Who,
    /// A tween's field.
    pub path: String,
    /// A tween's start, if given.
    pub from: Option<Vec2>,
    /// A tween's end.
    pub to: Vec2,
    /// A tween's or a cut's seconds.
    pub duration: f32,
    /// A tween's easing.
    pub easing: String,
    /// An anim cue's state.
    pub state: String,
    /// A line cue's dialogue node (no prefix).
    pub line: String,
    /// Its condition.
    pub when: Option<Cond>,
    /// An event's edits.
    pub effects: Vec<Effect>,
    /// A sound.
    pub sound: String,
    /// Hold until done.
    pub wait: bool,
    /// `file:line` of its row.
    pub at_ref: String,
}

impl Cue {
    /// Where it ends: a tween's or a timed cut's end, else when it fires.
    pub fn end(&self) -> f32 {
        match self.kind {
            Kind::Tween | Kind::Camera => self.at + self.duration,
            _ => self.at,
        }
    }
}

/// One timeline.
#[derive(Clone, Debug)]
pub struct Line {
    /// Its row's key.
    pub key: String,
    /// Seconds: its row's length, or its last cue's end.
    pub length: f32,
    /// Its cues by time (rows at one time in file order).
    pub cues: Vec<Cue>,
    /// `file:line` of its row.
    pub at_ref: String,
}

impl Line {
    /// Its tracks in order of first appearance, with their cues' places.
    pub fn tracks(&self) -> Vec<(String, Vec<usize>)> {
        let mut out: Vec<(String, Vec<usize>)> = Vec::new();
        for (i, c) in self.cues.iter().enumerate() {
            match out.iter_mut().find(|t| t.0 == c.track) {
                Some(t) => t.1.push(i),
                None => out.push((c.track.clone(), vec![i])),
            }
        }
        out
    }
}

/// Every timeline of a project.
#[derive(Clone, Debug, Default)]
pub struct Timelines {
    lines: BTreeMap<String, Line>,
    /// Rows that did not read: `(file:line, message)`.
    pub problems: Vec<(String, String)>,
}

fn unref(r: &str, prefix: &str) -> String {
    let r = r.trim();
    r.strip_prefix(prefix)
        .and_then(|x| x.strip_prefix(':'))
        .unwrap_or(r)
        .to_string()
}

fn text(t: &DefTable, row: &argus_scene::DefRow, f: &str) -> String {
    match t.value(row, f) {
        Some(Value::Str(s) | Value::Ref(s)) => s.clone(),
        _ => String::new(),
    }
}

fn number(t: &DefTable, row: &argus_scene::DefRow, f: &str) -> f32 {
    match t.value(row, f) {
        Some(Value::Float(x)) => *x as f32,
        Some(Value::Int(i)) => *i as f32,
        _ => 0.0,
    }
}

fn vec2(t: &DefTable, row: &argus_scene::DefRow, f: &str) -> Vec2 {
    match t.value(row, f) {
        Some(Value::Vec2(v)) => *v,
        _ => Vec2::ZERO,
    }
}

impl Timelines {
    /// The timelines of a project's `timeline` table (none when it has
    /// none).
    pub fn from_defs(defs: &Defs) -> Timelines {
        let mut out = Timelines::default();
        let Some(t) = defs.table(TABLE) else {
            return out;
        };
        let mut cues: Vec<(String, Cue)> = Vec::new();
        let mut problems: Vec<(String, String)> = Vec::new();
        for row in &t.rows {
            let on = unref(&text(t, row, "timeline"), TABLE);
            let kind_text = text(t, row, "kind");
            let mut bad =
                |msg: String| problems.push((row.at.clone(), format!("{}: {msg}", row.key)));
            if on.is_empty() {
                if !kind_text.is_empty() {
                    bad(format!(
                        "a cue ({kind_text}) needs timeline = \"timeline:...\", the timeline it is on"
                    ));
                    continue;
                }
                out.lines.insert(
                    row.key.clone(),
                    Line {
                        key: row.key.clone(),
                        length: number(t, row, "length"),
                        cues: Vec::new(),
                        at_ref: row.at.clone(),
                    },
                );
                continue;
            }
            let Some(kind) = Kind::parse(kind_text.trim()) else {
                bad(format!(
                    "kind {kind_text:?} is not tween, anim, camera, line, or event"
                ));
                continue;
            };
            let who_text = text(t, row, "who");
            let who = match who_text.trim() {
                "" => Who::Role(0),
                w if w.starts_with('@') && w.len() > 1 => Who::Id(w[1..].to_string()),
                w => match ROLES.iter().position(|r| *r == w) {
                    Some(i) => Who::Role(i),
                    None => {
                        bad(format!(
                            "who {w:?} is not someone: actor, other, camera, or @id"
                        ));
                        continue;
                    }
                },
            };
            let track = text(t, row, "track");
            let mut cue = Cue {
                key: row.key.clone(),
                track: if track.trim().is_empty() {
                    kind.name().to_string()
                } else {
                    track
                },
                at: number(t, row, "at"),
                kind,
                who,
                path: text(t, row, "path"),
                from: t.given(row, "from").then(|| vec2(t, row, "from")),
                to: vec2(t, row, "to"),
                duration: number(t, row, "duration"),
                easing: text(t, row, "easing"),
                state: text(t, row, "state"),
                line: unref(&text(t, row, "line"), "dialogue"),
                when: None,
                effects: Vec::new(),
                sound: text(t, row, "sound"),
                wait: matches!(t.value(row, "wait"), Some(Value::Bool(true))),
                at_ref: row.at.clone(),
            };
            let when = text(t, row, "when");
            if !when.trim().is_empty() {
                match parse_cond_with(&when, ROLES) {
                    Ok(c) => cue.when = Some(c),
                    Err(e) => {
                        bad(format!("when: {e}"));
                        continue;
                    }
                }
            }
            let effects = text(t, row, "effects");
            if !effects.trim().is_empty() {
                match parse_effects_with(&effects, ROLES) {
                    Ok(e) => cue.effects = e,
                    Err(e) => {
                        bad(format!("effects: {e}"));
                        continue;
                    }
                }
            }
            // What each kind needs.
            let missing = match kind {
                Kind::Tween if cue.path.trim().is_empty() => Some("a tween cue needs a path"),
                Kind::Tween
                    if !cue.path.split('.').nth(1).is_some_and(|f| !f.is_empty())
                        || cue.path.split('.').count() > 3 =>
                {
                    Some("a tween's path is Component.field or Component.vec.x")
                }
                Kind::Anim if cue.state.trim().is_empty() => Some("an anim cue needs a state"),
                Kind::Line if cue.line.is_empty() => {
                    Some("a line cue needs a line, the dialogue node it starts at")
                }
                Kind::Event if cue.effects.is_empty() && cue.sound.is_empty() && !cue.wait => {
                    Some("an event cue does nothing without effects, a sound, or a wait")
                }
                Kind::Event if cue.wait && cue.when.is_none() => {
                    Some("an event cue that waits needs a when to wait for")
                }
                Kind::Camera if cue.wait => Some("a camera cue cannot wait; give it a duration"),
                _ => None,
            };
            if let Some(m) = missing {
                bad(m.to_string());
                continue;
            }
            cues.push((on, cue));
        }
        out.problems = problems;
        for (on, cue) in cues {
            match out.lines.get_mut(&on) {
                Some(l) => l.cues.push(cue),
                None => out.problems.push((
                    cue.at_ref.clone(),
                    format!("{}: timeline {on:?} is no timeline row", cue.key),
                )),
            }
        }
        for l in out.lines.values_mut() {
            // Stable: rows at one time keep their file order.
            l.cues.sort_by(|a, b| a.at.total_cmp(&b.at));
            let end = l.cues.iter().map(Cue::end).fold(0.0f32, f32::max);
            if l.length <= 0.0 {
                l.length = end;
            } else {
                for c in &l.cues {
                    if c.at > l.length {
                        out.problems.push((
                            c.at_ref.clone(),
                            format!(
                                "{}: at {} is past the end of {} ({} seconds); it would never fire",
                                c.key, c.at, l.key, l.length
                            ),
                        ));
                    }
                }
            }
        }
        out.problems.sort();
        out
    }

    /// Check the fields the cues name against the components there are
    /// (after scripts have declared theirs): a tween's path, a condition's
    /// and an edit's fields.
    pub fn check(&mut self, registry: &Registry) {
        let mut found = Vec::new();
        for l in self.lines.values() {
            for c in &l.cues {
                let mut paths: Vec<&str> = Vec::new();
                if c.kind == Kind::Tween {
                    let base: Vec<&str> = c.path.split('.').collect();
                    if base.len() >= 2 {
                        paths.push(&c.path[..base[0].len() + 1 + base[1].len()]);
                    }
                }
                if let Some(Cond(any)) = &c.when {
                    for all in any {
                        for Test { path, .. } in all {
                            paths.push(&path.path);
                        }
                    }
                }
                for e in &c.effects {
                    match e {
                        Effect::Set(p, _) | Effect::Add(p, _) => paths.push(&p.path),
                        Effect::Emit(_) => {}
                    }
                }
                for p in paths {
                    if let Err(e) = registry.resolve_path(p) {
                        found.push((c.at_ref.clone(), format!("{}: {}", c.key, e.message)));
                    }
                }
            }
        }
        self.problems.extend(found);
        self.problems.sort();
        self.problems.dedup();
    }

    /// A timeline by name (`outro` or `timeline:outro`).
    pub fn line(&self, name: &str) -> Option<&Line> {
        self.lines.get(&unref(name, TABLE))
    }

    /// Every timeline, by name.
    pub fn lines(&self) -> impl Iterator<Item = &Line> {
        self.lines.values()
    }

    /// How many timelines.
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// None.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}
