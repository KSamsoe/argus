//! `argus`: the Argus daemon's protocol as a shell command (§5 "mirrored
//! one-to-one by CLI subcommands", §16 "one schema generates the
//! protocol, the CLI help, the MCP tool descriptions").
//!
//! There is no command table in this crate. Every subcommand is a row of
//! [`argus_daemon::METHODS`]: `argus world query` is `world.query`,
//! `argus input log get` is `input.log.get`, and the help for each is the
//! same doc, params, and returns text the daemon serves as
//! `session.docs`. The only commands that are not methods are `help` and
//! `daemon` (start, stop, status, run), which exist so a method has a
//! process to reach.
//!
//! Parameters are `key=value` words (values parsed as JSON when they are
//! JSON, strings otherwise, with `[a, b]` and `{k: v}` accepted with bare
//! words), dotted keys for nesting, `--params '{...}'` for a whole object,
//! and one bare word for the method's first parameter, so `argus world
//! get @player` reads as it should.

#![warn(missing_docs)]

pub mod client;
pub mod help;
pub mod params;
pub mod project;
pub mod render;

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
