//! Text with holes in it: `{Component.field}` and `{@id:Component.field}`
//! filled from the world. A `Describe` (phase 1 item 12) and a UI text
//! (phase 3 item 1) say things like "HP {Health.cur}/{Health.max}" this way;
//! the semantic frame and the UI layout fill them from the same code.

use crate::{Entity, Value, World};

/// The protocol's spelling of an entity: `@id` for a named one, else `e:N`.
pub fn entity_id(world: &World, e: Entity) -> String {
    match world.string_id(e) {
        Some(id) => format!("@{id}"),
        None => e.to_string(),
    }
}

/// A value as text: whole numbers bare, others to two places, vectors as
/// `(x, y)`, entities by id.
pub fn show(world: &World, v: &Value) -> String {
    let num = |f: f64| {
        if f.fract() == 0.0 && f.abs() < 1e15 {
            format!("{}", f as i64)
        } else {
            let s = format!("{f:.2}");
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        }
    };
    match v {
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::UInt(u) => u.to_string(),
        Value::Float(f) => num(*f),
        Value::Vec2(p) => format!("({}, {})", num(p.x as f64), num(p.y as f64)),
        Value::Str(s) | Value::Ref(s) => s.clone(),
        Value::Entity(e) => entity_id(world, *e),
    }
}

/// Fill a text's `{Component.field}` holes (from `e`) and
/// `{@id:Component.field}` holes (from that entity). A hole that names
/// nothing reads `{?...}` and is reported in `problems`, once.
pub fn fill(world: &World, e: Entity, text: &str, problems: &mut Vec<String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let Some(j) = rest[i..].find('}') else {
            out.push_str(&rest[i..]);
            return out;
        };
        let hole = &rest[i + 1..i + j];
        let (who, path) = match hole.strip_prefix('@').and_then(|h| h.split_once(':')) {
            Some((id, p)) => (world.entity(&format!("@{id}")).ok(), p),
            None => (Some(e), hole),
        };
        let got = who.ok_or_else(|| format!("no entity {}", hole.split(':').next().unwrap_or("")));
        match got.and_then(|w| world.get_field(w, path).map_err(|e| e.message)) {
            Ok(v) => out.push_str(&show(world, &v)),
            Err(why) => {
                out.push_str(&format!("{{?{hole}}}"));
                let p = format!("{}: {{{hole}}}: {why}", entity_id(world, e));
                if !problems.contains(&p) {
                    problems.push(p);
                }
            }
        }
        rest = &rest[i + j + 1..];
    }
    out.push_str(rest);
    out
}
