//! Help text, generated from the one schema (`argus_daemon::docs`: §16,
//! one schema generates the protocol, the CLI help, and the MCP tool
//! descriptions).

use crate::params;
use crate::Command;
use argus_daemon::{docs, MethodDoc, METHODS};
use std::fmt::Write;

/// Program usage plus every namespace and method, one line each.
pub fn overview() -> String {
    let mut out = String::new();
    out.push_str(
        "usage: argus <namespace> <method> [value] [key=value ...] [--json] [--follow]\n       argus <namespace>              list a namespace's methods\n       argus <namespace> <method> -h  one method's doc, params, returns\n       argus daemon <start|stop|status|run> [--scene s] [--seed n] [--window WxH] [--play] [--mods a,b]\n       argus editor                   the editor window on the project's daemon (started if none is running)\n       argus help [namespace | method]\n\nflags: --project <dir> (default: the nearest argus.toml upward from here, or $ARGUS_PROJECT)\n       --socket <name>   connect to this socket instead of the project's daemon\n       --no-start        fail if no daemon is running instead of starting one\n       --params <json>   a whole params object (or @file.json); key=value words layer on top\n       --json            print the result (or error) as one JSON line\n       --follow          keep the connection open and print notifications (obs.subscribe does this)\n\nvalues: JSON when they parse as JSON (2, true, \"text\", [1,2]); bare words are strings;\n        [a, b] and {k: v} accept bare words; dotted keys nest (where.has=[Enemy]);\n        a repeated key becomes a list; one bare value goes to the method's first parameter.\n\nmethods (each is `argus <ns> <name>`, or `argus <ns>.<name>`):\n",
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
    out.push_str("\n  test\n    test [name]            run the scenarios in tests/*.luau (test.run); --seeds 1..50, --file tests/x.luau, --update goldens\n");
    out.push_str(
        "\n  tools (what `argus mcp` serves an agent; each is `argus <tool> key=value ...`)\n",
    );
    for t in docs::TOOLS {
        let _ = writeln!(out, "    {:<22} {}", t.name, first_sentence(t.doc));
    }
    out.push_str(
        "    mcp                    serve the tools over MCP on stdio (for an agent host)\n",
    );
    out.push_str("    docs --for-agent       write the project's ARGUS.md from the live daemon\n");
    out.push_str("\n  build\n    build                  a release: the executable and its packs, or the web page (--target, --release)\n");
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
    if joined == "build" {
        return build();
    }
    if docs::tool(&joined).is_some() {
        return tool_topic(&joined);
    }
    if joined == "mcp" {
        return mcp();
    }
    if joined == "docs" {
        return docs_help();
    }
    if joined == "test" {
        return test();
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
        let _ = writeln!(out, "\nparams:  {}", docs::params_text(m.params));
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
    "usage: argus daemon start [--scene s] [--seed n] [--window [WxH]] [--play] [--mods a,b]\n                                                  start the project's daemon in the background\n       argus daemon stop                          ask it to exit (session.shutdown)\n       argus daemon status                        running or not, pid, socket\n       argus daemon run [the same flags]          serve in the foreground until stopped\n\n--window [WxH]: a window for a person to play in: the game plays at once, and closing the window\n                stops the daemon. --play: play without a window too.\n--mods a,b:     mods from the project's mods/ laid over the game, in that order.\n\nThe daemon is found through <project>/.argus/daemon.json. A method call starts one when none answers,\nunless --no-start is given.\n".to_string()
}

/// Help for an agent tool: its description (the same text the MCP server
/// gives), its parameters, and, for a tool that shares a namespace's name,
/// that namespace's methods.
pub fn tool_topic(name: &str) -> String {
    let Some(t) = docs::tool(name) else {
        return overview();
    };
    let mut out = format!("usage: argus {name} [key=value ...] [--params '<json>'] [--json]\n\n");
    out.push_str(&docs::tool_description(t));
    out.push('\n');
    if !t.params.is_empty() {
        let _ = writeln!(out, "\nparams:  {}", docs::params_text(t.params));
    }
    if METHODS
        .iter()
        .any(|m| m.name.split('.').next() == Some(name))
    {
        let _ = write!(out, "\n{}", namespace(name));
    }
    out
}

/// Help for `argus mcp`.
pub fn mcp() -> String {
    "usage: argus mcp [--project <dir>]\n\nServe the agent tools over the Model Context Protocol on stdin and stdout (section 16),\nfor an agent host: {\"command\": \"argus\", \"args\": [\"mcp\", \"--project\", \"<dir>\"]}. It finds or starts the\nproject's daemon, and on connect runs world.summary into its instructions. Tool descriptions and\ninput schemas come from the same table as this help; `argus help` lists the tools.\n".to_string()
}

/// Help for `argus editor`.
pub fn editor() -> String {
    "usage: argus editor [--project <dir>]\n\nThe editor window (section 11, phase 4): its own program, argus-editor, that reaches the game only\nthrough the protocol, as the agent does. It opens on the project's daemon, starting one if none is\nrunning. Everything done in it is a row in the change feed with the argus command that does it, by\n\"human\" (or $ARGUS_AUTHOR); Show as command lists those lines to copy.\n".to_string()
}

/// Help for `argus docs`.
pub fn docs_help() -> String {
    "usage: argus docs --for-agent [--out <file>]   the project's ARGUS.md, from the live daemon\n       argus docs                                 the engine's reference: the tools and every method\n\nARGUS.md (section 16) holds the component schemas with their docs, the systems with their stages\nand write sets, the prefab catalog, and each scene's summary, regenerated on demand so it never\ndrifts; a coding agent's project instructions point at it. --out defaults to <project>/ARGUS.md.\n".to_string()
}

/// Help for `argus build`.
pub fn build() -> String {
    "usage: argus build [--target windows|linux|macos|web] [--release] [--out dir] [--since dir] [--json]\n       argus build --mod <id> [--out dir] [--json]\n\nA release of the project (section 15): one directory with the executable, named for the game,\nand data/ with the game's pack and a pack per language (lang/<code>/ in the project). The web\ntarget writes the page and the wasm module instead of an executable. Packs are built from the\nimport cache with the scripts compiled; a failed import or a script error stops the build.\n\n--target: default this machine's; a desktop build is made on its own platform. web needs wasi-sdk\n          (WASI_SDK) and wasm-bindgen-cli.\n--release: optimised. --out: default <project>/dist/<target>.\n--since: a previous build's directory; the game goes into a patch with only what changed.\n\nThe build checks itself: the game from its packs, and the executable run headless, reach the\nsame world as the project after the same ticks. build.json in the directory says what was made.\nA player's report key saves a replay file; `argus mark import path=<file>` makes it a mark.\n\n--mod <id>: the pack of the mod in mods/<id>/ (phase 3 item 8), <id>.pack, for a player's mods\n          folder (the save location's mods/, in the order its load-order.toml declares). The game's\n          own pack never holds mods/. It checks the mod loads with the game, nothing left out, and\n          that the game plays the same with the mod from its pack as from its directory.\n          --out: default <project>/dist/mods.\n".to_string()
}

/// Help for `argus test`.
pub fn test() -> String {
    let m = docs::find("test.run").expect("test.run is a method");
    format!(
        "usage: argus test [name] [--seeds 1..50] [--file tests/x.luau] [--update] [--software] [--no-replays] [--json]\n\n{}\n\nname: exact or substring of a scenario name; none runs every scenario.\n--seeds: override the scenario's seed or range: 7, 1..50, or 1,2,9.\n--file: only that file's scenarios. --update: rewrite state goldens under tests/goldens/.\n--software: draw failing runs' clips with the software rasteriser, as CI does. --no-replays: save none.\nA failing run's replay opens with `argus replay open path=<its dir>`.\n--json: the test.run report as one line. Exit 0 when every run passed, 1 otherwise.\n\nsame as: test.run {{{}}}\n",
        m.doc, docs::params_text(m.params)
    )
}

/// Help for whatever the words resolved to.
pub fn for_command(cmd: &Command) -> String {
    match cmd {
        Command::Method(name, _) => method(docs::find(name).expect("resolved")),
        Command::Namespace(ns) => namespace(ns),
        Command::Daemon(_) => daemon(),
        Command::Editor => editor(),
        Command::Test(_) => test(),
        Command::Build => build(),
        Command::Tool(name, _) => tool_topic(name),
        Command::Mcp => mcp(),
        Command::Docs => docs_help(),
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
