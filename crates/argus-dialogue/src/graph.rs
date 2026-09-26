//! The dialogue graph: the `dialogue` def table's rows as nodes, with
//! their conditions and effects parsed once.
//!
//! A row is a node, or, with `from`, a choice or a branch of one. A node
//! shows its `line` (a string table's key) said by its `speaker` (another
//! key), then offers its choices (rows `from` it that have a line and whose
//! `when` holds), or goes on to the first branch that holds (rows `from` it
//! without a line), or to its `next`. A node whose `when` does not hold is
//! passed over to its `next`. `effects` are edits made when a node is shown
//! or a choice or branch taken.
//!
//! A condition is comparisons joined by `and` and `or` (`and` binds first),
//! each `[subject:]Component.field [op value]`, or `not` one: `Run.gems >= 1
//! and not speaker:Hermit.met`. The subject is `listener` (the default),
//! `speaker`, or an authored entity, `@id`; a bare path is true when the
//! field is. An effect is `path = value`, `path += n`, `path -= n`, or
//! `emit event.name`, several separated by `;`.
//!
//! The same small language serves timelines (phase 3 item 5) with their
//! own parties: [`parse_cond_with`] and [`parse_effects_with`] read it
//! against a list of role names, the first the default subject, and
//! [`make_edits`] makes a list of effects in a system.

use argus_ecs::{Entity, Value, World};
use argus_scene::{DefSchema, Defs};
use argus_sim::SystemCtx;
use std::collections::HashMap;

/// The table's name and reference prefix.
pub const TABLE: &str = "dialogue";

/// The `dialogue` def table's schema: declared by the engine, so every
/// project has the table and `world.defs {table: "dialogue"}` answers it.
pub fn schema() -> DefSchema {
    use argus_ecs::{FieldType, SchemaBuilder};
    let str_field = |b: SchemaBuilder, name: &str, doc: &str| {
        b.field_full(
            name,
            FieldType::Str,
            Value::Str(String::new()),
            None,
            doc,
            false,
        )
    };
    let ref_field = |b: SchemaBuilder, name: &str, doc: &str| {
        b.field_full(
            name,
            FieldType::Ref,
            Value::Ref(String::new()),
            None,
            doc,
            false,
        )
    };
    let mut b = SchemaBuilder::new(TABLE)
        .doc("Dialogue (section 12): each row a node, or with from a choice or branch of one. A node shows its line (a string key) said by its speaker (a string key), then offers its choices, or takes the first branch whose when holds, or goes to next")
        .source("argus-dialogue");
    b = str_field(b, "id", "The node's key (the row's header)");
    b = str_field(
        b,
        "speaker",
        "A string key: who says the line (their name in the string table)",
    );
    b = str_field(b, "line", "A string key: what is said; a choice's, what the player picks; empty: a node that only branches");
    b = ref_field(
        b,
        "from",
        "The node this row is a choice of (with a line) or a branch of (without)",
    );
    b = ref_field(
        b,
        "next",
        "Where it goes after; empty: the conversation ends",
    );
    b = str_field(b, "when", "A condition on the world: the node is passed over, the choice not offered, the branch not taken unless it holds");
    b = str_field(b, "effects", "Edits made when the node is shown or the choice or branch taken: path = value; path += n; emit event");
    b = str_field(
        b,
        "sound",
        "A sound played when the line is shown (on its bus: a dialogue bus ducks the music)",
    );
    DefSchema::new(b.build().expect("the dialogue schema is valid"))
        .key("id")
        .prefix(TABLE)
}

/// Who a path is about.
#[derive(Clone, Debug, PartialEq)]
pub enum Subject {
    /// One of the parties the text was read with, by its place in the
    /// roles: in a conversation the listener (0, the default) or the
    /// speaker (1).
    Role(usize),
    /// An authored entity (`@id`).
    Id(String),
}

/// A conversation's parties as its conditions and effects name them: the
/// listener, the default subject, and the speaker.
pub const ROLES: &[&str] = &["listener", "speaker"];
/// The listener's place in [`ROLES`].
pub const LISTENER: usize = 0;
/// The speaker's place in [`ROLES`].
pub const SPEAKER: usize = 1;

/// A field of someone.
#[derive(Clone, Debug, PartialEq)]
pub struct Path {
    /// Whose.
    pub subject: Subject,
    /// `Component.field`.
    pub path: String,
}

/// A comparison.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op {
    /// `==`
    Eq,
    /// `!=`
    Ne,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
}

/// A literal.
#[derive(Clone, Debug, PartialEq)]
pub enum Lit {
    /// A number.
    Num(f64),
    /// A boolean.
    Bool(bool),
    /// Words.
    Str(String),
}

/// One comparison or truth test, maybe negated.
#[derive(Clone, Debug, PartialEq)]
pub struct Test {
    /// `not`.
    pub not: bool,
    /// The field.
    pub path: Path,
    /// Compared with, or `None`: is it true.
    pub cmp: Option<(Op, Lit)>,
}

/// A condition: any of (all of tests).
#[derive(Clone, Debug, PartialEq)]
pub struct Cond(pub Vec<Vec<Test>>);

/// An edit.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// `path = value`
    Set(Path, Lit),
    /// `path += n` (or `-=`, as a negative n)
    Add(Path, f64),
    /// `emit name`
    Emit(String),
}

/// One row.
#[derive(Clone, Debug)]
pub struct Node {
    /// Its key.
    pub key: String,
    /// String key of who speaks.
    pub speaker: String,
    /// String key of the line.
    pub line: String,
    /// The node this is a choice or branch of.
    pub from: Option<String>,
    /// Where it goes.
    pub next: Option<String>,
    /// Its condition.
    pub when: Option<Cond>,
    /// Its edits.
    pub effects: Vec<Effect>,
    /// A sound for its line.
    pub sound: String,
    /// `file:line` of the row.
    pub at: String,
}

/// Every dialogue node.
#[derive(Clone, Debug, Default)]
pub struct Graph {
    nodes: HashMap<String, Node>,
    children: HashMap<String, Vec<String>>,
    /// Rows that did not read: `(file:line, message)`.
    pub problems: Vec<(String, String)>,
}

fn unref(r: &str) -> Option<String> {
    let r = r.trim();
    if r.is_empty() {
        return None;
    }
    Some(r.strip_prefix("dialogue:").unwrap_or(r).to_string())
}

impl Graph {
    /// The graph of a project's `dialogue` table (empty when it has none).
    pub fn from_defs(defs: &Defs) -> Graph {
        let mut g = Graph::default();
        let Some(t) = defs.table(TABLE) else {
            return g;
        };
        let names: Vec<&str> = t.schema.fields.iter().map(|f| f.name.as_str()).collect();
        for row in &t.rows {
            let get = |f: &str| -> String {
                names
                    .iter()
                    .position(|n| *n == f)
                    .and_then(|i| row.values.get(i))
                    .map(|v| match v {
                        Value::Str(s) | Value::Ref(s) => s.clone(),
                        _ => String::new(),
                    })
                    .unwrap_or_default()
            };
            let mut node = Node {
                key: row.key.clone(),
                speaker: get("speaker"),
                line: get("line"),
                from: unref(&get("from")),
                next: unref(&get("next")),
                when: None,
                effects: Vec::new(),
                sound: get("sound"),
                at: row.at.clone(),
            };
            let when = get("when");
            if !when.trim().is_empty() {
                match parse_cond_with(&when, ROLES) {
                    Ok(c) => node.when = Some(c),
                    Err(e) => g
                        .problems
                        .push((row.at.clone(), format!("{}: when: {e}", row.key))),
                }
            }
            let effects = get("effects");
            if !effects.trim().is_empty() {
                match parse_effects_with(&effects, ROLES) {
                    Ok(e) => node.effects = e,
                    Err(e) => g
                        .problems
                        .push((row.at.clone(), format!("{}: effects: {e}", row.key))),
                }
            }
            if let Some(f) = &node.from {
                g.children
                    .entry(f.clone())
                    .or_default()
                    .push(node.key.clone());
            }
            g.nodes.insert(node.key.clone(), node);
        }
        // Where a row points at a node there is not.
        let mut dangling = Vec::new();
        for n in g.nodes.values() {
            for (what, to) in [("from", &n.from), ("next", &n.next)] {
                if let Some(to) = to {
                    if !g.nodes.contains_key(to) {
                        dangling.push((
                            n.at.clone(),
                            format!("{}: {what} {to:?} is no dialogue row", n.key),
                        ));
                    }
                }
            }
        }
        dangling.sort();
        g.problems.extend(dangling);
        g
    }

    /// A node by key (`hermit.hello` or `dialogue:hermit.hello`).
    pub fn node(&self, key: &str) -> Option<&Node> {
        self.nodes.get(key.strip_prefix("dialogue:").unwrap_or(key))
    }

    /// The rows `from` a node, in file order.
    pub fn children(&self, key: &str) -> &[String] {
        self.children.get(key).map(Vec::as_slice).unwrap_or(&[])
    }

    /// How many rows.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// No rows.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

// ----- parsing -------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Word(String),
    Str(String),
    Op(String),
}

fn tokens(s: &str) -> Result<Vec<Tok>, String> {
    let cs: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '"' || c == '\'' {
            let mut w = String::new();
            i += 1;
            while i < cs.len() && cs[i] != c {
                w.push(cs[i]);
                i += 1;
            }
            if i >= cs.len() {
                return Err(format!("{s:?} has an unclosed quote"));
            }
            i += 1;
            out.push(Tok::Str(w));
        } else if "=!<>+-".contains(c)
            && !(c == '-'
                && i + 1 < cs.len()
                && cs[i + 1].is_ascii_digit()
                && !matches!(out.last(), Some(Tok::Word(_)) | Some(Tok::Str(_))))
        {
            let mut w = c.to_string();
            if i + 1 < cs.len() && cs[i + 1] == '=' {
                w.push('=');
                i += 1;
            }
            i += 1;
            out.push(Tok::Op(w));
        } else {
            let mut w = String::new();
            while i < cs.len() && !cs[i].is_whitespace() && !"=!<>+\"'".contains(cs[i]) {
                w.push(cs[i]);
                i += 1;
            }
            out.push(Tok::Word(w));
        }
    }
    Ok(out)
}

fn path_of(w: &str, roles: &[&str]) -> Result<Path, String> {
    let (subject, path) = match w.split_once(':') {
        Some((id, p)) if id.starts_with('@') => (Subject::Id(id[1..].to_string()), p),
        Some((who, p)) => match roles.iter().position(|r| *r == who) {
            Some(i) => (Subject::Role(i), p),
            None => {
                return Err(format!(
                    "{who:?} is not someone: {}, or @id",
                    roles.join(", ")
                ))
            }
        },
        None => (Subject::Role(0), w),
    };
    let ok = path.split_once('.').is_some_and(|(c, f)| {
        !c.is_empty() && !f.is_empty() && c.chars().all(|x| x.is_alphanumeric() || x == '_')
    });
    if !ok {
        return Err(format!("{w:?} is not Component.field"));
    }
    Ok(Path {
        subject,
        path: path.to_string(),
    })
}

fn lit_of(t: &Tok) -> Lit {
    match t {
        Tok::Str(s) => Lit::Str(s.clone()),
        Tok::Word(w) | Tok::Op(w) => match w.as_str() {
            "true" => Lit::Bool(true),
            "false" => Lit::Bool(false),
            _ => match w.parse::<f64>() {
                Ok(n) => Lit::Num(n),
                Err(_) => Lit::Str(w.clone()),
            },
        },
    }
}

/// Parse a conversation's condition.
pub fn parse_cond(s: &str) -> Result<Cond, String> {
    parse_cond_with(s, ROLES)
}

/// Parse a condition whose subjects are `roles` (the first the default)
/// or an authored `@id`.
pub fn parse_cond_with(s: &str, roles: &[&str]) -> Result<Cond, String> {
    let toks = tokens(s)?;
    let mut any = vec![Vec::new()];
    let mut i = 0;
    let mut expect_test = true;
    while i < toks.len() {
        match &toks[i] {
            Tok::Word(w) if !expect_test && (w == "and" || w == "or") => {
                if w == "or" {
                    any.push(Vec::new());
                }
                expect_test = true;
                i += 1;
            }
            Tok::Word(w) if expect_test => {
                let (not, w) = if w == "not" {
                    i += 1;
                    match toks.get(i) {
                        Some(Tok::Word(w)) => (true, w.clone()),
                        _ => return Err(format!("{s:?}: not what?")),
                    }
                } else {
                    (false, w.clone())
                };
                let path = path_of(&w, roles)?;
                i += 1;
                let mut cmp = None;
                if let Some(Tok::Op(o)) = toks.get(i) {
                    let op = match o.as_str() {
                        "==" | "=" => Op::Eq,
                        "!=" => Op::Ne,
                        "<" => Op::Lt,
                        "<=" => Op::Le,
                        ">" => Op::Gt,
                        ">=" => Op::Ge,
                        other => return Err(format!("{s:?}: {other:?} is not a comparison")),
                    };
                    let v = match toks.get(i + 1) {
                        Some(Tok::Op(o)) => {
                            return Err(format!("{s:?}: {} compared with {o:?}", path.path))
                        }
                        Some(v) => v,
                        None => return Err(format!("{s:?}: {} compared with nothing", path.path)),
                    };
                    cmp = Some((op, lit_of(v)));
                    i += 2;
                }
                any.last_mut().expect("one").push(Test { not, path, cmp });
                expect_test = false;
            }
            other => {
                return Err(format!(
                    "{s:?}: unexpected {:?}; a condition is [not] [who:]Component.field [op value], joined by and / or",
                    match other {
                        Tok::Word(w) | Tok::Op(w) | Tok::Str(w) => w.clone(),
                    }
                ))
            }
        }
    }
    if expect_test {
        return Err(format!("{s:?} ends where a test should be"));
    }
    Ok(Cond(any))
}

/// Parse a conversation's effects.
pub fn parse_effects(s: &str) -> Result<Vec<Effect>, String> {
    parse_effects_with(s, ROLES)
}

/// Parse effects whose subjects are `roles` (the first the default) or an
/// authored `@id`.
pub fn parse_effects_with(s: &str, roles: &[&str]) -> Result<Vec<Effect>, String> {
    let mut out = Vec::new();
    for part in s.split(';') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some(name) = part.strip_prefix("emit ") {
            let name = name.trim();
            if name.is_empty() || name.contains(' ') {
                return Err(format!("{part:?}: emit one event name"));
            }
            out.push(Effect::Emit(name.to_string()));
            continue;
        }
        let toks = tokens(part)?;
        let [Tok::Word(w), Tok::Op(o), v] = toks.as_slice() else {
            return Err(format!(
                "{part:?}: an effect is [who:]Component.field = value, += n, -= n, or emit name"
            ));
        };
        let path = path_of(w, roles)?;
        let lit = lit_of(v);
        out.push(match (o.as_str(), lit) {
            ("=", l) => Effect::Set(path, l),
            ("+=", Lit::Num(n)) => Effect::Add(path, n),
            ("-=", Lit::Num(n)) => Effect::Add(path, -n),
            (op, _) => return Err(format!("{part:?}: {op} needs a number, or = a value")),
        });
    }
    Ok(out)
}

// ----- evaluating -----------------------------------------------------------

/// The entity a path is about, if there is one: `who` holds the parties
/// in the order of the roles the text was read with.
pub fn subject(world: &World, p: &Path, who: &[Entity]) -> Option<Entity> {
    let e = match &p.subject {
        Subject::Role(i) => *who.get(*i)?,
        Subject::Id(id) => world.entity(&format!("@{id}")).ok()?,
    };
    (!e.is_null() && world.is_alive(e)).then_some(e)
}

fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Int(i) => Some(*i as f64),
        Value::UInt(u) => Some(*u as f64),
        Value::Float(f) => Some(*f),
        _ => None,
    }
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Str(s) | Value::Ref(s) => !s.is_empty(),
        Value::Entity(e) => !e.is_null(),
        other => num(other).is_some_and(|n| n != 0.0),
    }
}

fn test(world: &World, t: &Test, who: &[Entity]) -> bool {
    let v = subject(world, &t.path, who).and_then(|e| world.get_field(e, &t.path.path).ok());
    let r = match (&v, &t.cmp) {
        (None, _) => false,
        (Some(v), None) => truthy(v),
        (Some(v), Some((op, lit))) => {
            let ord = match (lit, v) {
                (Lit::Num(n), v) => num(v).and_then(|a| a.partial_cmp(n)),
                (Lit::Bool(b), Value::Bool(a)) => Some(a.cmp(b)),
                (Lit::Str(s), Value::Str(a) | Value::Ref(a)) => Some(a.as_str().cmp(s.as_str())),
                _ => None,
            };
            match ord {
                None => *op == Op::Ne,
                Some(o) => match op {
                    Op::Eq => o.is_eq(),
                    Op::Ne => o.is_ne(),
                    Op::Lt => o.is_lt(),
                    Op::Le => o.is_le(),
                    Op::Gt => o.is_gt(),
                    Op::Ge => o.is_ge(),
                },
            }
        }
    };
    r != t.not
}

/// Whether a condition holds (none always does).
pub fn holds(world: &World, c: Option<&Cond>, who: &[Entity]) -> bool {
    match c {
        None => true,
        Some(Cond(any)) => any
            .iter()
            .any(|all| all.iter().all(|t| test(world, t, who))),
    }
}

/// A literal as the value a field of `old`'s kind takes.
pub fn value_like(old: &Value, lit: &Lit) -> Option<Value> {
    Some(match (old, lit) {
        (Value::Int(_), Lit::Num(n)) => Value::Int(*n as i64),
        (Value::UInt(_), Lit::Num(n)) if *n >= 0.0 => Value::UInt(*n as u64),
        (Value::Float(_), Lit::Num(n)) => Value::Float(*n),
        (Value::Bool(_), Lit::Bool(b)) => Value::Bool(*b),
        (Value::Str(_), Lit::Str(s)) => Value::Str(s.clone()),
        (Value::Str(_), Lit::Num(n)) => Value::Str(n.to_string()),
        (Value::Ref(_), Lit::Str(s)) => Value::Ref(s.clone()),
        _ => return None,
    })
}

/// Make `effects` from a system: each field edit is a write of the world
/// like any system's, declared as it happens (as a tween's target is), so
/// `who_writes` stays true; each `emit` is an event with `fields`. An edit
/// that cannot be made is said in `problems` and the rest go on.
pub fn make_edits(
    ctx: &mut SystemCtx<'_>,
    effects: &[Effect],
    who: &[Entity],
    fields: &[(String, Value)],
    problems: &mut Vec<String>,
) {
    for eff in effects {
        let (path, lit) = match eff {
            Effect::Emit(name) => {
                ctx.emit(name.clone(), fields.to_vec());
                continue;
            }
            Effect::Set(p, l) => (p, l.clone()),
            Effect::Add(p, d) => (p, Lit::Num(*d)),
        };
        let Some(e) = subject(ctx.world(), path, who) else {
            problems.push(format!("no one to edit for {}", path.path));
            continue;
        };
        let old = match ctx.world().get_field(e, &path.path) {
            Ok(v) => v,
            Err(err) => {
                problems.push(err.message);
                continue;
            }
        };
        let lit = match (eff, &old) {
            (Effect::Add(_, d), v) => match v {
                Value::Int(i) => Lit::Num(*i as f64 + d),
                Value::UInt(u) => Lit::Num((*u as f64 + d).max(0.0)),
                Value::Float(f) => Lit::Num(f + d),
                _ => {
                    problems.push(format!("{} is not a number to add to", path.path));
                    continue;
                }
            },
            _ => lit,
        };
        let Some(v) = value_like(&old, &lit) else {
            problems.push(format!("{} cannot take {lit:?}", path.path));
            continue;
        };
        if let Ok((c, _)) = ctx.world().registry().resolve_path(&path.path) {
            ctx.declare(c, true);
        }
        if let Err(err) = ctx.world().set_field(e, &path.path, v) {
            problems.push(err.message);
        }
    }
}
