//! Results as text for a terminal. Row results (`{columns, rows}`) become
//! aligned tables; everything else is pretty JSON. `--json` bypasses this.

use serde_json::Value as Json;
use std::fmt::Write;

/// A result for a human.
pub fn result(v: &Json) -> String {
    let Some(obj) = v.as_object() else {
        return pretty(v);
    };
    let Some(rows) = obj.get("rows").and_then(Json::as_array) else {
        return pretty(v);
    };
    let mut out = String::new();
    // Scalars first: total, next, source, scene, and the like.
    for (k, val) in obj {
        if k == "rows" || k == "columns" {
            continue;
        }
        if val.is_array() || val.is_object() {
            let _ = writeln!(out, "{k}: {val}");
        } else {
            let _ = writeln!(out, "{k}: {}", cell(val));
        }
    }
    let columns: Vec<String> = match obj.get("columns").and_then(Json::as_array) {
        Some(c) => c.iter().map(cell).collect(),
        None => match rows.first() {
            Some(Json::Object(first)) => first.keys().cloned().collect(),
            _ => Vec::new(),
        },
    };
    let table: Vec<Vec<String>> = rows
        .iter()
        .map(|r| match r {
            Json::Array(a) => a.iter().map(cell).collect(),
            Json::Object(o) => columns
                .iter()
                .map(|c| o.get(c).map(cell).unwrap_or_default())
                .collect(),
            other => vec![cell(other)],
        })
        .collect();
    if columns.is_empty() && table.is_empty() {
        out.push_str("(no rows)\n");
        return out;
    }
    let width = columns
        .len()
        .max(table.iter().map(Vec::len).max().unwrap_or(0));
    let mut widths = vec![0usize; width];
    for (i, c) in columns.iter().enumerate() {
        widths[i] = widths[i].max(c.chars().count());
    }
    for r in &table {
        for (i, c) in r.iter().enumerate() {
            widths[i] = widths[i].max(c.chars().count().min(60));
        }
    }
    if !columns.is_empty() {
        out.push_str(&line(&columns, &widths));
        let rule: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
        out.push_str(&line(&rule, &widths));
    }
    for r in &table {
        out.push_str(&line(r, &widths));
    }
    out
}

fn line(cells: &[String], widths: &[usize]) -> String {
    let mut s = String::new();
    for (i, w) in widths.iter().enumerate() {
        let c = cells.get(i).map(String::as_str).unwrap_or("");
        if i + 1 == widths.len() {
            s.push_str(c);
        } else {
            let _ = write!(s, "{c:<w$}  ", w = *w);
        }
    }
    s.trim_end().to_string() + "\n"
}

/// One cell: strings bare, numbers short, the rest compact JSON.
pub fn cell(v: &Json) -> String {
    match v {
        Json::String(s) => s.clone(),
        Json::Null => String::new(),
        Json::Number(n) => match n.as_f64() {
            Some(f) if n.is_f64() => {
                let s = format!("{f:.3}");
                s.trim_end_matches('0').trim_end_matches('.').to_string()
            }
            _ => n.to_string(),
        },
        other => other.to_string(),
    }
}

fn pretty(v: &Json) -> String {
    match v {
        Json::String(s) => format!("{s}\n"),
        other => serde_json::to_string_pretty(other).unwrap_or_default() + "\n",
    }
}

/// A JSON-RPC error for stderr: code, sentence, hint, file:line.
pub fn error(e: &Json) -> String {
    let mut out = format!(
        "error {}: {}\n",
        e["code"],
        e["message"].as_str().unwrap_or("")
    );
    if let Some(h) = e["data"]["hint"].as_str() {
        let _ = writeln!(out, "  hint: {h}");
    }
    if let Some(r) = e["data"]["ref"].as_str() {
        let _ = writeln!(out, "  at: {r}");
    }
    out
}

/// A server notification, one line: `event death {...}` or the JSON.
pub fn notification(n: &Json, as_json: bool) -> String {
    if as_json {
        return n.to_string();
    }
    let method = n["method"].as_str().unwrap_or("?");
    let p = &n["params"];
    match method {
        "event" => format!(
            "event {} #{} frame {} {}",
            p["name"].as_str().unwrap_or("?"),
            p["seq"],
            p["frame"],
            p["fields"]
        ),
        "edit.committed" => format!(
            "commit #{} by {} {:?} touched {}",
            p["id"],
            p["author"].as_str().unwrap_or("?"),
            p["note"].as_str().unwrap_or(""),
            p["touched"]
        ),
        _ => format!("{method} {p}"),
    }
}
