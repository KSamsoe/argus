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

/// Run a query.
pub fn run(src: &dyn Source, params: &Json) -> Result<Json> {
    let select = str_list(params, "select")?;
    let select = if select.is_empty() {
        vec!["id".to_string()]
    } else {
        select
    };
    for s in &select {
        if s == "id" || s == "entity" {
            continue;
        }
        if s.contains('.') {
            src.require_path(s)?;
        } else {
            src.require_component(s)?;
        }
    }

    let empty = json!({});
    let where_ = params.get("where").unwrap_or(&empty);
    let Some(wobj) = where_.as_object() else {
        return Err(bad_params("where must be an object"));
    };
    let has = str_list(where_, "has")?;
    let not = str_list(where_, "not")?;
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
        if matches!(k.as_str(), "has" | "not" | "within") {
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

    if let Some(order) = opt_str(params, "order")? {
        if let Some(inner) = order
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

/// One selected column of one entity.
pub fn project(src: &dyn Source, id: u64, sel: &str) -> Json {
    if sel == "id" || sel == "entity" {
        return src.entity_ref(id);
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
    let equal = v == j;
    match cmp {
        Cmp::Eq => equal,
        Cmp::Ne => !equal,
        _ => false,
    }
}

/// A `bad cursor` code, exported so the daemon's error table can name it.
pub const BAD_CURSOR: u32 = 4706;

/// Dummy use so `codes` stays referenced when the crate has no other user.
#[allow(dead_code)]
const _: u32 = codes::INDEX;
