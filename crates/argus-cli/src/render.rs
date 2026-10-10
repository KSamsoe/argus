//! Results as text for a terminal. Row results (`{columns, rows}`) become
//! aligned tables; everything else is pretty JSON. `--json` bypasses this.

use serde_json::{json, Value as Json};
use std::fmt::Write;

/// A result for a human.
pub fn result(v: &Json) -> String {
    let Some(obj) = v.as_object() else {
        return pretty(v);
    };
    if obj.get("format").and_then(Json::as_str) == Some("semantic") {
        return semantic(v);
    }
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
/// a line per failing seed with the expectation that failed, a summary. A
/// sweep's is a line per candidate (phase 4 item 3: "which ones starve").
pub fn test_report(r: &Json) -> String {
    let mut out = String::new();
    if let Some(cands) = r["candidates"].as_array() {
        let s = &r["sweep"];
        let target = s["entity"].as_str().or(s["def"].as_str()).unwrap_or("?");
        let _ = writeln!(out, "SWEEP {target} {}", s["path"].as_str().unwrap_or("?"));
        let mut bad = 0;
        for c in cands {
            let ok = c["ok"] == json!(true);
            if !ok {
                bad += 1;
            }
            let first = c["failing"][0].clone();
            let why = if first.is_null() {
                String::new()
            } else {
                // A run that errored says why in error; one whose
                // expectations failed says which in failed.
                let said = match first["error"].as_str().filter(|e| !e.is_empty()) {
                    Some(e) => e.to_string(),
                    None => cell(&first["failed"]),
                };
                format!(
                    "  first: {} seed {}: {}",
                    first["scenario"].as_str().unwrap_or("?"),
                    cell(&first["seed"]),
                    said
                )
            };
            let _ = writeln!(
                out,
                "{}  {:<8} {} runs, {} failed  ({:.1} s){why}",
                if ok { "PASS" } else { "FAIL" },
                cell(&c["value"]),
                cell(&c["runs"]),
                cell(&c["failed"]),
                c["ms"].as_f64().unwrap_or(0.0) / 1000.0
            );
        }
        let _ = writeln!(
            out,
            "{} candidates, {bad} failed, {:.1} s",
            cands.len(),
            r["ms"].as_f64().unwrap_or(0.0) / 1000.0
        );
        return out;
    }
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
            let r = &s["replay"];
            if let Some(dir) = r["dir"].as_str() {
                let clip = if r["clip"].is_string() {
                    ", with a clip"
                } else {
                    ""
                };
                let _ = writeln!(
                    out,
                    "    seed {seed}: replay at frame {}{clip}: argus replay open path={dir}",
                    cell(&r["frame"])
                );
            } else if let Some(e) = r["error"].as_str() {
                let _ = writeln!(out, "    seed {seed}: no replay: {e}");
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

/// A semantic frame as lines to read, in the frame's own order: the view,
/// the HUD, the entities one by one, the groups, the effects, the tiles.
/// What a narrator would say, and what a person reads at a terminal.
pub fn semantic(v: &Json) -> String {
    let mut out = String::new();
    let b = |x: &Json| match x.as_array() {
        Some(a) if a.len() == 4 => format!(
            "({},{})-({},{})",
            cell(&a[0]),
            cell(&a[1]),
            cell(&a[2]),
            cell(&a[3])
        ),
        _ => String::new(),
    };
    let rect = &v["camera"]["rect"];
    let _ = writeln!(
        out,
        "Tick {}, {}x{} px. Camera {} shows x {} to {}, y {} to {}.",
        cell(&v["tick"]),
        cell(&v["size"][0]),
        cell(&v["size"][1]),
        cell(&v["camera"]["id"]),
        cell(&rect[0]),
        cell(&rect[2]),
        cell(&rect[1]),
        cell(&rect[3]),
    );
    for h in v["hud"].as_array().into_iter().flatten() {
        let _ = writeln!(out, "HUD {}: {}", cell(&h["id"]), cell(&h["text"]));
    }
    for u in v["ui"].as_array().into_iter().flatten() {
        let mut how = String::new();
        if u["button"] == true {
            how.push_str(" button");
        }
        if u["focused"] == true {
            how.push_str(", focused");
        }
        if u["disabled"] == true {
            how.push_str(", disabled");
        }
        let _ = writeln!(
            out,
            "UI {}{how}: {}  at {}",
            cell(&u["id"]),
            cell(&u["text"]),
            b(&u["bounds"])
        );
    }
    for e in v["entities"].as_array().into_iter().flatten() {
        let (id, name, text) = (cell(&e[0]), cell(&e[1]), cell(&e[2]));
        let what = match (name.is_empty(), text.is_empty()) {
            (true, true) => String::new(),
            (false, true) => format!(" {name}"),
            (true, false) => format!(": {text}"),
            (false, false) => format!(" {name}: {text}"),
        };
        let _ = writeln!(out, "{id}{what}  at {}  {}", b(&e[3]), cell(&e[5]));
    }
    for g in v["groups"].as_array().into_iter().flatten() {
        let _ = writeln!(
            out,
            "{} x{}  in {}  {}",
            cell(&g["what"]),
            cell(&g["count"]),
            b(&g["bounds"]),
            cell(&g["look"])
        );
    }
    if v["omitted"].as_u64().unwrap_or(0) > 0 {
        let _ = writeln!(out, "and {} more kinds not listed", cell(&v["omitted"]));
    }
    for fx in v["effects"].as_array().into_iter().flatten() {
        let line = match fx["kind"].as_str().unwrap_or_default() {
            "particles" => format!(
                "Particles {} x{} from {} in {}",
                cell(&fx["effect"]),
                cell(&fx["count"]),
                cell(&fx["emitter"]),
                b(&fx["bounds"])
            ),
            "shake" => format!(
                "Camera {} shakes, trauma {}",
                cell(&fx["camera"]),
                cell(&fx["trauma"])
            ),
            "material" => format!(
                "Material {} on {} sprites",
                cell(&fx["material"]),
                cell(&fx["sprites"])
            ),
            _ => fx.to_string(),
        };
        let _ = writeln!(out, "{line}");
    }
    // 2D lighting (phase 5 item 3): only when the camera has it.
    let lit = &v["lighting"];
    if lit.is_object() {
        let _ = writeln!(
            out,
            "Lit: ambient {} at {}{}",
            cell(&lit["ambient"]),
            cell(&lit["level"]),
            if lit["shadows"] == true {
                ", with shadows"
            } else {
                ", no shadows"
            }
        );
        for l in lit["lights"].as_array().into_iter().flatten() {
            let mut how = format!(
                "{} {} at ({}, {}), {} px, {} x{}",
                cell(&l["kind"]),
                cell(&l["id"]),
                cell(&l["at"][0]),
                cell(&l["at"][1]),
                cell(&l["radius_px"]),
                cell(&l["color"]),
                cell(&l["intensity"])
            );
            if l["kind"] == "cone" {
                how.push_str(&format!(
                    ", pointing {} degrees, {} each side",
                    cell(&l["direction"]),
                    cell(&l["angle"])
                ));
            }
            if l["shadows"] == false {
                how.push_str(", no shadows");
            }
            if l.get("flicker").is_some() {
                how.push_str(&format!(", flickering {}", cell(&l["flicker"])));
            }
            let _ = writeln!(out, "Light {how}");
        }
        if lit["omitted"].as_u64().unwrap_or(0) > 0 {
            let _ = writeln!(out, "and {} more lights", cell(&lit["omitted"]));
        }
    }
    for m in v["tiles"]["maps"].as_array().into_iter().flatten() {
        let _ = writeln!(
            out,
            "Tiles of {} ({}) from cell ({},{}), one character a cell ({}):",
            cell(&m["entity"]),
            cell(&m["map"]),
            cell(&m["cell"][0]),
            cell(&m["cell"][1]),
            cell(&v["tiles"]["key"]),
        );
        for r in m["rows"].as_array().into_iter().flatten() {
            let _ = writeln!(out, "  {}", cell(r));
        }
    }
    for p in v["problems"].as_array().into_iter().flatten() {
        let _ = writeln!(out, "Problem: {}", cell(p));
    }
    let _ = writeln!(out, "({} bytes)", cell(&v["bytes"]));
    out
}

/// `argus projects`: the open project, the recent ones, those under a
/// folder, and the templates.
pub fn projects(r: &Json) -> String {
    let mut out = String::new();
    let line = |out: &mut String, p: &Json| {
        let _ = writeln!(out, "  {:<24} {}", cell(&p["name"]), cell(&p["path"]));
    };
    if !r["open"].is_null() {
        let _ = writeln!(out, "here:");
        line(&mut out, &r["open"]);
    }
    let recent = r["recent"].as_array().cloned().unwrap_or_default();
    if recent.is_empty() {
        let _ = writeln!(out, "no projects opened yet; `argus new <dir>` makes one");
    } else {
        let _ = writeln!(out, "recent:");
        for p in &recent {
            line(&mut out, p);
        }
    }
    if let Some(under) = r["under"].as_array() {
        let _ = writeln!(out, "under the folder: {}", under.len());
        for p in under {
            line(&mut out, p);
        }
    }
    let _ = writeln!(out, "templates (argus new <dir> --template <name>):");
    for t in r["templates"].as_array().into_iter().flatten() {
        let _ = writeln!(out, "  {:<12} {}", cell(&t["name"]), cell(&t["about"]));
    }
    out
}
