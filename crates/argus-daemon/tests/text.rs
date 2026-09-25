//! Strings and dialogue through the protocol (phase 3 item 4, §12): a
//! language's table beside the base's and what it lacks (strings.list), a
//! string changed in a transaction and taken back (edit.text), the dialogue
//! table in world.defs, and rows that do not read in script.errors.

mod common;

use common::*;
use serde_json::json;

fn words(root: &std::path::Path) {
    write(
        root,
        "strings/en.toml",
        "language.name = \"English\"\n\n[menu]\nresume = \"Resume\" # the first button\nquit = \"Quit\"\n",
    );
    write(
        root,
        "strings/fr.toml",
        "language.name = \"Français\"\nmenu.resume = \"Reprendre\"\n",
    );
    write(
        root,
        "defs/dialogue.toml",
        "[\"guard.hi\"]\nspeaker = \"guard.name\"\nline = \"guard.hi\"\nnext = \"dialogue:guard.bye\"\n\n[\"guard.bye\"]\nline = \"guard.bye\"\n",
    );
}

#[test]
fn strings_are_listed_changed_in_a_transaction_and_taken_back() {
    let t = temp("text");
    project(t.path());
    words(t.path());
    let mut d = open(t.path());
    let l = ok(&mut d, "strings.list", json!({ "lang": "fr" }));
    assert_eq!(l["base"], "en");
    assert_eq!(l["columns"], json!(["key", "en", "fr"]));
    assert_eq!(
        l["rows"],
        json!([
            ["language.name", "English", "Français"],
            ["menu.quit", "Quit", null],
            ["menu.resume", "Resume", "Reprendre"]
        ])
    );
    assert_eq!(
        l["languages"],
        json!([
            { "code": "en", "name": "English", "strings": 3, "missing": 0 },
            { "code": "fr", "name": "Français", "strings": 2, "missing": 1 }
        ])
    );
    let m = ok(
        &mut d,
        "strings.list",
        json!({ "lang": "fr", "missing": true }),
    );
    assert_eq!(m["rows"], json!([["menu.quit", "Quit", null]]));

    // A translator's word, in a transaction by an author: kept in the file,
    // in the feed, and undone.
    let tx = ok(
        &mut d,
        "edit.begin",
        json!({ "author": "human:translator", "note": "quit" }),
    )["tx"]
        .clone();
    ok(
        &mut d,
        "edit.text",
        json!({ "tx": tx, "lang": "fr", "key": "menu.quit", "text": "Quitter" }),
    );
    ok(
        &mut d,
        "edit.text",
        json!({ "tx": tx, "lang": "en", "key": "menu.resume", "text": "Go on" }),
    );
    ok(&mut d, "edit.commit", json!({ "tx": tx }));
    let fr = std::fs::read_to_string(t.path().join("strings/fr.toml")).unwrap();
    assert!(fr.contains("Quitter"), "{fr}");
    let en = std::fs::read_to_string(t.path().join("strings/en.toml")).unwrap();
    assert!(
        en.contains("resume = \"Go on\" # the first button"),
        "the comment stays: {en}"
    );
    let m = ok(
        &mut d,
        "strings.list",
        json!({ "lang": "fr", "missing": true }),
    );
    assert_eq!(m["total"], 0);
    let feed = ok(&mut d, "edit.feed", json!({}));
    assert!(feed.to_string().contains("\"text\""), "{feed}");
    ok(&mut d, "edit.undo", json!({ "author": "human:translator" }));
    let l = ok(&mut d, "strings.list", json!({ "lang": "fr" }));
    assert_eq!(l["rows"][1], json!(["menu.quit", "Quit", null]));
    assert_eq!(l["rows"][2], json!(["menu.resume", "Resume", "Reprendre"]));
    let e = err(
        &mut d,
        "edit.text",
        json!({ "tx": 1, "lang": "../x", "key": "a", "text": "b" }),
    );
    assert_eq!(e.code, argus_daemon::codes::BAD_PARAMS);

    // The dialogue table is a def table like any other; its row that does
    // not read is listed with the scripts' errors, with its line.
    let defs = ok(&mut d, "world.defs", json!({ "table": "dialogue" }));
    assert_eq!(defs["total"], 2);
    assert_eq!(defs["columns"][0], "id");
    assert_eq!(ok(&mut d, "script.errors", json!({}))["total"], 0);
    write(
        t.path(),
        "defs/dialogue.toml",
        "[\"guard.hi\"]\nspeaker = \"guard.name\"\nline = \"guard.hi\"\nnext = \"dialogue:guard.bye\"\n\n[\"guard.bye\"]\nline = \"guard.bye\"\nwhen = \"Health.cur >\"\n",
    );
    ok(&mut d, "script.reload", json!({}));
    let errors = ok(&mut d, "script.errors", json!({}));
    let rows = errors["rows"].as_array().unwrap();
    let bad = rows
        .iter()
        .find(|r| r["phase"] == "dialogue")
        .unwrap_or_else(|| panic!("{errors}"));
    assert_eq!(bad["file"], "defs/dialogue.toml");
    assert!(
        bad["message"].as_str().unwrap().contains("guard.bye: when"),
        "{bad}"
    );
}
