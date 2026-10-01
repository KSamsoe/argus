//! The agent tools (§16): about a dozen verbs over the protocol, described
//! once in `argus_daemon::docs::TOOLS`. `argus mcp` serves them to an MCP
//! client and `argus <tool>` runs them from a shell; both come here, so a
//! tool call in an agent's transcript reruns as the same command line.

use crate::client::Client;
use argus_daemon::docs::{self, ToolDoc};
use serde_json::{json, Map, Value as Json};

/// Something that answers protocol calls: the daemon over its socket, or a
/// daemon in the same process (tests).
pub trait Caller {
    /// Call a method: its result, or the error object the daemon sent.
    fn call(&mut self, method: &str, params: Json) -> Result<Json, Json>;
}

impl Caller for Client {
    fn call(&mut self, method: &str, params: Json) -> Result<Json, Json> {
        match Client::call(self, method, params) {
            Err(e) => Err(json!({ "code": -32000, "message": format!("connection lost: {e}") })),
            Ok(resp) => match resp.get("error") {
                Some(e) => Err(e.clone()),
                None => Ok(resp.get("result").cloned().unwrap_or(Json::Null)),
            },
        }
    }
}

/// A bad tool call, as the daemon would report bad params.
fn bad(msg: impl Into<String>) -> Json {
    argus_daemon::json::error_json(&argus_daemon::json::bad_params(msg))
}

/// The given keys of `args` that are present, as a params object.
fn pick(args: &Json, keys: &[&str]) -> Json {
    let mut o = Map::new();
    for k in keys {
        if let Some(v) = args.get(*k).filter(|v| !v.is_null()) {
            o.insert((*k).to_string(), v.clone());
        }
    }
    Json::Object(o)
}

fn flag(args: &Json, key: &str) -> bool {
    args.get(key).and_then(Json::as_bool).unwrap_or(false)
}

/// Refuse keys the tool does not take, naming the ones it does.
fn check(t: &ToolDoc, args: &Json) -> Result<(), Json> {
    let Some(o) = args.as_object() else {
        return if args.is_null() {
            Ok(())
        } else {
            Err(bad(format!("{} takes an object of parameters", t.name)))
        };
    };
    for k in o.keys() {
        if !t.params.iter().any(|p| p.name == k) {
            let names: Vec<&str> = t.params.iter().map(|p| p.name).collect();
            return Err(bad(format!(
                "{} takes no {k:?}; it takes {}",
                t.name,
                if names.is_empty() {
                    "nothing".to_string()
                } else {
                    names.join(", ")
                }
            )));
        }
    }
    Ok(())
}

/// Run a tool with its arguments.
pub fn run(c: &mut dyn Caller, name: &str, args: &Json) -> Result<Json, Json> {
    let t = docs::tool(name).ok_or_else(|| {
        let names: Vec<&str> = docs::TOOLS.iter().map(|t| t.name).collect();
        bad(format!(
            "no tool {name:?}; the tools are {}",
            names.join(", ")
        ))
    })?;
    check(t, args)?;
    let args = if args.is_null() { &json!({}) } else { args };
    match name {
        "summary" => c.call("world.summary", pick(args, &["scene"])),
        "query" => {
            if let Some(s) = args.get("strings") {
                let mut p = if s.is_object() { s.clone() } else { json!({}) };
                if let Some(l) = s.as_str() {
                    p["lang"] = json!(l);
                }
                c.call("strings.list", p)
            } else if args.get("mods").and_then(Json::as_bool) == Some(true) {
                c.call("mods.list", json!({}))
            } else if let Some(t) = args.get("timelines").filter(|v| !v.is_null()) {
                match t.as_str() {
                    Some(name) => c.call("timeline.list", json!({ "name": name })),
                    None => c.call("timeline.list", json!({})),
                }
            } else if let Some(p) = args.get("path") {
                c.call("nav.path", p.clone())
            } else if args.get("entity").is_some() {
                c.call("world.get", pick(args, &["entity", "scene"]))
            } else {
                c.call(
                    "world.query",
                    pick(
                        args,
                        &["select", "where", "group_by", "order", "limit", "scene"],
                    ),
                )
            }
        }
        "describe" => {
            if flag(args, "systems") {
                c.call("script.systems", json!({ "limit": 1000 }))
            } else {
                let mut p = pick(args, &["component"]);
                p["limit"] = json!(1000);
                c.call("world.describe", p)
            }
        }
        "edit" => {
            if flag(args, "undo") {
                return c.call("edit.undo", json!({ "author": author(args) }));
            }
            let tx = transaction(c, args, "edit")?;
            // Ops are checked as they are queued and again at the commit.
            c.call("edit.commit", json!({ "tx": tx })).map_err(|e| {
                let _ = c.call("edit.abort", json!({ "tx": tx }));
                nothing_kept(e)
            })
        }
        "propose" => {
            if flag(args, "list") {
                return c.call("proposal.list", json!({}));
            }
            let tx = transaction(c, args, "propose")?;
            let mut p = pick(args, &["note", "proposal"]);
            p["tx"] = json!(tx);
            c.call("edit.propose", p).map_err(|e| {
                let _ = c.call("edit.abort", json!({ "tx": tx }));
                nothing_kept(e)
            })
        }
        "run" => {
            let mut did = Vec::new();
            let mut result = Json::Null;
            let mut step = |c: &mut dyn Caller, m: &str, p: Json| -> Result<(), Json> {
                result = c.call(m, p)?;
                did.push(m.to_string());
                Ok(())
            };
            if let Some(name) = args.get("load").and_then(Json::as_str) {
                step(c, "sim.load", json!({ "name": name }))?;
            }
            if flag(args, "stop") {
                step(c, "sim.stop", json!({}))?;
            }
            if flag(args, "play") {
                step(c, "sim.play", json!({}))?;
            }
            if flag(args, "pause") {
                step(c, "sim.pause", json!({}))?;
            }
            if let Some(x) = args.get("speed").filter(|v| !v.is_null()) {
                step(c, "sim.speed", json!({ "x": x }))?;
            }
            if let Some(n) = args.get("step").filter(|v| !v.is_null()) {
                step(c, "sim.step", json!({ "n": n }))?;
            }
            if let Some(u) = args.get("until").filter(|v| !v.is_null()) {
                if !u.is_object() {
                    return Err(bad("until is an object: {event}, {time}, or {ticks}"));
                }
                step(c, "sim.run_until", u.clone())?;
            }
            if let Some(n) = args.get("turns").filter(|v| !v.is_null()) {
                step(c, "sim.advance", json!({ "n": n }))?;
            }
            if let Some(name) = args.get("save").and_then(Json::as_str) {
                step(c, "sim.save", json!({ "name": name }))?;
            }
            let status = c.call("sim.status", json!({}))?;
            Ok(json!({ "did": did, "result": result, "status": status }))
        }
        "seek" => {
            if let Some(time) = args.get("time").filter(|v| !v.is_null()) {
                let Some(t) = args.get("timeline").and_then(Json::as_str) else {
                    return Err(bad(
                        "seek with time needs timeline: a timeline's name or entity",
                    ));
                };
                let mut p = json!({ "time": time });
                if t.starts_with("e:") || t.starts_with('@') {
                    p["timeline"] = json!(t);
                } else {
                    p["name"] = json!(t);
                }
                return c.call("timeline.seek", p);
            }
            if args.get("replays").and_then(Json::as_bool) == Some(true) {
                return c.call("replay.list", json!({}));
            }
            if let Some(path) = args.get("replay").and_then(Json::as_str) {
                return c.call("replay.open", json!({ "path": path }));
            }
            if args.get("frame").is_none() && args.get("turn").is_none() {
                // Nothing to go to: where the run can go.
                return c.call("sim.ring", json!({}));
            }
            match args.get("fork").and_then(Json::as_str) {
                Some(name) => {
                    let mut p = pick(args, &["frame", "turn"]);
                    p["name"] = json!(name);
                    c.call("sim.fork", p)
                }
                None => c.call("sim.seek", pick(args, &["frame", "turn"])),
            }
        }
        "observe" => {
            let mut p = pick(args, &["annotate", "width", "height"]);
            p["format"] = args.get("format").cloned().unwrap_or(json!("semantic"));
            c.call("obs.frame", p)
        }
        "events" => {
            if let Some(w) = args.get("why").filter(|v| !v.is_null()) {
                let p = match w {
                    Json::Number(_) => json!({ "event": w }),
                    Json::String(s) => json!({ "name": s }),
                    _ => return Err(bad("why is an event's sequence number or an event name")),
                };
                return c.call("obs.why", p);
            }
            if let Some(t) = args.get("trace").filter(|v| !v.is_null()) {
                if !t.is_object() {
                    return Err(bad("trace is an object: {entity, fields, frames}"));
                }
                return c.call("obs.trace", t.clone());
            }
            c.call(
                "obs.events",
                pick(args, &["name", "entity", "since", "limit"]),
            )
        }
        "test" => c.call(
            "test.run",
            pick(args, &["name", "file", "seeds", "update", "set", "sweep"]),
        ),
        "reload" => {
            if let Some(m) = args.get("mods").filter(|v| !v.is_null()) {
                let loaded = c.call("scene.load", json!({ "mods": m }))?;
                let mods = c.call("mods.list", json!({}))?;
                return Ok(
                    json!({ "scene": loaded["issues"].clone(), "run_from": loaded["run_from"].clone(), "mods": mods }),
                );
            }
            let scripts = c.call("script.reload", json!({}))?;
            let assets = c.call("asset.import", json!({}))?;
            let errors = c.call("script.errors", json!({}))?;
            Ok(json!({ "scripts": scripts, "assets": assets, "errors": errors }))
        }
        "review" => {
            if args.get("list").and_then(Json::as_bool) == Some(true)
                || args.get("question").is_none()
            {
                let all = args.get("all").and_then(Json::as_bool).unwrap_or(true);
                return c.call("review.list", json!({ "all": all }));
            }
            c.call(
                "obs.review",
                pick(args, &["question", "frames", "options", "note"]),
            )
        }
        "profile" => {
            let last = args.get("last").cloned().unwrap_or(json!(600));
            if let Some(n) = args.get("capture").filter(|v| !v.is_null()) {
                return c.call("prof.capture", json!({ "ticks": n }));
            }
            if let Some(n) = args.get("worst").filter(|v| !v.is_null()) {
                return c.call("prof.worst", json!({ "n": n }));
            }
            if args.get("calls").and_then(Json::as_bool) == Some(true) {
                return c.call("prof.calls", json!({}));
            }
            if args.get("systems").and_then(Json::as_bool) == Some(true) {
                let systems = c.call("prof.systems", json!({ "last": last }))?;
                let alloc = c.call("prof.alloc", json!({ "last": last }))?;
                return Ok(json!({ "systems": systems, "alloc": alloc }));
            }
            c.call("prof.targets", json!({ "last": last }))
        }
        "marks" => {
            let action = args.get("action").and_then(Json::as_str).unwrap_or("list");
            match action {
                "list" => c.call("mark.list", pick(args, &["all"])),
                "get" | "open" | "note" | "close" => {
                    c.call(&format!("mark.{action}"), pick(args, &["id", "note"]))
                }
                "import" => c.call("mark.import", pick(args, &["path"])),
                "pull" => c.call("mark.pull", pick(args, &["url"])),
                other => Err(bad(format!(
                    "no marks action {other:?}: list, get, open, note, close, import, or pull"
                ))),
            }
        }
        _ => Err(bad(format!("tool {name} has no implementation"))),
    }
}

/// Say on an error that the transaction kept nothing.
fn nothing_kept(mut e: Json) -> Json {
    if let Some(o) = e.as_object_mut() {
        o.insert("nothing_kept".into(), json!(true));
    }
    e
}

fn author(args: &Json) -> String {
    args.get("author")
        .and_then(Json::as_str)
        .unwrap_or("agent:claude")
        .to_string()
}

/// Open a transaction and apply `ops` in it; on any failure it is aborted
/// and the error says which op. The transaction's id.
fn transaction(c: &mut dyn Caller, args: &Json, tool: &str) -> Result<Json, Json> {
    let ops = args.get("ops").and_then(Json::as_array).ok_or_else(|| {
        bad(format!(
            "{tool} needs ops: [{{op: \"set\", entity, path, value}}, ...]"
        ))
    })?;
    let note = args.get("note").and_then(Json::as_str).unwrap_or("");
    let begun = c.call(
        "edit.begin",
        json!({ "author": author(args), "note": note }),
    )?;
    let tx = begun.get("tx").cloned().unwrap_or(Json::Null);
    for (i, op) in ops.iter().enumerate() {
        let kind = op.get("op").and_then(Json::as_str).unwrap_or("");
        // def and asset too: a tuning session's commit (item 3) and an
        // animator's edit (item 8) are lines of this tool in the feed.
        if ![
            "set", "add", "remove", "spawn", "despawn", "tiles", "text", "def", "asset",
        ]
        .contains(&kind)
        {
            let _ = c.call("edit.abort", json!({ "tx": tx }));
            return Err(bad(format!(
                "op {i} has op {kind:?}: set, add, remove, spawn, despawn, tiles, text, def, or asset"
            )));
        }
        let mut p = op.as_object().cloned().unwrap_or_default();
        p.remove("op");
        p.insert("tx".into(), tx.clone());
        if let Err(mut e) = c.call(&format!("edit.{kind}"), Json::Object(p)) {
            let _ = c.call("edit.abort", json!({ "tx": tx }));
            if let Some(o) = e.as_object_mut() {
                o.insert("op".into(), json!(i));
            }
            return Err(nothing_kept(e));
        }
    }
    Ok(tx)
}
