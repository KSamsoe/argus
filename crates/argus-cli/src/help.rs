//! Help text, generated from `METHODS` (§16: one schema generates the
//! protocol, the CLI help, and the tool descriptions).

use crate::params;
use crate::Command;
use argus_daemon::{docs, MethodDoc, METHODS};
use std::fmt::Write;

/// Program usage plus every namespace and method, one line each.
pub fn overview() -> String {
    let mut out = String::new();
    out.push_str(
        "usage: argus <namespace> <method> [value] [key=value ...] [--json] [--follow]\n       argus <namespace>              list a namespace's methods\n       argus <namespace> <method> -h  one method's doc, params, returns\n       argus daemon <start|stop|status|run> [--scene s] [--seed n]\n       argus help [namespace | method]\n\nflags: --project <dir> (default: the nearest argus.toml upward from here, or $ARGUS_PROJECT)\n       --socket <name>   connect to this socket instead of the project's daemon\n       --no-start        fail if no daemon is running instead of starting one\n       --params <json>   a whole params object (or @file.json); key=value words layer on top\n       --json            print the result (or error) as one JSON line\n       --follow          keep the connection open and print notifications (obs.subscribe does this)\n\nvalues: JSON when they parse as JSON (2, true, \"text\", [1,2]); bare words are strings;\n        [a, b] and {k: v} accept bare words; dotted keys nest (where.has=[Enemy]);\n        a repeated key becomes a list; one bare value goes to the method's first parameter.\n\nmethods (each is `argus <ns> <name>`, or `argus <ns>.<name>`):\n",
    );
    let mut ns_seen = String::new();
    for m in METHODS {
        let ns = m.name.split('.').next().unwrap_or("");
        if ns != ns_seen {
            let _ = writeln!(out, "\n  {ns}");
            ns_seen = ns.to_string();
        }
        let _ = writeln!(out, "    {:<22} {}", m.name, first_sentence(m.doc));
    }
    out.push_str("\n  daemon\n    daemon start           start the project's daemon in the background (a method does this too when needed)\n    daemon stop            session.shutdown on the running daemon\n    daemon status          is a daemon running for this project, and which\n    daemon run             run the daemon in the foreground (what argusd --project does)\n");
    out
}

/// `argus help <topic>`: nothing, a namespace, or a method.
pub fn topic(words: &[String]) -> String {
    if words.is_empty() {
        return overview();
    }
    let joined = words.join(".");
    if let Some(m) = docs::find(&joined) {
        return method(m);
    }
    if joined == "daemon" {
        return daemon();
    }
    if METHODS
        .iter()
        .any(|m| m.name.split('.').next() == Some(joined.as_str()))
    {
        return namespace(&joined);
    }
    format!("argus: no help for {joined:?}; `argus help` lists everything\n")
}

/// Every method of a namespace with its full doc.
pub fn namespace(ns: &str) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "argus {ns}");
    for m in METHODS
        .iter()
        .filter(|m| m.name.split('.').next() == Some(ns))
    {
        let _ = writeln!(out, "\n  {}", usage_line(m.name).trim_end());
        let _ = writeln!(out, "    {}", m.doc);
    }
    out
}

/// Help for one method: usage, doc, params, returns, as the daemon
/// documents it.
pub fn method(m: &MethodDoc) -> String {
    let mut out = String::new();
    out.push_str(&usage_line(m.name));
    let _ = writeln!(out, "\n{}", m.doc);
    if !m.params.is_empty() {
        let _ = writeln!(out, "\nparams:  {}", m.params);
        let names = params::names(m.name);
        if let Some(first) = names.first() {
            let _ = writeln!(out, "         one bare value is {first}=");
        }
    }
    let _ = writeln!(out, "returns: {}", m.returns);
    let _ = writeln!(out, "\nsame as: session.docs {{method: \"{}\"}}", m.name);
    out
}

/// `usage: argus world get [entity] [key=value ...]`.
pub fn usage_line(method: &str) -> String {
    let words = method.replace('.', " ");
    let names = params::names(method);
    let mut out = format!("usage: argus {words}");
    if let Some(first) = names.first() {
        let _ = write!(out, " [{first}]");
    }
    if !names.is_empty() {
        out.push_str(" [key=value ...]");
    }
    out.push('\n');
    out
}

/// Help for `argus daemon`.
pub fn daemon() -> String {
    "usage: argus daemon start [--scene s] [--seed n]   start the project's daemon in the background\n       argus daemon stop                          ask it to exit (session.shutdown)\n       argus daemon status                        running or not, pid, socket\n       argus daemon run [--scene s] [--seed n]     serve in the foreground until stopped\n\nThe daemon is found through <project>/.argus/daemon.json. A method call starts one when none answers,\nunless --no-start is given.\n".to_string()
}

/// Help for whatever the words resolved to.
pub fn for_command(cmd: &Command) -> String {
    match cmd {
        Command::Method(name, _) => method(docs::find(name).expect("resolved")),
        Command::Namespace(ns) => namespace(ns),
        Command::Daemon(_) => daemon(),
        Command::Help(t) => topic(t),
        Command::Unknown(_) => overview(),
    }
}

fn first_sentence(doc: &str) -> &str {
    match doc.find(". ") {
        Some(i) => &doc[..=i],
        None => doc,
    }
}
