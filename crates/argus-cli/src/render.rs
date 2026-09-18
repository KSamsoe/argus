//! Results as text for a terminal. Row results (`{columns, rows}`) become
//! aligned tables; everything else is pretty JSON. `--json` bypasses this.

use serde_json::{json, Value as Json};
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

/// A `test.run` report as the compact text of §9: one line per scenario,
/// a line per failing seed with the expectation that failed, a summary.
pub fn test_report(r: &Json) -> String {
    let mut out = String::new();
    for e in r["load_errors"].as_array().into_iter().flatten() {
        let _ = writeln!(
            out,
            "LOAD  {}: {}",
            e["file"].as_str().unwrap_or("?"),
            e["message"].as_str().unwrap_or("")
        );
    }
    for sc in r["scenarios"].as_array().into_iter().flatten() {
        let seeds = sc["seeds"].as_array().cloned().unwrap_or_default();
        let failed: Vec<&Json> = seeds.iter().filter(|s| s["ok"] != json!(true)).collect();
        let ms: f64 = seeds.iter().filter_map(|s| s["ms"].as_f64()).sum();
        let seed_text = match seeds.len() {
            0 => "no seeds".to_string(),
            1 => format!("seed {}", cell(&seeds[0]["seed"])),
            n => format!(
                "{n} seeds {}..{}: {} ok, {} failed",
                cell(&seeds[0]["seed"]),
                cell(&seeds[n - 1]["seed"]),
                n - failed.len(),
                failed.len()
            ),
        };
        let _ = writeln!(
            out,
            "{}  {}  ({}, {seed_text}, {:.0} ms)",
            if failed.is_empty() { "PASS" } else { "FAIL" },
            sc["name"].as_str().unwrap_or("?"),
            sc["scene"].as_str().unwrap_or("?"),
            ms
        );
        for s in failed.iter().take(10) {
            let seed = cell(&s["seed"]);
            if let Some(e) = s["error"].as_str() {
                let _ = writeln!(out, "    seed {seed}: error: {e}");
            }
            for ex in s["expectations"].as_array().into_iter().flatten() {
                if ex["ok"] != json!(true) {
                    let _ = writeln!(
                        out,
                        "    seed {seed}: {}",
                        ex["message"].as_str().unwrap_or("?")
                    );
                }
            }
        }
        if failed.len() > 10 {
            let _ = writeln!(out, "    ... and {} more failing seeds", failed.len() - 10);
        }
    }
    let n = r["scenarios"].as_array().map(Vec::len).unwrap_or(0);
    let _ = writeln!(
        out,
        "{n} scenario{}, {} run{}, {} failed, {:.0} ms",
        if n == 1 { "" } else { "s" },
        r["total_runs"],
        if r["total_runs"] == json!(1) { "" } else { "s" },
        r["failed_runs"],
        r["ms"].as_f64().unwrap_or(0.0)
    );
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
