//! The scenario test runner (§9): `tests/*.luau` files that load a scene,
//! fix a seed, script some input, run some frames, and assert on state and
//! events. Everything a scenario does goes through the protocol, so the
//! runner is a client like the editor and the CLI (§2), and a scenario is
//! reproducible by construction (§6): same scene, seed, and input log.
//!
//! The harness (`t`, `t.input`, `t.expect`, matchers, goldens, seed sweeps)
//! is written in Luau, in `harness.luau`, over five Rust functions: call
//! a method, read a golden, write a golden, read the clock, and slugify.
//! Rust owns discovery, the seed sweep options, and the report shape.

#![warn(missing_docs)]

pub mod api;

use mlua::{DeserializeOptions, Function, Lua, LuaSerdeExt, SerializeOptions, Table, Value};
use serde_json::{json, Value as Json};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// The harness prelude.
pub const HARNESS: &str = include_str!("harness.luau");

/// What a scenario talks to: something that answers protocol methods.
/// The daemon implements it in-process; a remote runner could implement
/// it over a socket.
pub trait Driver {
    /// Answer one method. The error is the JSON-RPC error object
    /// (`{code, message, data: {hint, ref}}`).
    fn call(&mut self, method: &str, params: &Json) -> Result<Json, Json>;
    /// The project root, where `tests/` and `tests/goldens/` live.
    fn root(&self) -> PathBuf;
}

/// `test.run` options.
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// Run only the scenario with this name (or whose name contains it).
    pub name: Option<String>,
    /// Run only scenarios from this file (project-relative).
    pub file: Option<String>,
    /// Sim seconds in the project's day (`[time] day`), which `days()`
    /// and `hours()` count in; `None`: 86,400.
    pub day_seconds: Option<f64>,
    /// Seed sweep override: `"7"`, `"1..50"`, or `"1,2,9"`.
    pub seeds: Option<String>,
    /// Rewrite goldens instead of comparing.
    pub update_goldens: bool,
    /// Save a replay of each failing run where it failed, through the
    /// driver's `replay.save` (§9, phase 2 item 3): under
    /// `.argus/replays/<scenario>/seed<n>/`, with a clip for the first five.
    pub replays: bool,
    /// Draw those clips with the software rasteriser, as CI does (§9).
    pub software: bool,
    /// Fields put in each scenario's world after its scene loads, live only
    /// (`sim.set`'s `{entity | def, path, value}`): a sweep's candidate, tried
    /// without writing it (phase 4 item 3).
    pub set: Vec<serde_json::Value>,
}

/// Parse a seeds spec into a list.
pub fn parse_seeds(spec: &str) -> Result<Vec<u64>, String> {
    let spec = spec.trim();
    if let Some((a, b)) = spec.split_once("..") {
        let a: u64 = a
            .trim()
            .parse()
            .map_err(|_| format!("bad seed range {spec:?}"))?;
        let b: u64 = b
            .trim()
            .parse()
            .map_err(|_| format!("bad seed range {spec:?}"))?;
        if b < a || b - a > 100_000 {
            return Err(format!(
                "seed range {spec:?} must go up and hold at most 100000 seeds"
            ));
        }
        return Ok((a..=b).collect());
    }
    spec.split(',')
        .map(|s| {
            s.trim()
                .parse::<u64>()
                .map_err(|_| format!("bad seed {s:?} in {spec:?}"))
        })
        .collect()
}

/// One scenario file found under `tests/`.
fn scenario_files(root: &Path) -> Vec<(String, PathBuf)> {
    let dir = root.join("tests");
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "luau") && p.is_file() {
                let rel = format!("tests/{}", p.file_name().unwrap().to_string_lossy());
                out.push((rel, p));
            }
        }
    }
    out.sort();
    out
}

/// `--file`'s scenario file as the runner names it (`tests/x.luau`), from
/// that, `x.luau`, `./tests/x.luau`, or a path under the project; an error
/// naming the files there are when it is not one of them.
fn scenario_file_named(root: &Path, given: &str) -> Result<String, String> {
    let files = scenario_files(root);
    let slashes = |s: &str| s.replace('\\', "/");
    let clean = slashes(given);
    let clean = clean.trim_start_matches("./");
    let root_s = slashes(&root.to_string_lossy());
    let root_s = root_s.trim_start_matches("//?/");
    let rel = match clean.strip_prefix(root_s) {
        Some(r) => r.trim_start_matches('/').to_string(),
        None => clean.to_string(),
    };
    let rel = if rel.contains('/') {
        rel
    } else {
        format!("tests/{rel}")
    };
    if files.iter().any(|(f, _)| *f == rel) {
        return Ok(rel);
    }
    let there: Vec<&str> = files.iter().map(|(f, _)| f.as_str()).collect();
    Err(format!(
        "--file {given}: the runner reads only .luau files directly in tests/, and {} {}; the files it reads are {}",
        rel,
        if root.join(&rel).is_file() { "is not one" } else { "does not exist" },
        if there.is_empty() { "none".to_string() } else { there.join(", ") },
    ))
}

fn to_lua(lua: &Lua, v: &Json) -> mlua::Result<Value> {
    lua.to_value_with(
        v,
        SerializeOptions::new()
            .serialize_none_to_null(false)
            .serialize_unit_to_null(false)
            .set_array_metatable(false),
    )
}

fn from_lua(lua: &Lua, v: Value) -> mlua::Result<Json> {
    lua.from_value_with(
        v,
        DeserializeOptions::new()
            .deny_unsupported_types(false)
            .deny_recursive_tables(true),
    )
}

/// Lua tables with no entries read back as `[]`; a params object must be
/// `{}`, and so must any empty table nested where the daemon expects an
/// object (`where`, `components`).
fn arrays_to_objects_where_empty(v: Json) -> Json {
    match v {
        Json::Array(a) if a.is_empty() => Json::Object(Default::default()),
        Json::Array(a) => Json::Array(a.into_iter().map(arrays_to_objects_where_empty).collect()),
        Json::Object(o) => Json::Object(
            o.into_iter()
                .map(|(k, v)| (k, arrays_to_objects_where_empty(v)))
                .collect(),
        ),
        other => other,
    }
}

/// Build the VM with the harness loaded and every scenario file executed
/// (registering its scenarios). Load errors are collected, not fatal.
fn prepare(root: &Path, day_seconds: Option<f64>) -> Result<(Lua, Table, Vec<Json>), String> {
    let lua = Lua::new();
    if let Some(d) = day_seconds {
        lua.globals()
            .set("__day_seconds", d)
            .map_err(|e| e.to_string())?;
    }
    let harness_fns = lua.create_table().map_err(|e| e.to_string())?;
    // The system a picture golden is drawn on, each keeping its own.
    harness_fns
        .set("system", std::env::consts::OS)
        .map_err(|e| e.to_string())?;
    let goldens = root.join("tests").join("goldens");
    let g1 = goldens.clone();
    harness_fns
        .set(
            "golden_read",
            lua.create_function(move |_, rel: String| {
                Ok(std::fs::read_to_string(g1.join(&rel)).ok())
            })
            .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
    // A seed that passes drops the replay an earlier failure of it saved,
    // so `replay.list` shows only what still fails (found at the phase 5
    // gate). `rel` is the seed's own folder under `.argus/replays/`.
    let replays = root.join(".argus").join("replays");
    harness_fns
        .set(
            "replay_clear",
            lua.create_function(move |_, rel: String| {
                let rel = std::path::Path::new(&rel);
                let inside = rel
                    .components()
                    .all(|c| matches!(c, std::path::Component::Normal(_)));
                let dir = replays.join(rel);
                if inside && dir.is_dir() {
                    std::fs::remove_dir_all(&dir).map_err(mlua::Error::external)?;
                    return Ok(true);
                }
                Ok(false)
            })
            .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
    let g3 = goldens.clone();
    harness_fns
        .set(
            "golden_exists",
            lua.create_function(move |_, rel: String| Ok(g3.join(&rel).is_file()))
                .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
    let g2 = goldens.clone();
    harness_fns
        .set(
            "golden_write",
            lua.create_function(move |_, (rel, text): (String, String)| {
                let p = g2.join(&rel);
                if let Some(d) = p.parent() {
                    std::fs::create_dir_all(d).map_err(mlua::Error::external)?;
                }
                std::fs::write(&p, text).map_err(mlua::Error::external)?;
                Ok(p.to_string_lossy().into_owned())
            })
            .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
    let t0 = Instant::now();
    harness_fns
        .set(
            "now_ms",
            lua.create_function(move |_, ()| Ok(t0.elapsed().as_secs_f64() * 1e3))
                .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
    harness_fns
        .set(
            "json_encode",
            lua.create_function(|lua, v: Value| {
                let j = from_lua(lua, v)?;
                serde_json::to_string_pretty(&j).map_err(mlua::Error::external)
            })
            .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
    harness_fns
        .set(
            "json_decode",
            lua.create_function(|lua, s: String| {
                let j: Json = serde_json::from_str(&s).map_err(mlua::Error::external)?;
                to_lua(lua, &j)
            })
            .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
    lua.globals()
        .set("__harness", harness_fns)
        .map_err(|e| e.to_string())?;
    let harness: Table = lua
        .load(HARNESS)
        .set_name("=harness.luau")
        .eval()
        .map_err(|e| format!("harness.luau: {e}"))?;

    let mut load_errors = Vec::new();
    for (rel, path) in scenario_files(root) {
        let src = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                load_errors.push(json!({ "file": rel, "message": e.to_string() }));
                continue;
            }
        };
        lua.globals()
            .set("__current_file", rel.clone())
            .map_err(|e| e.to_string())?;
        if let Err(e) = lua.load(&src).set_name(format!("={rel}")).exec() {
            load_errors.push(json!({ "file": rel, "message": e.to_string() }));
        }
    }
    Ok((lua, harness, load_errors))
}

/// `test.list`: the scenarios under `tests/` with name, file, scene, and
/// seeds, without running anything. `day_seconds` as in [`Options`].
pub fn list(root: &Path, day_seconds: Option<f64>) -> Result<Json, String> {
    let (lua, harness, load_errors) = prepare(root, day_seconds)?;
    let f: Function = harness.get("list").map_err(|e| e.to_string())?;
    let v: Value = f.call(()).map_err(|e| e.to_string())?;
    let rows = from_lua(&lua, v).map_err(|e| e.to_string())?;
    let rows = match rows {
        Json::Array(a) => a,
        _ => Vec::new(),
    };
    Ok(json!({ "total": rows.len(), "rows": rows, "load_errors": load_errors }))
}

/// `test.run`: run the selected scenarios over their seeds and report.
pub fn run(driver: &mut dyn Driver, opts: &Options) -> Result<Json, String> {
    let root = driver.root();
    // A file the runner does not read is an error, not a pass of nothing:
    // a CI step that ran one believed its scenario passed (issue 29).
    let file = opts
        .file
        .as_deref()
        .filter(|f| !f.is_empty())
        .map(|f| scenario_file_named(&root, f))
        .transpose()?;
    let (lua, harness, load_errors) = prepare(&root, opts.day_seconds)?;
    let seeds = match &opts.seeds {
        Some(s) => Some(parse_seeds(s)?),
        None => None,
    };
    let lua_opts = to_lua(
        &lua,
        &json!({
            "name": opts.name,
            "file": file,
            "seeds": seeds,
            "replays": opts.replays,
            "software": opts.software,
            "update": opts.update_goldens,
            "set": opts.set,
        }),
    )
    .map_err(|e| e.to_string())?;
    let run_fn: Function = harness.get("run").map_err(|e| e.to_string())?;
    let fns: Table = lua.globals().get("__harness").map_err(|e| e.to_string())?;
    let started = Instant::now();
    let report: Json = lua
        .scope(|scope| {
            let call = scope.create_function_mut(|lua, (method, params): (String, Value)| {
                let params = arrays_to_objects_where_empty(from_lua(lua, params)?);
                match driver.call(&method, &params) {
                    Ok(v) => Ok((to_lua(lua, &v)?, Value::Nil)),
                    Err(e) => Ok((Value::Nil, to_lua(lua, &e)?)),
                }
            })?;
            fns.set("call", call)?;
            let v: Value = run_fn.call(lua_opts)?;
            from_lua(&lua, v)
        })
        .map_err(|e| e.to_string())?;
    let mut report = report;
    // A filter that selected nothing ran nothing: an error, not a pass.
    let named = opts.name.as_deref().filter(|n| !n.is_empty());
    if report["total_runs"].as_u64() == Some(0) && (named.is_some() || file.is_some()) {
        let what = match (named, &file) {
            (Some(n), Some(f)) => format!("no scenario in {f} has {n:?} in its name"),
            (Some(n), None) => format!("no scenario has {n:?} in its name"),
            (None, Some(f)) => format!("{f} has no scenarios"),
            (None, None) => unreachable!(),
        };
        return Err(format!(
            "{what}, so nothing ran; argus test list lists them"
        ));
    }
    if let Some(o) = report.as_object_mut() {
        o.insert(
            "ms".to_string(),
            json!(started.elapsed().as_secs_f64() * 1e3),
        );
        o.insert("load_errors".to_string(), Json::Array(load_errors.clone()));
        if !load_errors.is_empty() {
            o.insert("ok".to_string(), json!(false));
        }
    }
    Ok(report)
}
