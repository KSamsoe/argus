//! The one `world.query` evaluator (§5 "Shape of a query") and the paging
//! contract every read shares: `limit` defaulting to 50, `total` always
//! present, `truncated`, and a `next` cursor. It runs over a [`Source`],
//! which is the live world or the index, so both give identical results.

use crate::codes;
use argus_ecs::{ArgusError, Result};
use serde_json::{json, Value as Json};

/// Default page size (§5).
pub const DEFAULT_LIMIT: u64 = 50;

/// Parameter error, shared spelling with the daemon (code 4704).
pub fn bad_params(msg: impl Into<String>) -> ArgusError {
    ArgusError::new(4704, msg).hint("session.docs {method} shows the parameters each method takes.")
}

/// Optional unsigned integer with a default.
pub fn u64_param(params: &Json, key: &str, default: u64) -> Result<u64> {
    match params.get(key) {
        None | Some(Json::Null) => Ok(default),
        Some(Json::Number(n)) => n
            .as_u64()
            .ok_or_else(|| bad_params(format!("{key} must be a non-negative integer"))),
        Some(_) => Err(bad_params(format!("{key} must be a non-negative integer"))),
    }
}

/// Optional list of strings (missing means empty; a bare string is one item).
pub fn str_list(params: &Json, key: &str) -> Result<Vec<String>> {
    match params.get(key) {
        None | Some(Json::Null) => Ok(Vec::new()),
        Some(Json::Array(a)) => a
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| bad_params(format!("{key} must be an array of strings")))
            })
            .collect(),
        Some(Json::String(s)) => Ok(vec![s.clone()]),
        Some(_) => Err(bad_params(format!("{key} must be an array of strings"))),
    }
}

/// Optional string parameter.
pub fn opt_str<'a>(params: &'a Json, key: &str) -> Result<Option<&'a str>> {
    match params.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::String(s)) => Ok(Some(s.as_str())),
        Some(_) => Err(bad_params(format!("{key} must be a string"))),
    }
}

/// Read `limit` and `cursor` from params.
pub fn page(params: &Json) -> Result<(usize, usize)> {
    let limit = u64_param(params, "limit", DEFAULT_LIMIT)? as usize;
    let offset = match params.get("cursor") {
        None | Some(Json::Null) => 0,
        Some(Json::String(c)) => c
            .strip_prefix("c:")
            .and_then(|n| n.parse::<usize>().ok())
            .ok_or_else(|| {
                ArgusError::new(4706, format!("{c:?} is not a cursor from this daemon"))
                    .hint("Pass the next value from the previous response unchanged.")
            })?,
        Some(_) => return Err(bad_params("cursor must be a string")),
    };
    Ok((limit, offset))
}

/// Slice `rows` into a page and build the standard envelope.
pub fn paged(rows: Vec<Json>, limit: usize, offset: usize) -> Json {
    let total = rows.len();
    let end = (offset + limit).min(total);
    let page: Vec<Json> = if offset < total {
        rows[offset..end].to_vec()
    } else {
        Vec::new()
    };
    let truncated = end < total;
    let mut out = json!({ "total": total, "rows": page, "truncated": truncated });
    if truncated {
        out["next"] = json!(format!("c:{end}"));
    }
    out
}

/// What a query runs over. Ids are opaque handles valid for one call.
pub trait Source {
    /// World frame the data is from (0 for the index).
    fn frame(&self) -> u64;
    /// Error unless the component exists.
    fn require_component(&self, name: &str) -> Result<()>;
    /// Error unless `Component.field` exists.
    fn require_path(&self, path: &str) -> Result<()>;
    /// Entities having every component in `has` and none in `not`, in a
    /// deterministic order.
    fn candidates(&self, has: &[String], not: &[String]) -> Result<Vec<u64>>;
    /// One field as JSON, or `None` if the entity lacks the component.
    fn field(&self, id: u64, path: &str) -> Option<Json>;
    /// A whole component as an object, or `None`.
    fn component(&self, id: u64, name: &str) -> Option<Json>;
    /// Protocol spelling (`"@id"` or `"e:n"`).
    fn entity_ref(&self, id: u64) -> Json;
    /// Resolve a protocol reference to an id.
    fn lookup(&self, r: &str) -> Result<u64>;
    /// The room an entity is authored in, in a scene with rooms (phase 3
    /// item 6: `where.room` and the `room` column).
    fn room(&self, _id: u64) -> Option<String> {
        None
    }
    /// Ids within `r` of a point from a broadphase, when the source has
    /// one (the live world does; the index scans). The evaluator still
    /// measures the distance itself.
    fn near(&self, _center: (f64, f64), _r: f64) -> Option<Vec<u64>> {
        None
    }
    /// World position: `GlobalTransform.pos`, else `Transform.pos`.
    fn position(&self, id: u64) -> Option<(f64, f64)> {
        for path in ["GlobalTransform.pos", "Transform.pos"] {
            if let Some(Json::Array(a)) = self.field(id, path) {
                if let (Some(x), Some(y)) = (
                    a.first().and_then(Json::as_f64),
                    a.get(1).and_then(Json::as_f64),
                ) {
                    return Some((x, y));
                }
            }
        }
        None
    }
}

#[derive(Clone, Copy)]
enum Cmp {
    Eq,
    Ne,
    Lt,
    Lte,
    Gt,
    Gte,
}

struct FieldPred {
    path: String,
    cmp: Cmp,
    value: Json,
}

/// An aggregate over a group's entities (§8 L1 "aggregates").
#[derive(Clone, Copy, PartialEq)]
enum Agg {
    Count,
    Sum,
    Mean,
    Min,
    Max,
}

/// One `select` entry: a column of each entity, or an aggregate
/// (`count()`, `sum(Stockpile.food)`) of each group.
enum Sel {
    Plain(String),
    Agg(Agg, Option<String>),
}

fn parse_sel(s: &str) -> Result<Sel> {
    let Some(open) = s.find('(') else {
        return Ok(Sel::Plain(s.to_string()));
    };
    let Some(inner) = s[open + 1..].strip_suffix(')') else {
        return Err(bad_params(format!(
            "select {s:?}: an aggregate is name(Component.field), e.g. sum(Stockpile.food)"
        )));
    };
    let agg = match &s[..open] {
        "count" => Agg::Count,
        "sum" => Agg::Sum,
        "mean" => Agg::Mean,
        "min" => Agg::Min,
        "max" => Agg::Max,
        other => {
            return Err(bad_params(format!(
                "select {s:?}: unknown aggregate {other:?}; use count, sum, mean, min, max"
            )))
        }
    };
    let inner = inner.trim();
    if inner.is_empty() {
        if agg != Agg::Count {
            return Err(bad_params(format!(
                "select {s:?}: {} needs a field, e.g. {}(Stockpile.food)",
                &s[..open],
                &s[..open]
            )));
        }
        return Ok(Sel::Agg(agg, None));
    }
    Ok(Sel::Agg(agg, Some(inner.to_string())))
}

/// Run a query.
pub fn run(src: &dyn Source, params: &Json) -> Result<Json> {
    let select = str_list(params, "select")?;
    let group_by = str_list(params, "group_by")?;
    let aggregating = !group_by.is_empty() || select.iter().any(|s| s.contains('('));
    let select = if !select.is_empty() {
        select
    } else if aggregating {
        vec!["count()".to_string()]
    } else {
        vec!["id".to_string()]
    };
    for g in &group_by {
        if !g.contains('.') {
            return Err(bad_params(format!(
                "group_by {g:?}: group by a field, Component.field"
            )));
        }
        src.require_path(g)?;
    }
    let mut sels = Vec::new();
    for s in &select {
        let sel = parse_sel(s)?;
        match &sel {
            Sel::Plain(p) if p == "id" || p == "entity" || p == "room" => {}
            Sel::Plain(p) if p.contains('.') => src.require_path(p)?,
            Sel::Plain(p) => src.require_component(p)?,
            Sel::Agg(_, Some(p)) => src.require_path(p)?,
            Sel::Agg(_, None) => {}
        }
        sels.push(sel);
    }

    let empty = json!({});
    let where_ = params.get("where").unwrap_or(&empty);
    let Some(wobj) = where_.as_object() else {
        return Err(bad_params("where must be an object"));
    };
    let mut has = str_list(where_, "has")?;
    let not = str_list(where_, "not")?;
    // The rooms the entities are authored in: a name or a list.
    let rooms: Option<Vec<String>> = match where_.get("room") {
        None | Some(Json::Null) => None,
        Some(Json::String(s)) => Some(vec![s.clone()]),
        Some(_) => Some(str_list(where_, "room")?),
    };
    // Grouping by a field is about the entities that have it: group_by
    // Job.kind counts jobs, not every entity with a blank kind.
    for g in &group_by {
        let c = g.split('.').next().unwrap_or(g).to_string();
        if !has.contains(&c) {
            has.push(c);
        }
    }
    for c in has.iter().chain(&not) {
        src.require_component(c)?;
    }
    let within = match where_.get("within") {
        None | Some(Json::Null) => None,
        Some(w) => {
            let of = opt_str(w, "of")?.ok_or_else(|| bad_params("within.of must be an entity"))?;
            let r = w.get("r").and_then(Json::as_f64).unwrap_or(0.0);
            let center = src.position(src.lookup(of)?).ok_or_else(|| {
                ArgusError::new(
                    argus_ecs::codes::MISSING_COMPONENT,
                    format!("{of} has no Transform, so within cannot be measured from it"),
                )
            })?;
            Some((center, r))
        }
    };
    let mut preds = Vec::new();
    for (k, v) in wobj {
        if matches!(k.as_str(), "has" | "not" | "within" | "room") {
            continue;
        }
        src.require_path(k)?;
        match v {
            Json::Object(o) => {
                for (op, val) in o {
                    let cmp = match op.as_str() {
                        "eq" => Cmp::Eq,
                        "ne" => Cmp::Ne,
                        "lt" => Cmp::Lt,
                        "lte" => Cmp::Lte,
                        "gt" => Cmp::Gt,
                        "gte" => Cmp::Gte,
                        other => {
                            return Err(bad_params(format!(
                                "{k}: unknown comparison {other:?}; use eq, ne, lt, lte, gt, gte"
                            )))
                        }
                    };
                    preds.push(FieldPred {
                        path: k.clone(),
                        cmp,
                        value: val.clone(),
                    });
                }
            }
            other => preds.push(FieldPred {
                path: k.clone(),
                cmp: Cmp::Eq,
                value: other.clone(),
            }),
        }
    }

    let mut rows: Vec<(u64, Option<f64>)> = Vec::new();
    // A spatial predicate narrows the candidates through the broadphase
    // before any field is read (§17: a hash, not a scan).
    let mut candidates = src.candidates(&has, &not)?;
    if let Some((center, r)) = within {
        if let Some(near) = src.near(center, r) {
            let near: std::collections::HashSet<u64> = near.into_iter().collect();
            candidates.retain(|id| near.contains(id));
        }
    }
    if let Some(rooms) = &rooms {
        candidates.retain(|id| src.room(*id).is_some_and(|r| rooms.contains(&r)));
    }
    'ent: for id in candidates {
        for p in &preds {
            let Some(v) = src.field(id, &p.path) else {
                continue 'ent;
            };
            if !compare(&v, p.cmp, &p.value) {
                continue 'ent;
            }
        }
        let mut dist = None;
        if let Some((center, r)) = within {
            let Some(pos) = src.position(id) else {
                continue;
            };
            let d = ((pos.0 - center.0).powi(2) + (pos.1 - center.1).powi(2)).sqrt();
            if d > r {
                continue;
            }
            dist = Some(d);
        }
        rows.push((id, dist));
    }

    if aggregating {
        let ids: Vec<u64> = rows.iter().map(|r| r.0).collect();
        return aggregate(src, params, &select, sels, &group_by, &ids);
    }

    if let Some(order) = opt_str(params, "order")? {
        if order == "ref" {
            // By how the protocol spells each entity (issue 34): ids by
            // name, then the rest by number. It does not change when
            // components come and go, as a draw from the rows must not.
            let key = |id: u64| {
                let r = src.entity_ref(id);
                let s = r.as_str().unwrap_or("").to_string();
                match s.strip_prefix("e:") {
                    Some(n) => {
                        let (i, g) = n.split_once('.').unwrap_or((n, "0"));
                        (
                            1u8,
                            String::new(),
                            i.parse::<u64>().unwrap_or(0),
                            g.parse::<u64>().unwrap_or(0),
                        )
                    }
                    None => (0u8, s, 0, 0),
                }
            };
            rows.sort_by_cached_key(|r| (key(r.0), r.0));
        } else if let Some(inner) = order
            .strip_prefix("dist(")
            .and_then(|s| s.strip_suffix(')'))
        {
            let center = src.position(src.lookup(inner)?).ok_or_else(|| {
                ArgusError::new(
                    argus_ecs::codes::MISSING_COMPONENT,
                    format!("{inner} has no Transform to measure distance from"),
                )
            })?;
            for (id, d) in rows.iter_mut() {
                *d = src
                    .position(*id)
                    .map(|p| ((p.0 - center.0).powi(2) + (p.1 - center.1).powi(2)).sqrt());
            }
            rows.sort_by(|a, b| {
                a.1.unwrap_or(f64::INFINITY)
                    .partial_cmp(&b.1.unwrap_or(f64::INFINITY))
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(a.0.cmp(&b.0))
            });
        } else {
            let (desc, path) = match order.strip_prefix('-') {
                Some(p) => (true, p),
                None => (false, order),
            };
            src.require_path(path)?;
            let mut keyed: Vec<(Option<f64>, u64, Option<f64>)> = rows
                .iter()
                .map(|(id, d)| (src.field(*id, path).and_then(|v| v.as_f64()), *id, *d))
                .collect();
            // Entities without the field sort last in either direction.
            keyed.sort_by(|a, b| {
                let o = match (a.0, b.0) {
                    (Some(x), Some(y)) => {
                        let o = x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal);
                        if desc {
                            o.reverse()
                        } else {
                            o
                        }
                    }
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => std::cmp::Ordering::Equal,
                };
                o.then(a.1.cmp(&b.1))
            });
            rows = keyed.into_iter().map(|(_, id, d)| (id, d)).collect();
        }
    } else {
        rows.sort_by_key(|r| r.0);
    }

    let (limit, offset) = page(params)?;
    let total = rows.len();
    let end = (offset + limit).min(total);
    let mut out_rows = Vec::new();
    if offset < total {
        for (id, _) in &rows[offset..end] {
            let row: Vec<Json> = select.iter().map(|s| project(src, *id, s)).collect();
            out_rows.push(Json::Array(row));
        }
    }
    let truncated = end < total;
    let mut out = json!({
        "frame": src.frame(),
        "total": total,
        "rows": out_rows,
        "truncated": truncated,
    });
    if truncated {
        out["next"] = json!(format!("c:{end}"));
    }
    Ok(out)
}

/// A running aggregate: numbers, or vec2s added axis by axis (a mean of
/// positions is the group's centre).
#[derive(Clone, Default)]
struct Acc {
    n: u64,
    sum: Vec<f64>,
    min: Vec<f64>,
    max: Vec<f64>,
    ints: bool,
}

impl Acc {
    fn add(&mut self, v: &Json) {
        let nums: Vec<f64> = match v {
            Json::Number(n) => vec![n.as_f64().unwrap_or(0.0)],
            Json::Bool(b) => vec![if *b { 1.0 } else { 0.0 }],
            Json::Array(a) if !a.is_empty() && a.iter().all(Json::is_number) => {
                a.iter().filter_map(Json::as_f64).collect()
            }
            _ => return,
        };
        let int = matches!(v, Json::Number(n) if n.is_i64() || n.is_u64());
        if self.n == 0 {
            self.sum = vec![0.0; nums.len()];
            self.min = nums.clone();
            self.max = nums.clone();
            self.ints = int;
        } else if nums.len() != self.sum.len() {
            return;
        }
        self.n += 1;
        self.ints &= int;
        for (k, x) in nums.into_iter().enumerate() {
            self.sum[k] += x;
            self.min[k] = self.min[k].min(x);
            self.max[k] = self.max[k].max(x);
        }
    }

    fn out(&self, agg: Agg) -> Json {
        let shape = |v: Vec<f64>, ints: bool| -> Json {
            let one = |x: f64| {
                if ints && x.fract() == 0.0 && x.abs() < 9.0e15 {
                    json!(x as i64)
                } else {
                    json!(x)
                }
            };
            if v.len() == 1 {
                one(v[0])
            } else {
                Json::Array(v.into_iter().map(one).collect())
            }
        };
        match agg {
            Agg::Count => json!(self.n),
            Agg::Sum if self.n == 0 => json!(0),
            Agg::Sum => shape(self.sum.clone(), self.ints),
            _ if self.n == 0 => Json::Null,
            Agg::Mean => shape(self.sum.iter().map(|s| s / self.n as f64).collect(), false),
            Agg::Min => shape(self.min.clone(), self.ints),
            Agg::Max => shape(self.max.clone(), self.ints),
        }
    }
}

/// A total order over group keys: null, false, true, numbers, strings,
/// then anything else by its text.
fn key_cmp(a: &Json, b: &Json) -> std::cmp::Ordering {
    fn rank(v: &Json) -> u8 {
        match v {
            Json::Null => 0,
            Json::Bool(_) => 1,
            Json::Number(_) => 2,
            Json::String(_) => 3,
            _ => 4,
        }
    }
    match (a, b) {
        (Json::Bool(x), Json::Bool(y)) => x.cmp(y),
        (Json::Number(x), Json::Number(y)) => x
            .as_f64()
            .unwrap_or(0.0)
            .partial_cmp(&y.as_f64().unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal),
        (Json::String(x), Json::String(y)) => x.cmp(y),
        _ => rank(a)
            .cmp(&rank(b))
            .then_with(|| a.to_string().cmp(&b.to_string())),
    }
}

/// Group the matched entities by `group_by` and answer one row a group
/// (§8: "world.query with group_by and sum over stockpiles, job queues,
/// and colonist needs"). Without `group_by`, the matched entities are one
/// group, and there is one row even when nothing matched.
fn aggregate(
    src: &dyn Source,
    params: &Json,
    select: &[String],
    sels: Vec<Sel>,
    group_by: &[String],
    ids: &[u64],
) -> Result<Json> {
    // A plain column is a group key; the keys not selected lead the row.
    let mut columns: Vec<String> = Vec::new();
    let mut cols: Vec<Sel> = Vec::new();
    for g in group_by {
        if !select.iter().any(|s| s == g) {
            columns.push(g.clone());
            cols.push(Sel::Plain(g.clone()));
        }
    }
    for (s, sel) in select.iter().zip(sels) {
        if let Sel::Plain(p) = &sel {
            if !group_by.iter().any(|g| g == p) {
                return Err(bad_params(format!(
                    "select {p:?} is one value an entity, and this query answers one row a group: add it to group_by, or aggregate it (count(), sum({p}), mean, min, max)"
                )));
            }
        }
        columns.push(s.clone());
        cols.push(sel);
    }
    let fields: Vec<&str> = cols
        .iter()
        .filter_map(|c| match c {
            Sel::Agg(_, Some(p)) => Some(p.as_str()),
            _ => None,
        })
        .collect();
    let mut groups: Vec<(Vec<Json>, Vec<Acc>, u64)> = Vec::new();
    let mut at: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    if group_by.is_empty() {
        groups.push((Vec::new(), vec![Acc::default(); fields.len()], 0));
        at.insert("[]".to_string(), 0);
    }
    for &id in ids {
        let key: Vec<Json> = group_by
            .iter()
            .map(|g| src.field(id, g).unwrap_or(Json::Null))
            .collect();
        let text = Json::Array(key.clone()).to_string();
        let i = match at.get(&text) {
            Some(&i) => i,
            None => {
                groups.push((key, vec![Acc::default(); fields.len()], 0));
                at.insert(text, groups.len() - 1);
                groups.len() - 1
            }
        };
        let g = &mut groups[i];
        g.2 += 1;
        for (k, f) in fields.iter().enumerate() {
            if let Some(v) = src.field(id, f) {
                g.1[k].add(&v);
            }
        }
    }
    let mut rows: Vec<(Vec<Json>, Vec<Json>)> = groups
        .into_iter()
        .map(|(key, accs, n)| {
            let mut k = 0usize;
            let row: Vec<Json> = cols
                .iter()
                .map(|c| match c {
                    Sel::Plain(p) => {
                        let i = group_by.iter().position(|g| g == p).expect("a key");
                        key[i].clone()
                    }
                    Sel::Agg(Agg::Count, None) => json!(n),
                    Sel::Agg(a, Some(_)) => {
                        k += 1;
                        accs[k - 1].out(*a)
                    }
                    Sel::Agg(_, None) => Json::Null,
                })
                .collect();
            (key, row)
        })
        .collect();
    rows.sort_by(|a, b| {
        a.0.iter()
            .zip(&b.0)
            .map(|(x, y)| key_cmp(x, y))
            .find(|o| o.is_ne())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    // `order` names a column of the answer: "-sum(Stockpile.food)".
    if let Some(order) = opt_str(params, "order")? {
        let (desc, name) = match order.strip_prefix('-') {
            Some(p) => (true, p),
            None => (false, order),
        };
        let Some(c) = columns.iter().position(|s| s == name) else {
            return Err(bad_params(format!(
                "order {order:?}: order a grouped query by one of its columns: {}",
                columns.join(", ")
            )));
        };
        rows.sort_by(|a, b| {
            let o = key_cmp(&a.1[c], &b.1[c]);
            if desc {
                o.reverse()
            } else {
                o
            }
        });
    }
    let (limit, offset) = page(params)?;
    let total = rows.len();
    let end = (offset + limit).min(total);
    let mut out_rows = Vec::new();
    if offset < total {
        for (_, row) in &rows[offset..end] {
            out_rows.push(Json::Array(row.clone()));
        }
    }
    let truncated = end < total;
    let mut out = json!({
        "frame": src.frame(),
        "total": total,
        "matched": ids.len(),
        "columns": columns,
        "rows": out_rows,
        "truncated": truncated,
    });
    if truncated {
        out["next"] = json!(format!("c:{end}"));
    }
    Ok(out)
}

/// One selected column of one entity.
pub fn project(src: &dyn Source, id: u64, sel: &str) -> Json {
    if sel == "id" || sel == "entity" {
        return src.entity_ref(id);
    }
    if sel == "room" {
        return src.room(id).map(Json::String).unwrap_or(Json::Null);
    }
    if sel.contains('.') {
        return src.field(id, sel).unwrap_or(Json::Null);
    }
    src.component(id, sel).unwrap_or(Json::Null)
}

fn compare(v: &Json, cmp: Cmp, j: &Json) -> bool {
    if let (Some(a), Some(b)) = (v.as_f64(), j.as_f64()) {
        return match cmp {
            Cmp::Eq => a == b,
            Cmp::Ne => a != b,
            Cmp::Lt => a < b,
            Cmp::Lte => a <= b,
            Cmp::Gt => a > b,
            Cmp::Gte => a >= b,
        };
    }
    let equal = same(v, j);
    match cmp {
        Cmp::Eq => equal,
        Cmp::Ne => !equal,
        _ => false,
    }
}

/// Equal as values: numbers by value at any depth, so a vec2 `[12, 12]`
/// is the position `[12.0, 12.0]` (found at the phase 5 gate: a list of
/// whole numbers, as a script or a shell writes it, matched nothing).
fn same(a: &Json, b: &Json) -> bool {
    match (a, b) {
        (Json::Number(x), Json::Number(y)) => x.as_f64() == y.as_f64(),
        (Json::Array(x), Json::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same(p, q))
        }
        (Json::Object(x), Json::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, p)| y.get(k).is_some_and(|q| same(p, q)))
        }
        _ => a == b,
    }
}

/// A `bad cursor` code, exported so the daemon's error table can name it.
pub const BAD_CURSOR: u32 = 4706;

/// Dummy use so `codes` stays referenced when the crate has no other user.
#[allow(dead_code)]
const _: u32 = codes::INDEX;
