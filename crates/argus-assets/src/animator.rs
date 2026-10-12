//! Animator state machines as assets (§13 "Sprite animation", phase 1
//! item 8): a TOML file with `type = "animator"` beside the sprites.
//!
//! ```toml
//! # assets/anim/hero.toml
//! type = "animator"
//! initial = "idle"
//! face = "KinematicBody.velocity.x" # mirrored while this is below 0
//!
//! [states.idle]
//! clip = "idle"                  # a clip of the entity's sprite
//!
//! [[states.idle.transitions]]
//! to = "run"
//! when = "KinematicBody.grounded and abs(KinematicBody.velocity.x) > 0.5"
//! blend = 0.05                   # seconds of crossfade
//!
//! [states.run]
//! clip = "run"
//! fps = "HeroFeel.run_fps"       # a number, or a field (a tune field)
//! events = [{ frame = 1, name = "footstep", sound = "sfx/step" }]
//!
//! [states.slash]
//! clip = "slash"
//! loop = false
//! windup = 2                     # the first two frames are the wind-up art
//! anticipation = 4               # it lasts four frames at the clip's rate
//! hold = 2                       # the hold frame (default: the last) holds 2 more
//! events = [{ frame = 3, name = "hitbox.on" }]
//!
//! [[states.slash.transitions]]
//! to = "idle"
//! when = "end"                   # also time, frame, loops
//! window = [3, 5]                # only while the clip is on frames 3 to 5
//!
//! [[any]]                        # from every state, checked first
//! to = "hurt"
//! when = "Health.stun > 0"
//! ```
//!
//! Conditions are field comparisons, never script (§17): terms joined by
//! `and` and `or` (`and` binds tighter, as in a dialogue's conditions),
//! each `[not] operand [op value]` where the operand is a field path
//! (`Component.field`, `.x`/`.y` into a vec2, optionally `abs(...)`) or
//! `end`, `time`, `frame`, `loops`, and `op` is `<`, `<=`, `>`, `>=`,
//! `==`, `!=`. A term with no comparison is true when the value is nonzero.
//! This module checks the syntax; `argus-anim` resolves the fields against
//! a world's components.
//!
//! `face` names a field the sprite faces by: below zero it is mirrored
//! (`Sprite.flip_x`), above zero it is not, and at zero it keeps the way
//! it faced, so a hero who stops still faces the way they walked. The art
//! is drawn facing right.

use crate::codes;
use argus_ecs::{ArgusError, Result};
use toml_edit::{ImDocument, Item, Value as TV};

/// A number, or a field path to read one from each tick (so a game can
/// make it a tune field).
#[derive(Clone, Debug, PartialEq)]
pub enum Num {
    /// A constant.
    Value(f32),
    /// `Component.field`, read from the animated entity.
    Field(String),
}

/// What a term reads.
#[derive(Clone, Debug, PartialEq)]
pub enum Operand {
    /// A field of the entity: `Component.field`, `Component.vec.x`.
    Field {
        /// The path.
        path: String,
        /// `abs(path)`.
        abs: bool,
    },
    /// The state's clip has played through once.
    End,
    /// Seconds in the state.
    Time,
    /// Position in the state's clip, 0 first.
    Frame,
    /// Times the clip has looped.
    Loops,
}

/// A comparison.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
    /// `==`
    Eq,
    /// `!=`
    Ne,
}

impl Op {
    /// Apply.
    pub fn test(self, a: f32, b: f32) -> bool {
        match self {
            Op::Lt => a < b,
            Op::Le => a <= b,
            Op::Gt => a > b,
            Op::Ge => a >= b,
            Op::Eq => a == b,
            Op::Ne => a != b,
        }
    }
}

/// One `[not] operand [op value]`.
#[derive(Clone, Debug, PartialEq)]
pub struct Term {
    /// Negated.
    pub not: bool,
    /// What it reads.
    pub what: Operand,
    /// The comparison; none means "nonzero".
    pub cmp: Option<(Op, f32)>,
}

/// Groups of terms joined by `or`: the condition holds when every term of
/// some group does.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Condition {
    /// The groups, each its terms joined by `and`.
    pub groups: Vec<Vec<Term>>,
    /// As written.
    pub text: String,
}

/// A transition.
#[derive(Clone, Debug, PartialEq)]
pub struct TransitionSpec {
    /// Target state.
    pub to: String,
    /// When.
    pub when: Condition,
    /// Clip positions it may fire between, inclusive.
    pub window: Option<[u32; 2]>,
    /// Crossfade seconds.
    pub blend: f32,
    /// `file:line`.
    pub at: String,
}

/// An event at a frame of a state's clip.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameEventSpec {
    /// Position in the clip, 0 first.
    pub frame: u32,
    /// Event name (`footstep`, `hitbox.on`).
    pub name: String,
    /// A sound to ask the mixer for (`sfx/step`).
    pub sound: Option<String>,
    /// `file:line`.
    pub at: String,
}

/// One state.
#[derive(Clone, Debug, PartialEq)]
pub struct StateSpec {
    /// Name.
    pub name: String,
    /// Clip name in the entity's sprite.
    pub clip: String,
    /// Loop the clip; `None` uses the clip's own setting.
    pub looping: Option<bool>,
    /// Frames per second; `None` uses the clip's.
    pub fps: Option<Num>,
    /// Frames at the start of the clip that are the wind-up art.
    pub windup: u32,
    /// How many frames (at the clip's rate) the wind-up lasts.
    pub anticipation: Option<Num>,
    /// Extra frames the hold frame is held.
    pub hold: Option<Num>,
    /// Which clip position is held; `None` is the last.
    pub hold_frame: Option<u32>,
    /// Events by frame.
    pub events: Vec<FrameEventSpec>,
    /// Transitions, in order.
    pub transitions: Vec<TransitionSpec>,
    /// `file:line`.
    pub at: String,
}

/// An imported state machine.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct AnimatorSpec {
    /// The first state.
    pub initial: String,
    /// States, in file order.
    pub states: Vec<StateSpec>,
    /// Transitions from every state, checked before the state's own.
    pub any: Vec<TransitionSpec>,
    /// The field the sprite faces by (mirrored below zero).
    pub face: Option<String>,
    /// `file:line` of `face`.
    pub face_at: String,
    /// The sidecar, `assets/...`.
    pub file: String,
}

impl AnimatorSpec {
    /// A state by name.
    pub fn state(&self, name: &str) -> Option<&StateSpec> {
        self.states.iter().find(|s| s.name == name)
    }
}

const HINT: &str = "An animator is type = \"animator\", initial = \"<state>\", face = \"<Component.field>\" (mirrored below 0), and [states.<name>] with clip, loop, fps, windup, anticipation, hold, hold_frame, events, and [[states.<name>.transitions]] with to, when, window, blend; [[any]] holds transitions from every state.";

fn bad(file: &str, line: u32, msg: impl Into<String>) -> ArgusError {
    ArgusError::new(codes::BAD_SIDECAR, msg.into())
        .hint(HINT)
        .at(Some(format!("{file}:{line}")))
}

fn line_of(text: &str, item: &Item) -> u32 {
    let at = item.span().map(|s| s.start).unwrap_or(0).min(text.len());
    text[..at].matches('\n').count() as u32 + 1
}

fn value_line(text: &str, v: &TV) -> u32 {
    let at = v.span().map(|s| s.start).unwrap_or(0).min(text.len());
    text[..at].matches('\n').count() as u32 + 1
}

/// Parse a condition. `Err` is a sentence saying what is wrong.
pub fn parse_condition(text: &str) -> std::result::Result<Condition, String> {
    let toks = tokenize(text)?;
    if toks.is_empty() {
        return Err(
            "when is empty; write a condition such as \"end\" or \"KinematicBody.grounded\"".into(),
        );
    }
    let mut groups = Vec::new();
    let mut terms = Vec::new();
    let mut i = 0;
    loop {
        let mut not = false;
        if toks.get(i).map(String::as_str) == Some("not") {
            not = true;
            i += 1;
        }
        let what = match toks.get(i).map(String::as_str) {
            None => return Err(format!("{text:?} ends where a value was expected")),
            Some("abs") => {
                if toks.get(i + 1).map(String::as_str) != Some("(")
                    || toks.get(i + 3).map(String::as_str) != Some(")")
                {
                    return Err(format!(
                        "{text:?}: abs takes one field, abs(Component.field)"
                    ));
                }
                let p = toks[i + 2].clone();
                check_path(&p)?;
                i += 4;
                Operand::Field { path: p, abs: true }
            }
            Some("end") => {
                i += 1;
                Operand::End
            }
            Some("time") => {
                i += 1;
                Operand::Time
            }
            Some("frame") => {
                i += 1;
                Operand::Frame
            }
            Some("loops") => {
                i += 1;
                Operand::Loops
            }
            Some(p) => {
                check_path(p)?;
                i += 1;
                Operand::Field {
                    path: p.to_string(),
                    abs: false,
                }
            }
        };
        let op = match toks.get(i).map(String::as_str) {
            Some("<") => Some(Op::Lt),
            Some("<=") => Some(Op::Le),
            Some(">") => Some(Op::Gt),
            Some(">=") => Some(Op::Ge),
            Some("==") => Some(Op::Eq),
            Some("!=") => Some(Op::Ne),
            _ => None,
        };
        let cmp = match op {
            Some(op) => {
                let v = toks
                    .get(i + 1)
                    .ok_or_else(|| format!("{text:?}: a comparison needs a value"))?;
                let n = match v.as_str() {
                    "true" => 1.0,
                    "false" => 0.0,
                    s => s
                        .parse::<f32>()
                        .map_err(|_| format!("{text:?}: {s:?} is not a number, true, or false"))?,
                };
                i += 2;
                Some((op, n))
            }
            None => None,
        };
        terms.push(Term { not, what, cmp });
        match toks.get(i).map(String::as_str) {
            None => break,
            Some("and") => i += 1,
            Some("or") => {
                groups.push(std::mem::take(&mut terms));
                i += 1;
            }
            Some(t) => {
                return Err(format!(
                    "{text:?}: expected and or or after a term, found {t:?}"
                ))
            }
        }
    }
    groups.push(terms);
    Ok(Condition {
        groups,
        text: text.to_string(),
    })
}

fn check_path(p: &str) -> std::result::Result<(), String> {
    let parts: Vec<&str> = p.split('.').collect();
    let word = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if parts.len() < 2 || parts.len() > 3 || !parts.iter().all(|s| word(s)) {
        return Err(format!(
            "{p:?} is not a field path; write Component.field (or Component.vec.x)"
        ));
    }
    if parts.len() == 3 && parts[2] != "x" && parts[2] != "y" {
        return Err(format!("{p:?}: the third part picks .x or .y of a vec2"));
    }
    Ok(())
}

fn tokenize(text: &str) -> std::result::Result<Vec<String>, String> {
    let cs: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '(' || c == ')' {
            out.push(c.to_string());
            i += 1;
        } else if "<>=!".contains(c) {
            let two = cs.get(i + 1) == Some(&'=');
            let t: String = if two {
                [c, '='].iter().collect()
            } else {
                c.to_string()
            };
            if t == "=" || t == "!" {
                return Err(format!("{text:?}: {t:?} is not an operator; use == or !="));
            }
            out.push(t);
            i += if two { 2 } else { 1 };
        } else if c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-' {
            let s = i;
            while i < cs.len()
                && (cs[i].is_ascii_alphanumeric()
                    || cs[i] == '_'
                    || cs[i] == '.'
                    || (cs[i] == '-' && i == s))
            {
                i += 1;
            }
            out.push(cs[s..i].iter().collect());
        } else {
            return Err(format!("{text:?}: unexpected {c:?}"));
        }
    }
    Ok(out)
}

fn num_of(file: &str, text: &str, item: &Item, key: &str) -> Result<Option<Num>> {
    match item.as_value() {
        None => Ok(None),
        Some(TV::Integer(i)) => Ok(Some(Num::Value(*i.value() as f32))),
        Some(TV::Float(f)) => Ok(Some(Num::Value(*f.value() as f32))),
        Some(TV::String(s)) => {
            let p = s.value().to_string();
            check_path(&p).map_err(|m| bad(file, line_of(text, item), format!("{key}: {m}")))?;
            Ok(Some(Num::Field(p)))
        }
        Some(_) => Err(bad(
            file,
            line_of(text, item),
            format!("{key} is a number or a field path such as \"Feel.run_fps\""),
        )),
    }
}

fn transitions(file: &str, text: &str, item: &Item, what: &str) -> Result<Vec<TransitionSpec>> {
    let mut out = Vec::new();
    let Some(arr) = item.as_array_of_tables() else {
        return Err(bad(
            file,
            line_of(text, item),
            format!("{what} is an array of tables: [[{what}]] with to and when"),
        ));
    };
    for t in arr.iter() {
        let at_line = t
            .get("to")
            .or_else(|| t.get("when"))
            .map(|i| line_of(text, i))
            .unwrap_or_else(|| line_of(text, item));
        for (k, _) in t.iter() {
            if !["to", "when", "window", "blend"].contains(&k) {
                return Err(bad(
                    file,
                    at_line,
                    format!(
                        "unknown transition key {k:?}; transitions have to, when, window, blend"
                    ),
                ));
            }
        }
        let to = t
            .get("to")
            .and_then(|v| v.as_str())
            .ok_or_else(|| bad(file, at_line, "a transition needs to = \"<state>\""))?
            .to_string();
        let when_item = t.get("when").ok_or_else(|| {
            bad(
                file,
                at_line,
                format!("the transition to {to:?} needs when = \"<condition>\""),
            )
        })?;
        let when_text = when_item
            .as_str()
            .ok_or_else(|| bad(file, line_of(text, when_item), "when is a string condition"))?;
        let when =
            parse_condition(when_text).map_err(|m| bad(file, line_of(text, when_item), m))?;
        let window = match t.get("window") {
            None => None,
            Some(w) => {
                let v: Vec<i64> = w
                    .as_array()
                    .map(|a| a.iter().filter_map(|x| x.as_integer()).collect())
                    .unwrap_or_default();
                if v.len() != 2 || v[0] < 0 || v[1] < v[0] {
                    return Err(bad(
                        file,
                        line_of(text, w),
                        "window is [first, last], clip positions from 0",
                    ));
                }
                Some([v[0] as u32, v[1] as u32])
            }
        };
        let blend = match t.get("blend") {
            None => 0.0,
            Some(b) => b
                .as_float()
                .or_else(|| b.as_integer().map(|i| i as f64))
                .filter(|x| *x >= 0.0)
                .ok_or_else(|| {
                    bad(
                        file,
                        line_of(text, b),
                        "blend is seconds of crossfade, 0 or more",
                    )
                })? as f32,
        };
        out.push(TransitionSpec {
            to,
            when,
            window,
            blend,
            at: format!("{file}:{at_line}"),
        });
    }
    Ok(out)
}

/// Parse an animator sidecar. Structural problems are errors with
/// `file:line`; unknown top-level keys come back as warnings.
pub fn parse_animator(file: &str, text: &str) -> Result<(AnimatorSpec, Vec<(String, u32)>)> {
    let doc = ImDocument::parse(text.to_string()).map_err(|e| {
        let line = e
            .span()
            .map(|s| text[..s.start.min(text.len())].matches('\n').count() as u32 + 1)
            .unwrap_or(1);
        bad(file, line, format!("not valid TOML: {}", e.message()))
    })?;
    let mut warnings = Vec::new();
    for (k, i) in doc.as_table().iter() {
        if !["type", "initial", "face", "states", "any"].contains(&k) {
            warnings.push((format!("unknown key {k:?}, ignored"), line_of(text, i)));
        }
    }
    let mut spec = AnimatorSpec {
        file: file.to_string(),
        ..Default::default()
    };
    let states = doc
        .get("states")
        .and_then(|s| s.as_table_like())
        .ok_or_else(|| bad(file, 1, "an animator needs [states.<name>] tables"))?;
    for (name, st) in states.iter() {
        let line = line_of(text, st);
        let t = st
            .as_table_like()
            .ok_or_else(|| bad(file, line, format!("states.{name} is a table")))?;
        let key_line = |k: &str| t.get(k).map(|i| line_of(text, i)).unwrap_or(line);
        let state_line = t.iter().next().map(|(k, _)| key_line(k)).unwrap_or(line);
        for (k, i) in t.iter() {
            if ![
                "clip",
                "loop",
                "fps",
                "windup",
                "anticipation",
                "hold",
                "hold_frame",
                "events",
                "transitions",
            ]
            .contains(&k)
            {
                warnings.push((
                    format!("states.{name}: unknown key {k:?}, ignored"),
                    line_of(text, i),
                ));
            }
        }
        let clip = t
            .get("clip")
            .and_then(|c| c.as_str())
            .ok_or_else(|| {
                bad(
                    file,
                    state_line,
                    format!("states.{name} needs clip = \"<clip of the sprite>\""),
                )
            })?
            .to_string();
        let int = |k: &str| -> Result<Option<u32>> {
            match t.get(k) {
                None => Ok(None),
                Some(i) => i
                    .as_integer()
                    .filter(|v| *v >= 0)
                    .map(|v| Some(v as u32))
                    .ok_or_else(|| {
                        bad(
                            file,
                            line_of(text, i),
                            format!("states.{name}.{k} is a whole number, 0 or more"),
                        )
                    }),
            }
        };
        let mut events = Vec::new();
        if let Some(ev) = t.get("events") {
            let arr = ev
                .as_array()
                .ok_or_else(|| bad(file, line_of(text, ev), format!("states.{name}.events is [{{ frame = 1, name = \"footstep\", sound = \"sfx/step\" }}]")))?;
            for e in arr.iter() {
                let l = value_line(text, e);
                let tb = e.as_inline_table().ok_or_else(|| {
                    bad(
                        file,
                        l,
                        "each event is { frame = n, name = \"...\" }, optionally sound",
                    )
                })?;
                let frame = tb
                    .get("frame")
                    .and_then(|f| f.as_integer())
                    .filter(|f| *f >= 0)
                    .ok_or_else(|| {
                        bad(file, l, "an event needs frame = n, a clip position from 0")
                    })? as u32;
                let ename = tb.get("name").and_then(|n| n.as_str()).map(str::to_string);
                let sound = tb.get("sound").and_then(|n| n.as_str()).map(str::to_string);
                let ename = match (ename, &sound) {
                    (Some(n), _) => n,
                    (None, Some(_)) => "sound".to_string(),
                    (None, None) => {
                        return Err(bad(file, l, "an event needs a name, a sound, or both"))
                    }
                };
                events.push(FrameEventSpec {
                    frame,
                    name: ename,
                    sound,
                    at: format!("{file}:{l}"),
                });
            }
        }
        let trans = match t.get("transitions") {
            Some(i) => transitions(file, text, i, &format!("states.{name}.transitions"))?,
            None => Vec::new(),
        };
        spec.states.push(StateSpec {
            name: name.to_string(),
            clip,
            looping: t.get("loop").and_then(|l| l.as_bool()),
            fps: match t.get("fps") {
                Some(i) => num_of(file, text, i, "fps")?,
                None => None,
            },
            windup: int("windup")?.unwrap_or(0),
            anticipation: match t.get("anticipation") {
                Some(i) => num_of(file, text, i, "anticipation")?,
                None => None,
            },
            hold: match t.get("hold") {
                Some(i) => num_of(file, text, i, "hold")?,
                None => None,
            },
            hold_frame: int("hold_frame")?,
            events,
            transitions: trans,
            at: format!("{file}:{state_line}"),
        });
    }
    if spec.states.is_empty() {
        return Err(bad(file, 1, "an animator needs at least one state"));
    }
    if let Some(any) = doc.get("any") {
        spec.any = transitions(file, text, any, "any")?;
    }
    if let Some(f) = doc.get("face") {
        let line = line_of(text, f);
        let p = f.as_str().ok_or_else(|| {
            bad(
                file,
                line,
                "face is a field path such as \"KinematicBody.velocity.x\"",
            )
        })?;
        check_path(p).map_err(|m| bad(file, line, format!("face: {m}")))?;
        spec.face = Some(p.to_string());
        spec.face_at = format!("{file}:{line}");
    }
    spec.initial = match doc.get("initial") {
        Some(i) => i
            .as_str()
            .ok_or_else(|| bad(file, line_of(text, i), "initial is a state name"))?
            .to_string(),
        None => spec.states[0].name.clone(),
    };
    let names: Vec<&str> = spec.states.iter().map(|s| s.name.as_str()).collect();
    let known = names.join(", ");
    if !names.contains(&spec.initial.as_str()) {
        let line = doc.get("initial").map(|i| line_of(text, i)).unwrap_or(1);
        return Err(bad(
            file,
            line,
            format!(
                "initial = {:?} is not a state; the states are {known}",
                spec.initial
            ),
        ));
    }
    for tr in spec
        .states
        .iter()
        .flat_map(|s| s.transitions.iter())
        .chain(spec.any.iter())
    {
        if !names.contains(&tr.to.as_str()) {
            let line: u32 = tr
                .at
                .rsplit(':')
                .next()
                .and_then(|l| l.parse().ok())
                .unwrap_or(1);
            return Err(bad(
                file,
                line,
                format!("to = {:?} is not a state; the states are {known}", tr.to),
            ));
        }
    }
    Ok((spec, warnings))
}
