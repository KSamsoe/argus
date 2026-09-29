//! `key=value` words to a method's (or a tool's) params object, guided by
//! its declared parameters in the one schema (`argus_daemon::docs`).

pub use argus_client::words::value;
use argus_client::words::{insert, split_kv};
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
