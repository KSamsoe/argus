//! `argus`: the Argus daemon's protocol as a shell command (§5 "mirrored
//! one-to-one by CLI subcommands", §16 "one schema generates the
//! protocol, the CLI help, the MCP tool descriptions").
//!
//! There is no command table in this crate. Every subcommand is a row of
//! [`argus_daemon::METHODS`]: `argus world query` is `world.query`,
//! `argus input log get` is `input.log.get`, and the help for each is the
//! same doc, params, and returns text the daemon serves as
//! `session.docs`. Beside the methods: the agent tools (§16, phase 2 item
//! 8), `argus summary`, `argus query`, `argus edit` and the rest, which are
//! what `argus mcp` serves, so an agent's tool call reruns as a command
//! line; `help`; `daemon` (start, stop, status, run), which exists so a
//! method has a process to reach; `build` (phase 2 item 7), which makes a
//! release; `mcp`, the MCP server; and `docs --for-agent`, which writes
//! ARGUS.md from the live daemon.
//!
//! Parameters are `key=value` words (values parsed as JSON when they are
//! JSON, strings otherwise, with `[a, b]` and `{k: v}` accepted with bare
//! words), dotted keys for nesting, `--params '{...}'` for a whole object,
//! and one bare word for the method's first parameter, so `argus world
//! get @player` reads as it should.

#![warn(missing_docs)]

pub mod agentdoc;
pub mod build;
pub mod client;
pub mod help;
pub mod mcp;
pub mod params;
pub mod project;
pub mod render;
pub mod tools;

use argus_daemon::METHODS;
use serde_json::{json, Value as Json};
use std::io::Write;
use std::path::PathBuf;

/// Exit code: success.
pub const EXIT_OK: i32 = 0;
/// Exit code: the daemon answered with an error.
pub const EXIT_RPC: i32 = 1;
/// Exit code: the command line could not be understood.
pub const EXIT_USAGE: i32 = 2;
/// Exit code: no daemon could be reached.
pub const EXIT_CONNECT: i32 = 3;

/// A parsed command line.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Invocation {
    /// Words before the first `key=value` or flag: the command path.
    pub words: Vec<String>,
    /// `key=value` words and bare values after the command.
    pub args: Vec<String>,
    /// `--params` JSON objects, merged in order under the key=value words.
    pub params_json: Vec<String>,
    /// `--json`: print the raw result (or error) as one JSON line.
    pub json: bool,
    /// `--follow`: keep the connection and print notifications.
    pub follow: bool,
    /// `--help`.
    pub help: bool,
    /// `--project <dir>`.
    pub project: Option<PathBuf>,
    /// `--socket <name>`: connect here instead of discovering.
    pub socket: Option<String>,
    /// `--no-start`: fail instead of starting a daemon.
    pub no_start: bool,
    /// `--scene`, `--seed`: for `daemon start` and `daemon run`.
    pub scene: Option<String>,
    /// See `scene`.
    pub seed: Option<u64>,
    /// `--seeds <spec>`: the seed sweep for `argus test`.
    pub seeds: Option<String>,
    /// `--file <tests/x.luau>`: only that file's scenarios, for `argus test`.
    pub file: Option<String>,
    /// `--update`: rewrite goldens, for `argus test`.
    pub update: bool,
    /// `--software`: clips of failing tests drawn with the software rasteriser.
    pub software: bool,
    /// `--no-replays`: failing tests save no replays.
    pub no_replays: bool,
    /// `--window [WxH]`: open a window, for `daemon start` and `daemon run`.
    pub window: Option<(u32, u32)>,
    /// `--play`: enter play mode at start, for `daemon start` and `daemon run`.
    pub play: bool,
    /// `--target <windows|linux|macos|web>`, for `argus build`.
    pub target: Option<String>,
    /// `--release`: an optimised build, for `argus build`.
    pub release: bool,
    /// `--out <dir>`: where `argus build` writes.
    pub out: Option<PathBuf>,
    /// `--since <dir>`: the build `argus build` patches.
    pub since: Option<PathBuf>,
    /// `--mod <id>`: `argus build` makes that mod's pack (phase 3 item 8).
    pub mod_id: Option<String>,
    /// `--mods a,b`: the mods `daemon start` and `daemon run` open with.
    pub mods: Vec<String>,
    /// `--web [addr]`: `daemon start` and `daemon run` serve the web client
    /// too (phase 5 item 10): `local` (the default), `tailnet`, or an
    /// address of either.
    pub web: Option<String>,
    /// `--for-agent`: `argus docs` writes the project's ARGUS.md.
    pub for_agent: bool,
}

/// Parse a command line (without the program name).
pub fn parse(argv: &[String]) -> Result<Invocation, String> {
    let mut inv = Invocation::default();
    let mut it = argv.iter();
    let mut in_args = false;
    while let Some(a) = it.next() {
        let need = |it: &mut std::slice::Iter<String>| -> Result<String, String> {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{a} needs a value"))
        };
        match a.as_str() {
            "--json" | "-j" => inv.json = true,
            "--follow" | "-f" => inv.follow = true,
            "--help" | "-h" => inv.help = true,
            "--no-start" => inv.no_start = true,
            "--project" | "-P" => inv.project = Some(PathBuf::from(need(&mut it)?)),
            "--socket" => inv.socket = Some(need(&mut it)?),
            "--params" | "-p" => inv.params_json.push(need(&mut it)?),
            "--scene" => inv.scene = Some(need(&mut it)?),
            "--seeds" => inv.seeds = Some(need(&mut it)?),
            "--file" => inv.file = Some(need(&mut it)?),
            "--update" => inv.update = true,
            "--software" => inv.software = true,
            "--no-replays" => inv.no_replays = true,
            "--play" => inv.play = true,
            "--release" => inv.release = true,
            "--for-agent" => inv.for_agent = true,
            "--target" => inv.target = Some(need(&mut it)?),
            "--out" => inv.out = Some(PathBuf::from(need(&mut it)?)),
            "--since" => inv.since = Some(PathBuf::from(need(&mut it)?)),
            "--mod" => inv.mod_id = Some(need(&mut it)?),
            "--web" => {
                inv.web = Some(match it.clone().next() {
                    Some(s) if !s.starts_with('-') => {
                        it.next();
                        s.clone()
                    }
                    _ => "local".to_string(),
                });
            }
            "--mods" => {
                inv.mods = need(&mut it)?
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            }
            "--window" => {
                let size = match it.clone().next() {
                    Some(s) if s.contains('x') && !s.starts_with('-') => {
                        it.next();
                        s.split_once('x')
                            .map(|(w, h)| (w.parse().unwrap_or(1280), h.parse().unwrap_or(720)))
                            .unwrap()
                    }
                    _ => (1280, 720),
                };
                inv.window = Some(size);
            }
            "--seed" => {
                let v = need(&mut it)?;
                inv.seed = Some(
                    v.parse()
                        .map_err(|_| format!("--seed needs an integer; got {v:?}"))?,
                );
            }
            s if s.starts_with("--") && s.len() > 2 && !in_args => {
                return Err(format!("unknown flag {s}; see `argus help`"));
            }
            s => {
                if in_args || s.contains('=') || !is_word(s) {
                    in_args = true;
                    inv.args.push(s.to_string());
                } else {
                    inv.words.push(s.to_string());
                }
            }
        }
    }
    Ok(inv)
}

/// A command word: letters, digits, dots, dashes, underscores; not a value.
fn is_word(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
        && !s.starts_with('@')
        && !s.starts_with('-')
        && !s.chars().next().unwrap().is_ascii_digit()
}

/// What the command words name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// A daemon method, plus the words left over (bare positionals).
    Method(&'static str, Vec<String>),
    /// A namespace alone (`argus world`): list its methods.
    Namespace(String),
    /// `argus help [topic]`.
    Help(Vec<String>),
    /// `argus daemon <start|stop|status|run>`.
    Daemon(Vec<String>),
    /// `argus editor`: the editor window on the project's daemon (§11).
    Editor,
    /// `argus test [name]`: `test.run` with a compact report (§9).
    Test(Vec<String>),
    /// `argus build`: a release of the project (§15).
    Build,
    /// An agent tool (`argus query ...`), plus bare words (§16).
    Tool(&'static str, Vec<String>),
    /// `argus mcp`: the MCP server on stdio (§16).
    Mcp,
    /// `argus docs [--for-agent]`: the reference, or the project's ARGUS.md.
    Docs,
    /// Nothing recognisable.
    Unknown(Vec<String>),
}

/// Map the command words to a method: the longest run of leading words
/// whose dotted join is a method name. `world query`, `world.query`, and
/// `input log get` all resolve.
pub fn resolve(words: &[String]) -> Command {
    if words.is_empty() {
        return Command::Help(Vec::new());
    }
    match words[0].as_str() {
        "help" => return Command::Help(words[1..].to_vec()),
        "daemon" => return Command::Daemon(words[1..].to_vec()),
        "editor" if words.len() == 1 => return Command::Editor,
        "build" => return Command::Build,
        "mcp" => return Command::Mcp,
        "docs" => return Command::Docs,
        "test" if words.len() == 1 || !METHODS.iter().any(|m| m.name == words[..2].join(".")) => {
            return Command::Test(words[1..].to_vec())
        }
        _ => {}
    }
    let mut best: Option<(&'static str, usize)> = None;
    for k in 1..=words.len() {
        let joined = words[..k].join(".");
        if let Some(m) = METHODS.iter().find(|m| m.name == joined) {
            best = Some((m.name, k));
        }
    }
    if let Some((name, k)) = best {
        return Command::Method(name, words[k..].to_vec());
    }
    if let Some(t) = argus_daemon::docs::tool(&words[0]) {
        return Command::Tool(t.name, words[1..].to_vec());
    }
    let ns = words[0].split('.').next().unwrap_or("").to_string();
    if words.len() == 1
        && METHODS
            .iter()
            .any(|m| m.name.split('.').next() == Some(&ns))
    {
        return Command::Namespace(ns);
    }
    Command::Unknown(words.to_vec())
}

/// Run a command line: parse, resolve, connect, call, print. Returns the
/// exit code. `out` gets results, `err` gets errors and daemon chatter.
pub fn run(argv: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let inv = match parse(argv) {
        Ok(i) => i,
        Err(e) => {
            let _ = writeln!(err, "argus: {e}");
            return EXIT_USAGE;
        }
    };
    let cmd = resolve(&inv.words);
    if inv.help {
        let _ = write!(out, "{}", help::for_command(&cmd));
        return EXIT_OK;
    }
    match cmd {
        Command::Help(topic) => {
            let _ = write!(out, "{}", help::topic(&topic));
            EXIT_OK
        }
        Command::Namespace(ns) => {
            let _ = write!(out, "{}", help::namespace(&ns));
            EXIT_OK
        }
        Command::Unknown(words) => {
            let _ = writeln!(
                err,
                "argus: no command {:?}; `argus help` lists them",
                words.join(" ")
            );
            EXIT_USAGE
        }
        Command::Daemon(words) => project::daemon_command(&inv, &words, out, err),
        Command::Editor => project::editor_command(&inv, out, err),
        Command::Test(words) => test_command(&inv, &words, out, err),
        Command::Build => build::build_command(&inv, out, err),
        Command::Tool(name, bare) => tool_command(&inv, name, bare, out, err),
        Command::Mcp => mcp::serve(&inv, err),
        Command::Docs => agentdoc::command(&inv, out, err),
        Command::Method(method, bare) => {
            let mut args = bare;
            args.extend(inv.args.iter().cloned());
            let params = match params::build(method, &args, &inv.params_json) {
                Ok(p) => p,
                Err(e) => {
                    let _ = writeln!(err, "argus: {e}");
                    let _ = write!(err, "{}", help::usage_line(method));
                    return EXIT_USAGE;
                }
            };
            let mut client = match project::connect(&inv, err) {
                Ok(c) => c,
                Err(e) => {
                    let _ = writeln!(err, "argus: {e}");
                    return EXIT_CONNECT;
                }
            };
            let follow = inv.follow || method == "obs.subscribe";
            let code = call_and_print(&mut client, method, params, inv.json, out, err);
            if code == EXIT_OK && follow {
                while let Ok(Some(n)) = client.next_notification() {
                    let _ = writeln!(out, "{}", render::notification(&n, inv.json));
                    let _ = out.flush();
                }
            }
            code
        }
    }
}

/// `argus <tool> [value] [key=value ...]`: an agent tool, as `argus mcp`
/// runs it. A tool that needs something (edit, propose, seek) run bare
/// shows its help.
fn tool_command(
    inv: &Invocation,
    name: &'static str,
    bare: Vec<String>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let mut args = bare;
    args.extend(inv.args.iter().cloned());
    if args.is_empty() && inv.params_json.is_empty() && ["edit", "propose", "seek"].contains(&name)
    {
        let _ = write!(out, "{}", help::tool_topic(name));
        return EXIT_OK;
    }
    let params = match params::build_tool(name, &args, &inv.params_json) {
        Ok(p) => p,
        Err(e) => {
            let _ = writeln!(err, "argus: {e}");
            return EXIT_USAGE;
        }
    };
    let mut client = match project::connect(inv, err) {
        Ok(c) => c,
        Err(e) => {
            let _ = writeln!(err, "argus: {e}");
            return EXIT_CONNECT;
        }
    };
    match tools::run(&mut client, name, &params) {
        Ok(result) => {
            if inv.json {
                let _ = writeln!(out, "{result}");
            } else {
                let _ = write!(out, "{}", render::result(&result));
            }
            EXIT_OK
        }
        Err(e) => {
            if inv.json {
                let _ = writeln!(out, "{}", json!({ "error": e }));
            } else {
                let _ = write!(err, "{}", render::error(&e));
            }
            EXIT_RPC
        }
    }
}

/// `argus test [name] [--seeds 1..50] [--file tests/x.luau] [--update] [--software] [--no-replays] [--json]`:
/// `test.run` on the project's daemon, reported compactly (§9). Exit 0
/// when every run passed, 1 otherwise.
fn test_command(
    inv: &Invocation,
    words: &[String],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let mut name: Vec<String> = words.to_vec();
    name.extend(inv.args.iter().filter(|a| !a.contains('=')).cloned());
    let mut params = json!({});
    if !name.is_empty() {
        params["name"] = json!(name.join(" "));
    }
    if let Some(s) = &inv.seeds {
        params["seeds"] = json!(s);
    }
    if let Some(f) = &inv.file {
        params["file"] = json!(f);
    }
    if inv.update {
        params["update"] = json!(true);
    }
    if inv.software {
        params["software"] = json!(true);
    }
    if inv.no_replays {
        params["replays"] = json!(false);
    }
    // The test tool's key=value words too (`argus test name=speed`); they
    // were dropped, and a filter that is dropped runs everything (found in
    // phase 3 item 3, when a name ran six minutes of seeds).
    for (k, v) in inv.args.iter().filter_map(|a| a.split_once('=')) {
        match k {
            "name" | "file" | "seeds" => params[k] = json!(v),
            "update" | "software" => params[k] = json!(v != "false"),
            "replays" => params[k] = json!(v != "false"),
            // Candidates tried without writing them (phase 4 item 3).
            "set" | "sweep" => match params::value(v) {
                Ok(x) => params[k] = x,
                Err(e) => {
                    let _ = writeln!(err, "argus: {k}: {e}");
                    return EXIT_USAGE;
                }
            },
            other => {
                let _ = writeln!(
                    err,
                    "argus: test takes name, file, seeds, update, software, replays, set, and sweep; not {other:?}"
                );
                return EXIT_USAGE;
            }
        }
    }
    let mut client = match project::connect(inv, err) {
        Ok(c) => c,
        Err(e) => {
            let _ = writeln!(err, "argus: {e}");
            return EXIT_CONNECT;
        }
    };
    let resp = match client.call("test.run", params) {
        Ok(r) => r,
        Err(e) => {
            let _ = writeln!(err, "argus: connection lost: {e}");
            return EXIT_CONNECT;
        }
    };
    if let Some(e) = resp.get("error") {
        if inv.json {
            let _ = writeln!(out, "{}", json!({ "error": e }));
        } else {
            let _ = write!(err, "{}", render::error(e));
        }
        return EXIT_RPC;
    }
    let report = resp.get("result").cloned().unwrap_or(Json::Null);
    if inv.json {
        let _ = writeln!(out, "{report}");
    } else {
        let _ = write!(out, "{}", render::test_report(&report));
    }
    if report["ok"] == json!(true) {
        EXIT_OK
    } else {
        EXIT_RPC
    }
}

/// One call, printed. The result or the error, as text or as JSON.
pub fn call_and_print(
    client: &mut client::Client,
    method: &str,
    params: Json,
    as_json: bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let select = params.get("select").cloned();
    let resp = match client.call(method, params) {
        Ok(r) => r,
        Err(e) => {
            let _ = writeln!(err, "argus: connection lost: {e}");
            return EXIT_CONNECT;
        }
    };
    if let Some(e) = resp.get("error") {
        if as_json {
            let _ = writeln!(out, "{}", json!({ "error": e }));
        } else {
            let _ = write!(err, "{}", render::error(e));
        }
        return EXIT_RPC;
    }
    let mut result = resp.get("result").cloned().unwrap_or(Json::Null);
    if as_json {
        let _ = writeln!(out, "{result}");
    } else {
        // world.query rows are shaped by the select list the caller sent;
        // it is the table header the human wants.
        if let (Some(sel), Some(obj)) = (select, result.as_object_mut()) {
            if obj.contains_key("rows") && !obj.contains_key("columns") && sel.is_array() {
                obj.insert("columns".to_string(), sel);
            }
        }
        let _ = write!(out, "{}", render::result(&result));
    }
    EXIT_OK
}
