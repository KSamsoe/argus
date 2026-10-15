//! Reaching players item 0: undo names one of the author's commits, not
//! only the latest, and is refused, naming them, while a later change
//! still in effect changed the same thing.

mod common;

use argus_daemon::codes;
use common::*;
use serde_json::{json, Value as Json};

fn get(d: &mut argus_daemon::Daemon, entity: &str, path: &str) -> Json {
    let g = ok(d, "world.get", json!({ "entity": entity }));
    let (c, f) = path.split_once('.').unwrap();
    g["components"][c][f].clone()
}

fn set(
    d: &mut argus_daemon::Daemon,
    author: &str,
    note: &str,
    entity: &str,
    path: &str,
    v: Json,
) -> u64 {
    let r = commit(
        d,
        author,
        note,
        &[(
            "edit.set",
            json!({ "entity": entity, "path": path, "value": v }),
        )],
    );
    r["id"].as_u64().unwrap()
}

#[test]
fn a_named_edit_is_undone_unless_a_later_one_changed_the_same_thing() {
    let t = temp("undo-named");
    project(t.path());
    let mut d = open(t.path());
    let a = "agent:claude";
    let (max0, bat0) = (
        get(&mut d, "@player", "Health.max"),
        get(&mut d, "@bat", "Health.max"),
    );
    let max = set(
        &mut d,
        a,
        "a tougher hero",
        "@player",
        "Health.max",
        json!(17),
    );
    let bat = set(&mut d, a, "a tougher bat", "@bat", "Health.max", json!(9));
    let cur = set(
        &mut d,
        a,
        "the hero hurt",
        "@player",
        "Health.cur",
        json!(2),
    );

    // The first, by name: the later two changed other fields.
    let u = ok(&mut d, "edit.undo", json!({ "author": a, "tx": max }));
    assert_eq!(u["kind"], json!({ "undo": max }));
    assert_eq!(get(&mut d, "@player", "Health.max"), max0);
    assert_eq!(
        get(&mut d, "@player", "Health.cur"),
        json!(2),
        "the later edit stays"
    );
    // Undone, it is not the author's to undo again; nor is a commit that
    // is not there, or another author's.
    let e = err(&mut d, "edit.undo", json!({ "author": a, "tx": max }));
    assert_eq!(e.code, codes::NOTHING_TO_UNDO);
    assert!(e.message.contains("undone already"), "{}", e.message);
    let e = err(&mut d, "edit.undo", json!({ "author": a, "tx": 999 }));
    assert!(e.message.contains("no commit #999"), "{}", e.message);
    // Plain undo is still the latest left: the hurt hero.
    let u = ok(&mut d, "edit.undo", json!({ "author": a }));
    assert_eq!(u["kind"], json!({ "undo": cur }));

    // Someone else changes the bat's maximum: the agent's bat edit cannot
    // be undone alone, and the error says whose change is in the way.
    let mine = set(
        &mut d,
        "human",
        "the bat at 4",
        "@bat",
        "Health.max",
        json!(4),
    );
    let e = err(&mut d, "edit.undo", json!({ "author": "human", "tx": bat }));
    assert!(e.message.contains(&format!("is {a}'s")), "{}", e.message);
    let e = err(&mut d, "edit.undo", json!({ "author": a, "tx": bat }));
    assert_eq!(e.code, codes::UNDO_CONFLICT);
    assert!(
        e.message
            .contains(&format!("#{mine} by human (the bat at 4)")),
        "{}",
        e.message
    );
    assert_eq!(
        get(&mut d, "@bat", "Health.max"),
        json!(4),
        "nothing changed"
    );
    // Once that change is undone, the agent's goes.
    ok(&mut d, "edit.undo", json!({ "author": "human" }));
    ok(&mut d, "edit.undo", json!({ "author": a, "tx": bat }));
    assert_eq!(get(&mut d, "@bat", "Health.max"), bat0);
    // A component's removal meets an edit of its field.
    let again = set(&mut d, a, "tough again", "@player", "Health.max", json!(12));
    commit(
        &mut d,
        "human",
        "no health bar",
        &[(
            "edit.remove",
            json!({ "entity": "@player", "component": "Health" }),
        )],
    );
    let e = err(&mut d, "edit.undo", json!({ "author": a, "tx": again }));
    assert_eq!(e.code, codes::UNDO_CONFLICT, "{}", e.message);
}
