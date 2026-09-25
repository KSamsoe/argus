//! `key=value` words to a method's (or a tool's) params object, guided by
//! its declared parameters in the one schema (`argus_daemon::docs`).

use argus_daemon::{docs, METHODS};
use serde_json::{Map, Value as Json};

/// Parameter names of a method in declared order: the order bare values
/// fill them.
pub fn names(method: &str) -> Vec<String> {
    docs::find(method)
        .map(|m| m.params.iter().map(|p| p.name.to_string()).collect())
        .unwrap_or_default()
}

/// A tool's parameter names in declared order.
pub fn tool_names(tool: &str) -> Vec<String> {
    docs::tool(tool)
        .map(|t| t.params.iter().map(|p| p.name.to_string()).collect())
        .unwrap_or_default()
}

/// Build the params object: `--params` JSON first, then `key=value`
/// words, then bare words assigned to the method's leading parameters.
pub fn build(method: &str, args: &[String], params_json: &[String]) -> Result<Json, String> {
    if !METHODS.iter().any(|m| m.name == method) {
        return Err(format!("no method {method}"));
    }
    build_with(method, &names(method), args, params_json)
}

/// [`build`] for a tool (`argus query entity=@player`).
pub fn build_tool(tool: &str, args: &[String], params_json: &[String]) -> Result<Json, String> {
    build_with(tool, &tool_names(tool), args, params_json)
}

fn build_with(
    method: &str,
    declared: &[String],
    args: &[String],
    params_json: &[String],
) -> Result<Json, String> {
    let mut obj = Map::new();
    for text in params_json {
        let text = if let Some(path) = text.strip_prefix('@') {
            std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?
        } else {
            text.clone()
        };
        let v: Json = serde_json::from_str(&text)
            .map_err(|e| format!("--params is not a JSON object: {e}"))?;
        match v {
            Json::Object(m) => obj.extend(m),
            _ => return Err("--params must be a JSON object".to_string()),
        }
    }
    let mut next_bare = 0usize;
    let mut by_word: std::collections::HashSet<String> = std::collections::HashSet::new();
    for a in args {
        if let Some((k, v)) = split_kv(a) {
            insert(&mut obj, k, value(v)?, &mut by_word)?;
        } else {
            let Some(name) = declared.get(next_bare) else {
                return Err(format!(
                    "{method} takes {} bare value(s); {a:?} has no parameter to go to. Use key=value.",
                    declared.len().min(next_bare)
                ));
            };
            next_bare += 1;
            if obj.contains_key(name) {
                return Err(format!("{name} given twice (bare {a:?} and {name}=...)"));
            }
            obj.insert(name.to_string(), value(a)?);
        }
    }
    Ok(Json::Object(obj))
}

/// `key=value` if the word is one: a key is an identifier path, so
/// `@player`, `2`, and `[a, b]` are bare values even with an `=` inside.
fn split_kv(a: &str) -> Option<(&str, &str)> {
    let i = a.find('=')?;
    let k = &a[..i];
    if k.is_empty()
        || !k
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
        || k.starts_with('.')
    {
        return None;
    }
    Some((k, &a[i + 1..]))
}

/// Insert at a dotted path, making objects on the way. A key repeated
/// among the words turns the value into a list; a word replaces what
/// `--params` gave for the same key.
fn insert(
    obj: &mut Map<String, Json>,
    key: &str,
    v: Json,
    by_word: &mut std::collections::HashSet<String>,
) -> Result<(), String> {
    let repeated = !by_word.insert(key.to_string());
    let mut parts = key.split('.').peekable();
    let mut cur = obj;
    while let Some(p) = parts.next() {
        if parts.peek().is_none() {
            match cur.get_mut(p) {
                Some(Json::Array(a)) if repeated => a.push(v),
                Some(old) if repeated => {
                    let first = old.take();
                    *old = Json::Array(vec![first, v]);
                }
                _ => {
                    cur.insert(p.to_string(), v);
                }
            }
            return Ok(());
        }
        let slot = cur
            .entry(p.to_string())
            .or_insert_with(|| Json::Object(Map::new()));
        match slot {
            Json::Object(m) => cur = m,
            _ => return Err(format!("{key}: {p} is already a plain value")),
        }
    }
    Ok(())
}

/// A word's value: JSON when it parses as JSON, else JSON with bare words
/// quoted when it is bracketed, else the string itself.
pub fn value(s: &str) -> Result<Json, String> {
    if let Ok(v) = serde_json::from_str::<Json>(s) {
        return Ok(v);
    }
    let t = s.trim();
    if t.starts_with('[') || t.starts_with('{') {
        let relaxed = quote_bare(t);
        return serde_json::from_str::<Json>(&relaxed)
            .map_err(|e| format!("cannot read {s:?} as a list or object: {e}"));
    }
    Ok(Json::String(s.to_string()))
}

/// Quote bare words in a bracketed literal: `[Enemy, @player]` becomes
/// `["Enemy", "@player"]`; `{of: @player, r: 6}` becomes JSON. Numbers,
/// `true`, `false`, and `null` stay as they are.
fn quote_bare(s: &str) -> String {
    let cs: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() + 16);
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if c == '"' {
            // Copy a quoted string through untouched.
            out.push(c);
            i += 1;
            while i < cs.len() {
                out.push(cs[i]);
                if cs[i] == '\\' && i + 1 < cs.len() {
                    out.push(cs[i + 1]);
                    i += 2;
                    continue;
                }
                i += 1;
                if cs[i - 1] == '"' {
                    break;
                }
            }
            continue;
        }
        if is_bare_start(c) {
            let mut word = String::new();
            while i < cs.len() {
                let d = cs[i];
                // A colon stays in the word only when glued on both sides
                // (`item:key.brass`); `of: @player` is a key and a value.
                let glued_colon =
                    d == ':' && i + 1 < cs.len() && is_bare_start(cs[i + 1]) && !word.is_empty();
                if is_bare_start(d) || glued_colon {
                    word.push(d);
                    i += 1;
                } else {
                    break;
                }
            }
            let keep =
                matches!(word.as_str(), "true" | "false" | "null") || word.parse::<f64>().is_ok();
            if keep {
                out.push_str(&word);
            } else {
                out.push('"');
                out.push_str(&word);
                out.push('"');
            }
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

fn is_bare_start(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '@' | '_' | '-' | '+' | '.' | '/' | '#')
}
