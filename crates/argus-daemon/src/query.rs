//! `world.query` (§5 "Shape of a query") and the paging contract every
//! read method shares: `limit` defaulting to 50, `total` always present,
//! `truncated`, and a `next` cursor.

use crate::codes;
use crate::json::{self, bad_params};
use argus_ecs::{ArgusError, Entity, Result, Value, World};
use serde_json::{json, Value as Json};

/// Default page size (§5).
pub const DEFAULT_LIMIT: u64 = 50;

/// Read `limit` and `cursor` from params.
pub fn page(params: &Json) -> Result<(usize, usize)> {
    let limit = json::u64_param(params, "limit", DEFAULT_LIMIT)? as usize;
    let offset = match params.get("cursor") {
        None | Some(Json::Null) => 0,
        Some(Json::String(c)) => c
            .strip_prefix("c:")
            .and_then(|n| n.parse::<usize>().ok())
            .ok_or_else(|| {
                ArgusError::new(
                    codes::BAD_CURSOR,
                    format!("{c:?} is not a cursor from this daemon"),
                )
                .hint("Pass the next value from the previous response unchanged.")
            })?,
        Some(_) => return Err(bad_params("cursor must be a string")),
    };
    Ok((limit, offset))
}

/// Slice `rows` into a page and build the standard envelope. Extra
/// top-level fields can be merged by the caller.
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

/// Run a query against the live world.
pub fn run(world: &World, params: &Json) -> Result<Json> {
    let registry = world.registry();
    let select = json::str_list(params, "select")?;
    let select = if select.is_empty() {
        vec!["id".to_string()]
    } else {
        select
    };
    // Validate select paths up front so an error names the bad one.
    for s in &select {
        if s == "id" || s == "entity" {
            continue;
        }
        if s.contains('.') {
            registry.resolve_path(s)?;
        } else {
            registry.require(s)?;
        }
    }

    let empty = json!({});
    let where_ = params.get("where").unwrap_or(&empty);
    if !where_.is_object() {
        return Err(bad_params("where must be an object"));
    }
    let has: Vec<_> = json::str_list(where_, "has")?
        .iter()
        .map(|n| registry.require(n))
        .collect::<Result<_>>()?;
    let not: Vec<_> = json::str_list(where_, "not")?
        .iter()
        .map(|n| registry.require(n))
        .collect::<Result<_>>()?;
    let within = match where_.get("within") {
        None | Some(Json::Null) => None,
        Some(w) => {
            let of = json::str_param(w, "of")?;
            let r = json::f64_param(w, "r", 0.0)?;
            let center = position(world, world.entity(of)?).ok_or_else(|| {
                ArgusError::new(
                    argus_ecs::codes::MISSING_COMPONENT,
                    format!("{of} has no Transform, so within cannot be measured from it"),
                )
            })?;
            Some((center, r))
        }
    };
    let mut preds = Vec::new();
    for (k, v) in where_.as_object().expect("checked") {
        if matches!(k.as_str(), "has" | "not" | "within") {
            continue;
        }
        registry.resolve_path(k)?;
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

    // Candidates.
    let mut q = world.query();
    for c in &has {
        q = q.has(*c);
    }
    for c in &not {
        q = q.not(*c);
    }
    let mut rows: Vec<(Entity, Option<f64>)> = Vec::new();
    'ent: for e in q.entities() {
        for p in &preds {
            let Ok(v) = world.get_field(e, &p.path) else {
                continue 'ent;
            };
            if !compare(&v, p.cmp, &p.value, world) {
                continue 'ent;
            }
        }
        let mut dist = None;
        if let Some((center, r)) = within {
            let Some(pos) = position(world, e) else {
                continue;
            };
            let d = ((pos.0 - center.0).powi(2) + (pos.1 - center.1).powi(2)).sqrt();
            if d > r {
                continue;
            }
            dist = Some(d);
        }
        rows.push((e, dist));
    }

    // Order.
    if let Some(order) = json::opt_str(params, "order")? {
        if let Some(inner) = order
            .strip_prefix("dist(")
            .and_then(|s| s.strip_suffix(')'))
        {
            let center = position(world, world.entity(inner)?).ok_or_else(|| {
                ArgusError::new(
                    argus_ecs::codes::MISSING_COMPONENT,
                    format!("{inner} has no Transform to measure distance from"),
                )
            })?;
            for (e, d) in rows.iter_mut() {
                *d = position(world, *e)
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
            registry.resolve_path(path)?;
            let mut keyed: Vec<(Option<f64>, Entity, Option<f64>)> = rows
                .iter()
                .map(|(e, d)| {
                    (
                        world.get_field(*e, path).ok().and_then(|v| v.as_f64()),
                        *e,
                        *d,
                    )
                })
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
            rows = keyed.into_iter().map(|(_, e, d)| (e, d)).collect();
        }
    } else {
        rows.sort_by_key(|r| r.0);
    }

    let (limit, offset) = page(params)?;
    let total = rows.len();
    let end = (offset + limit).min(total);
    let mut out_rows = Vec::new();
    if offset < total {
        for (e, _) in &rows[offset..end] {
            let mut row = Vec::with_capacity(select.len());
            for s in &select {
                row.push(project(world, *e, s));
            }
            out_rows.push(Json::Array(row));
        }
    }
    let truncated = end < total;
    let mut out = json!({
        "frame": world.frame(),
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
pub fn project(world: &World, e: Entity, sel: &str) -> Json {
    if sel == "id" || sel == "entity" {
        return json::entity_ref(world, e);
    }
    if sel.contains('.') {
        return world
            .get_field(e, sel)
            .map(|v| json::to_json(world, &v))
            .unwrap_or(Json::Null);
    }
    let Some(c) = world.component_id(sel) else {
        return Json::Null;
    };
    match world.get_component(e, c) {
        Ok(fields) => Json::Object(
            fields
                .into_iter()
                .map(|(k, v)| (k, json::to_json(world, &v)))
                .collect(),
        ),
        Err(_) => Json::Null,
    }
}

/// World position: `GlobalTransform.pos` when present, else `Transform.pos`.
pub fn position(world: &World, e: Entity) -> Option<(f64, f64)> {
    for path in ["GlobalTransform.pos", "Transform.pos"] {
        if let Ok(Value::Vec2(p)) = world.get_field(e, path) {
            return Some((p.x as f64, p.y as f64));
        }
    }
    None
}

fn compare(v: &Value, cmp: Cmp, j: &Json, world: &World) -> bool {
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
    let equal = json::to_json(world, v) == *j
        || matches!((v, j), (Value::Entity(e), Json::String(s)) if world.entity(s).ok() == Some(*e));
    match cmp {
        Cmp::Eq => equal,
        Cmp::Ne => !equal,
        _ => false,
    }
}
