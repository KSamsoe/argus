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
    std::thread::spawn(move || {
        let daemon = open(&root);
        let _ = serve(daemon, Transport::Socket(name2), Some(&root));
    });

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
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap()
}
