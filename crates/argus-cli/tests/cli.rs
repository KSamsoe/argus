//! §5 "mirrored one-to-one by CLI subcommands"; §16 "one schema generates
//! the protocol, the CLI help, the MCP tool descriptions".

#[path = "../../argus-daemon/tests/common/mod.rs"]
mod common;

use argus_cli::{
    params, parse, render, resolve, run, Command, EXIT_CONNECT, EXIT_OK, EXIT_RPC, EXIT_USAGE,
};
use argus_daemon::{serve, socket_name_for, Transport, METHODS};
use serde_json::{json, Value as Json};
use std::path::Path;
use std::process::Command as Proc;

fn args(s: &[&str]) -> Vec<String> {
    s.iter().map(|s| s.to_string()).collect()
}

/// Run against a socket, capturing both streams.
fn cli(socket: &str, argv: &[&str]) -> (i32, String, String) {
    let mut full = vec!["--socket".to_string(), socket.to_string()];
    full.extend(args(argv));
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run(&full, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn every_method_is_a_subcommand_and_help_comes_from_the_same_table() {
    // Both spellings resolve, and nothing resolves that is not a method.
    for m in METHODS {
        let words: Vec<String> = m.name.split('.').map(str::to_string).collect();
        assert_eq!(
            resolve(&words),
            Command::Method(m.name, vec![]),
            "{}",
            m.name
        );
        assert_eq!(resolve(&args(&[m.name])), Command::Method(m.name, vec![]));
    }
    assert_eq!(
        resolve(&args(&["input", "log", "get", "since"])),
        Command::Method("input.log.get", args(&["since"])),
        "the longest join wins and the rest are bare values"
    );
    assert_eq!(
        resolve(&args(&["world"])),
        Command::Namespace("world".into())
    );
    assert_eq!(
        resolve(&args(&["world", "frobnicate"])),
        Command::Unknown(args(&["world", "frobnicate"]))
    );
    assert_eq!(resolve(&[]), Command::Help(vec![]));

    // The overview lists every method and nothing this crate invented.
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(run(&args(&["help"]), &mut out, &mut err), EXIT_OK);
    let text = String::from_utf8(out).unwrap();
    for m in METHODS {
        assert!(text.contains(m.name), "help lacks {}", m.name);
    }
    let listed: Vec<&str> = text
        .lines()
        .filter(|l| l.starts_with("    ") && !l.starts_with("     "))
        .filter_map(|l| l.split_whitespace().next())
        .filter(|w| w.contains('.'))
        .collect();
    assert_eq!(
        listed.len(),
        METHODS.len(),
        "one line per method, no extras"
    );

    // A method's help is its own doc, params, and returns.
    let (mut out, mut err) = (Vec::new(), Vec::new());
    run(&args(&["world", "get", "--help"]), &mut out, &mut err);
    let text = String::from_utf8(out).unwrap();
    let doc = argus_daemon::docs::find("world.get").unwrap();
    assert!(text.starts_with("usage: argus world get [entity] [key=value ...]"));
    assert!(text.contains(doc.doc));
    assert!(text.contains(&argus_daemon::docs::params_text(doc.params)));
    assert!(text.contains(doc.returns));

    // The tools (section 16) are commands too, with the MCP server's text.
    for t in argus_daemon::docs::TOOLS {
        if t.name == "test" {
            continue; // `argus test` runs test.run, which is the tool
        }
        assert_eq!(
            resolve(&args(&[t.name])),
            Command::Tool(t.name, vec![]),
            "{}",
            t.name
        );
        let (mut out, mut err) = (Vec::new(), Vec::new());
        run(&args(&[t.name, "--help"]), &mut out, &mut err);
        let text = String::from_utf8(out).unwrap();
        assert!(
            text.contains(&argus_daemon::docs::tool_description(t)),
            "{}: {text}",
            t.name
        );
    }
    assert_eq!(
        resolve(&args(&["edit", "begin"])),
        Command::Method("edit.begin", vec![]),
        "a method still wins over a tool of the same name"
    );
    let (mut out, mut err) = (Vec::new(), Vec::new());
    run(&args(&["help", "prof.alloc"]), &mut out, &mut err);
    assert!(String::from_utf8(out).unwrap().contains("counted"));

    // Usage errors say so and exit 2.
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(
        run(&args(&["world", "frobnicate"]), &mut out, &mut err),
        EXIT_USAGE
    );
    assert!(String::from_utf8(err).unwrap().contains("argus help"));
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(run(&args(&["--bogus"]), &mut out, &mut err), EXIT_USAGE);
}

#[test]
fn key_values_become_the_params_object() {
    let inv = parse(&args(&[
        "world",
        "query",
        "select=[id,Health.cur]",
        "where.has=[Enemy]",
        "where.within={of: @player, r: 6}",
        "limit=5",
        "--json",
        "--project",
        "x",
    ]))
    .unwrap();
    assert_eq!(inv.words, args(&["world", "query"]));
    assert!(inv.json);
    assert_eq!(inv.project.as_deref(), Some(Path::new("x")));
    let p = params::build("world.query", &inv.args, &inv.params_json).unwrap();
    assert_eq!(
        p,
        json!({
            "select": ["id", "Health.cur"],
            "where": { "has": ["Enemy"], "within": { "of": "@player", "r": 6 } },
            "limit": 5
        })
    );

    // A bare value goes to the first declared parameter; JSON literals stay typed.
    assert_eq!(
        params::names("world.get"),
        args(&["entity", "resolved", "scene", "source"])
    );
    assert_eq!(
        params::names("sim.run_until"),
        args(&["event", "time", "ticks", "max_ticks"]),
        "each of the alternatives is a parameter of its own"
    );
    assert_eq!(
        params::names("edit.spawn"),
        args(&["tx", "id", "room", "prefab", "tags", "components"])
    );
    assert_eq!(
        params::build("world.get", &args(&["@player", "resolved=true"]), &[]).unwrap(),
        json!({ "entity": "@player", "resolved": true })
    );
    assert_eq!(
        params::build("sim.step", &args(&["5"]), &[]).unwrap(),
        json!({ "n": 5 })
    );
    assert_eq!(
        params::build(
            "edit.begin",
            &args(&["author=human:kevin", "note=\"why not\""]),
            &[]
        )
        .unwrap(),
        json!({ "author": "human:kevin", "note": "why not" })
    );
    // A repeated key becomes a list; --params layers under the words.
    assert_eq!(
        params::build(
            "world.query",
            &args(&["select=id", "select=Health.cur", "limit=2"]),
            &["{\"limit\": 1, \"where\": {\"has\": [\"Enemy\"]}}".to_string()]
        )
        .unwrap(),
        json!({ "select": ["id", "Health.cur"], "limit": 2, "where": { "has": ["Enemy"] } })
    );
    // Too many bare values is a usage error with the method named.
    let e = params::build("sim.step", &args(&["5", "6"]), &[]).unwrap_err();
    assert!(e.contains("sim.step"));

    // Tables render aligned; other results are pretty JSON.
    let text = render::result(&json!({
        "total": 2, "next": null,
        "columns": ["id", "Health.cur"],
        "rows": [["@bat", 3], ["@player", 10.5]]
    }));
    assert_eq!(
        text,
        "next: \ntotal: 2\nid       Health.cur\n-------  ----------\n@bat     3\n@player  10.5\n"
    );
    assert_eq!(
        render::result(&json!({ "tick": 3 })),
        "{\n  \"tick\": 3\n}\n"
    );
    assert_eq!(
        render::error(
            &json!({ "code": 4704, "message": "no", "data": { "hint": "try", "ref": "a.toml:3" } })
        ),
        "error 4704: no\n  hint: try\n  at: a.toml:3\n"
    );
}

#[test]
fn the_cli_talks_to_a_daemon_over_its_socket() {
    let t = common::temp("cli");
    common::project(t.path());
    let name = format!("{}-cli", socket_name_for(t.path()));
    let root = t.path().to_path_buf();
    let name2 = name.clone();
    std::thread::spawn(move || {
        let daemon = common::open(&root);
        let _ = serve(daemon, Transport::Socket(name2), Some(&root));
    });
    let mut up = false;
    for _ in 0..100 {
        if argus_cli::client::Client::connect(&name).is_ok() {
            up = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(up);

    let (code, out, _) = cli(&name, &["world", "summary", "--json"]);
    assert_eq!(code, EXIT_OK);
    let v: Json = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["scene"], "arena");

    let (code, out, _) = cli(&name, &["world", "get", "@player", "--json"]);
    assert_eq!(code, EXIT_OK, "{out}");
    let v: Json = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["components"]["Health"]["cur"], 3, "the fixture's player");

    let (code, out, _) = cli(
        &name,
        &[
            "world",
            "query",
            "select=[id,Health.cur]",
            "where.has=[Enemy]",
            "order=-Health.cur",
        ],
    );
    assert_eq!(code, EXIT_OK);
    assert!(out.contains("total: "), "{out}");
    assert!(
        out.lines()
            .any(|l| l.starts_with("id ") && l.contains("Health.cur")),
        "{out}"
    );
    assert!(out.contains("@bat"), "{out}");

    // An error is code, sentence, hint, exit 1; --json gives the object.
    let (code, out, err) = cli(&name, &["world", "get"]);
    assert_eq!(code, EXIT_RPC);
    assert!(out.is_empty());
    assert!(err.starts_with("error 4704: "), "{err}");
    assert!(err.contains("hint: "), "{err}");
    let (code, out, _) = cli(&name, &["world", "get", "--json"]);
    assert_eq!(code, EXIT_RPC);
    assert_eq!(
        serde_json::from_str::<Json>(out.trim()).unwrap()["error"]["code"],
        4704
    );

    // Sim and edit round trips, spelled as a human would.
    let (code, out, _) = cli(&name, &["sim.step", "3", "--json"]);
    assert_eq!(code, EXIT_OK);
    assert_eq!(serde_json::from_str::<Json>(out.trim()).unwrap()["tick"], 3);
    let (_, out, _) = cli(
        &name,
        &["edit", "begin", "author=human:kevin", "note=cli", "--json"],
    );
    let tx = serde_json::from_str::<Json>(out.trim()).unwrap()["tx"].clone();
    let (code, _, err) = cli(
        &name,
        &[
            "edit",
            "set",
            &format!("tx={tx}"),
            "entity=@player",
            "path=Health.cur",
            "value=4",
        ],
    );
    assert_eq!(code, EXIT_OK, "{err}");
    let (code, out, _) = cli(&name, &["edit", "commit", &format!("tx={tx}"), "--json"]);
    assert_eq!(code, EXIT_OK);
    assert_eq!(
        serde_json::from_str::<Json>(out.trim()).unwrap()["note"],
        "cli"
    );
    let (_, out, _) = cli(&name, &["world", "get", "@player", "--json"]);
    assert_eq!(
        serde_json::from_str::<Json>(out.trim()).unwrap()["components"]["Health"]["cur"],
        4
    );

    // session.docs through the CLI is the same text as the CLI's own help source.
    let (code, out, _) = cli(&name, &["session", "docs", "world.query", "--json"]);
    assert_eq!(code, EXIT_OK);
    let text = serde_json::from_str::<Json>(out.trim()).unwrap()["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(text.starts_with("world.query\n"));
    assert!(text.contains(argus_daemon::docs::find("world.query").unwrap().doc));

    // No daemon on a socket: exit 3.
    let (code, _, err) = cli(&format!("{name}-nobody"), &["world", "summary"]);
    assert_eq!(code, EXIT_CONNECT);
    assert!(err.contains("cannot connect"));

    // argus test: the runner's report, compact.
    common::write(
        t.path(),
        "tests/bat.luau",
        "return scenario \"the bat dies\" { scene = \"arena\", seeds = range(1, 3), run = function(t)\n  t:run_until { event = \"death\", max_frames = 50 }\n  t.expect:field(\"@bat\", \"Health.cur\"):eq(0)\nend }",
    );
    common::write(
        t.path(),
        "tests/wrong.luau",
        "return scenario \"wrong\" { scene = \"arena\", seed = 4, run = function(t)\n  t.expect:count({ has = {\"Enemy\"} }):eq(0)\nend }",
    );
    let (code, out, err) = cli(&name, &["test"]);
    assert_eq!(code, EXIT_RPC, "{out}{err}");
    assert!(
        out.starts_with("PASS  the bat dies  (arena, 3 seeds 1..3: 3 ok, 0 failed, "),
        "{out}"
    );
    assert!(out.contains("FAIL  wrong  (arena, seed 4, "), "{out}");
    assert!(
        out.contains("    seed 4: count {has = [\"Enemy\"]}: 1 == 0 failed"),
        "{out}"
    );
    // The failing run saved its moment, and says how to open it (§9).
    assert!(
        out.contains("    seed 4: replay at frame 0") && out.contains("argus replay open path="),
        "{out}"
    );
    assert!(
        out.trim_end().ends_with("2 scenarios, 4 runs, 1 failed")
            || out.contains("2 scenarios, 4 runs, 1 failed, "),
        "{out}"
    );
    assert!(t
        .path()
        .join(".argus/replays/wrong/seed4/replay.bin")
        .is_file());
    let (code, out, _) = cli(&name, &["test", "wrong", "--no-replays"]);
    assert_eq!(code, EXIT_RPC);
    assert!(!out.contains("replay at frame"), "{out}");
    let (code, out, _) = cli(&name, &["test", "bat", "dies", "--seeds", "1..2", "--json"]);
    assert_eq!(code, EXIT_OK, "{out}");
    let rep: Json = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(rep["total_runs"], 2);
    assert_eq!(rep["scenarios"][0]["name"], "the bat dies");
    let (code, out, _) = cli(&name, &["test", "--help"]);
    assert_eq!(code, EXIT_OK);
    assert!(out.starts_with("usage: argus test [name]"));
    assert_eq!(
        resolve(&args(&["test", "run"])),
        Command::Method("test.run", vec![]),
        "the method spelling still works"
    );
    assert_eq!(
        resolve(&args(&["test", "wrong"])),
        Command::Test(args(&["wrong"]))
    );

    let (code, _, _) = cli(&name, &["session", "shutdown"]);
    assert_eq!(code, EXIT_OK);
}

/// Stops the daemon if the test dies part way.
struct StopOnDrop(std::path::PathBuf);
impl Drop for StopOnDrop {
    fn drop(&mut self) {
        let _ = Proc::new(env!("CARGO_BIN_EXE_argus"))
            .args(["--project"])
            .arg(&self.0)
            .args(["daemon", "stop"])
            .output();
    }
}

fn bin(root: &Path, argv: &[&str]) -> (i32, String, String) {
    let o = Proc::new(env!("CARGO_BIN_EXE_argus"))
        .arg("--project")
        .arg(root)
        .args(argv)
        .output()
        .unwrap();
    (
        o.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

#[test]
fn the_binary_finds_starts_and_stops_a_project_daemon() {
    let t = common::temp("cli-bin");
    common::project(t.path());
    let root = t.path().canonicalize().unwrap();
    let _guard = StopOnDrop(root.clone());

    let (code, out, _) = bin(&root, &["daemon", "status"]);
    assert_eq!(code, EXIT_CONNECT);
    assert_eq!(out.trim(), "not running");
    let (code, _, err) = bin(&root, &["world", "summary", "--no-start"]);
    assert_eq!(code, EXIT_CONNECT);
    assert!(err.contains("argus daemon start"), "{err}");

    // A method call starts the daemon, and says so on stderr.
    let (code, out, err) = bin(&root, &["world", "summary", "--json"]);
    assert_eq!(code, EXIT_OK, "{err}");
    assert!(err.contains("starting a daemon"), "{err}");
    assert_eq!(
        serde_json::from_str::<Json>(out.trim()).unwrap()["scene"],
        "arena"
    );
    let (code, out, _) = bin(&root, &["daemon", "status", "--json"]);
    assert_eq!(code, EXIT_OK);
    let st: Json = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(st["running"], true);
    let pid = st["daemon"]["pid"].as_u64().unwrap();

    // The second call reuses it; start reports it as already running.
    let (code, _, err) = bin(&root, &["sim", "step", "2"]);
    assert_eq!(code, EXIT_OK, "{err}");
    assert!(!err.contains("starting"), "{err}");
    let (code, out, _) = bin(&root, &["daemon", "start"]);
    assert_eq!(code, EXIT_OK);
    assert!(
        out.contains(&format!("already running: pid {pid}")),
        "{out}"
    );
    let (code, out, err) = bin(&root, &["sim", "status", "--json"]);
    assert_eq!(code, EXIT_OK, "{err}");
    assert_eq!(
        serde_json::from_str::<Json>(out.trim()).unwrap()["tick"],
        2,
        "same daemon, same world"
    );

    // Stop: the record goes away and status says so.
    let (code, out, _) = bin(&root, &["daemon", "stop", "--json"]);
    assert_eq!(code, EXIT_OK);
    assert_eq!(
        serde_json::from_str::<Json>(out.trim()).unwrap()["shutdown"],
        true
    );
    // `stop` returns once the daemon has gone, not when it agreed to: a
    // command right after it used to reach the one on its way out and fail
    // with "connection lost" (found after the phase-1 gate).
    assert!(
        !root.join(".argus").join("daemon.json").exists(),
        "daemon.json removed before stop returns"
    );
    let (code, out, _) = bin(&root, &["daemon", "status"]);
    assert_eq!(code, EXIT_CONNECT);
    assert_eq!(out.trim(), "not running");
    assert!(root.join(".argus").join("daemon.log").exists());
    let (code, _, err) = bin(&root, &["world", "summary"]);
    assert_eq!(
        code, EXIT_OK,
        "a call right after stop starts a new daemon: {err}"
    );
    assert!(err.contains("starting a daemon"), "{err}");
    let (code, _, _) = bin(&root, &["daemon", "stop"]);
    assert_eq!(code, EXIT_OK);
}

/// A semantic frame reads as lines, in the frame's order: what a narrator
/// would say.
#[test]
fn a_semantic_frame_reads_as_lines() {
    let text = render::result(&json!({
        "format": "semantic", "tick": 7, "size": [640, 360],
        "camera": { "id": "@camera", "rect": [0, 0, 16, 9], "angle": 0 },
        "hud": [{ "id": "@hud", "text": "Coins 3" }],
        "columns": ["id", "name", "text", "bounds", "z", "look"],
        "entities": [["@player", "player", "HP 3", [1, 2, 3, 4], 5, "rect #FFFFFF"], ["e:9", "", "", [5, 6, 7, 8], 0, "circle"]],
        "groups": [{ "what": "Coin", "count": 5, "bounds": [0, 0, 9, 9], "look": "circle #FFD866", "ids": [] }],
        "effects": [{ "kind": "shake", "camera": "@camera", "trauma": 0.5 }],
        "omitted": 0, "problems": [],
        "tiles": { "key": "# solid", "maps": [{ "entity": "@floor", "map": "m", "cell": [0, 0], "rows": ["..##"] }] },
        "bytes": 800
    }));
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines[0],
        "Tick 7, 640x360 px. Camera @camera shows x 0 to 16, y 0 to 9."
    );
    assert_eq!(lines[1], "HUD @hud: Coins 3");
    assert_eq!(
        lines[2],
        "@player player: HP 3  at (1,2)-(3,4)  rect #FFFFFF"
    );
    assert_eq!(lines[3], "e:9  at (5,6)-(7,8)  circle");
    assert_eq!(lines[4], "Coin x5  in (0,0)-(9,9)  circle #FFD866");
    assert_eq!(lines[5], "Camera @camera shakes, trauma 0.5");
    assert!(
        lines[6].starts_with("Tiles of @floor (m) from cell (0,0)"),
        "{}",
        lines[6]
    );
    assert_eq!(lines[7], "  ..##");
    assert_eq!(lines[8], "(800 bytes)");
}
