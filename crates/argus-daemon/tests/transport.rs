//! The local socket transport: a client connects, calls, subscribes, and
//! receives a server-initiated notification (§2, §5).

mod common;

use argus_daemon::{serve, server, Transport};
use common::*;
use interprocess::local_socket::prelude::*;
use serde_json::{json, Value as Json};
use std::io::{BufRead, BufReader, Write};

#[test]
fn a_client_talks_to_the_daemon_over_the_local_socket() {
    let t = temp("socket");
    project(t.path());
    let name = format!("{}-test", server::socket_name_for(t.path()));
    let root = t.path().to_path_buf();
    let name2 = name.clone();
    // The daemon lives on its own thread (the world and the Luau VM are
    // not Send); the server owns it there.
    let thread = std::thread::spawn(move || {
        let daemon = open(&root);
        let _ = serve(daemon, Transport::Socket(name2), Some(&root));
    });
    let _daemon = Served::new(&name, thread);

    // Connect (retry while the listener comes up).
    let mut stream = None;
    for _ in 0..50 {
        if let Ok(s) = server::connect(&name) {
            stream = Some(s);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let stream = stream.expect("connect to argusd");
    let (recv, mut send) = stream.split();
    let mut reader = BufReader::new(recv);

    let r = call(&mut send, &mut reader, 1, "world.summary", json!({}));
    assert_eq!(r["id"], 1);
    assert_eq!(r["result"]["scene"], "arena");
    let r = call(
        &mut send,
        &mut reader,
        2,
        "obs.subscribe",
        json!({ "topics": ["edits", "events"] }),
    );
    assert_eq!(r["result"]["topics"], json!(["edits", "events"]));
    let r = call(
        &mut send,
        &mut reader,
        3,
        "edit.begin",
        json!({ "author": "agent:claude", "note": "over the wire" }),
    );
    let tx = r["result"]["tx"].clone();
    call(
        &mut send,
        &mut reader,
        4,
        "edit.set",
        json!({ "tx": tx, "entity": "@player", "path": "Health.cur", "value": 2 }),
    );
    let r = call(
        &mut send,
        &mut reader,
        5,
        "edit.commit",
        json!({ "tx": tx }),
    );
    assert_eq!(r["result"]["note"], "over the wire");
    // The commit notification follows on the same connection.
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let n: Json = serde_json::from_str(&line).unwrap();
    assert_eq!(n["method"], "edit.committed");
    assert_eq!(n["params"]["touched"], json!(["@player"]));
    assert!(n.get("id").is_none());
    // Stepping the sim streams events.
    let r = call(&mut send, &mut reader, 6, "sim.step", json!({ "n": 2 }));
    assert_eq!(r["result"]["tick"], 2);
    let mut got_death = false;
    for _ in 0..10 {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let n: Json = serde_json::from_str(&line).unwrap();
        if n["method"] == "event" && n["params"]["name"] == "death" {
            got_death = true;
            break;
        }
    }
    assert!(got_death);
    let r = call(
        &mut send,
        &mut reader,
        7,
        "world.get",
        json!({ "entity": "@player" }),
    );
    assert_eq!(r["result"]["components"]["Health"]["cur"], 2);
    assert!(read(t.path(), "scenes/arena/player.toml").contains("cur = 2"));
    let info = read(t.path(), ".argus/daemon.json");
    assert!(info.contains(&name), "{info}");
}

fn call(
    send: &mut impl Write,
    reader: &mut impl BufRead,
    id: u64,
    method: &str,
    params: Json,
) -> Json {
    let req = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
    send.write_all(req.to_string().as_bytes()).unwrap();
    send.write_all(
        b"
",
    )
    .unwrap();
    send.flush().unwrap();
    // The reply, past any notification still on the line (a change's feed
    // row, phase 4), as a client sets them aside.
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let msg: Json = serde_json::from_str(&line).unwrap();
        if msg.get("id").is_some() {
            return msg;
        }
    }
}

/// Issue 28: `argus daemon stop` (session.shutdown) stops a scenario whose
/// own code never ends, which held the daemon's thread so the shutdown
/// waited behind it for ever; the test.run answers, stopped, and the
/// daemon then shuts down.
#[test]
fn a_shutdown_stops_a_scenario_that_never_ends() {
    let t = temp("socket-stop");
    project(t.path());
    write(
        t.path(),
        "tests/forever.luau",
        "return scenario \"forever\" { scene = \"arena\", seed = 7, run = function(t) while true do end end }",
    );
    let name = format!("{}-stop", server::socket_name_for(t.path()));
    let root = t.path().to_path_buf();
    let name2 = name.clone();
    let thread = std::thread::spawn(move || {
        let daemon = open(&root);
        let _ = serve(daemon, Transport::Socket(name2), Some(&root));
    });
    let _daemon = Served::new(&name, thread);
    let connect = || {
        for _ in 0..50 {
            if let Ok(s) = server::connect(&name) {
                return s;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        panic!("connect to argusd");
    };
    let (recv, mut send) = connect().split();
    let mut reader = BufReader::new(recv);
    let started = std::time::Instant::now();
    // The run, on its own thread: it answers only once the scenario stops.
    let runner = std::thread::spawn(move || call(&mut send, &mut reader, 1, "test.run", json!({})));
    std::thread::sleep(std::time::Duration::from_millis(500));
    let (_r2, mut send2) = connect().split();
    let stop = json!({ "jsonrpc": "2.0", "id": 9, "method": "session.shutdown" });
    writeln!(send2, "{stop}").unwrap();
    send2.flush().unwrap();
    let r = runner.join().unwrap();
    assert!(started.elapsed().as_secs() < 20, "{:?}", started.elapsed());
    assert_eq!(r["result"]["stopped"], true, "{r}");
    let said = r["result"]["scenarios"][0]["seeds"][0]["error"]
        .as_str()
        .unwrap_or("");
    assert!(
        said.contains("daemon stop") && said.contains("tests/forever.luau"),
        "{said}"
    );
}
