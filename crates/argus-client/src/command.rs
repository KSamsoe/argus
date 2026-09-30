//! "Show as command" (§11): any call written as the `argus` line that
//! makes it, "so the human learns the agent's language by using the
//! editor and can paste a command into a chat instead of describing a
//! click". The change feed carries the same line for every change (phase
//! 4), so whatever a person did is something the agent could have typed.
//!
//! The line is for a POSIX shell (bash, Git Bash on Windows): a value that
//! needs it is single-quoted. [`read_line`] is its inverse, and
//! `read_line(&line(t, p)) == (t, p)` for every params object.

use crate::words;
use serde_json::{Map, Value as Json};

/// The `argus` line for a method (`sim.speed`) or an agent tool (`edit`)
/// with these params: `argus sim speed value=10`. Keys are written in the
/// params' order; nested objects as dotted keys where their keys allow,
/// else as literals; lists and objects in the CLI's bracket form when it
/// reads back the same, else as JSON.
pub fn line(target: &str, params: &Json) -> String {
    let mut out = String::from("argus");
    let (ns, rest) = match target.split_once('.') {
        Some((a, b)) => (a, Some(b)),
        None => (target, None),
    };
    out.push(' ');
    out.push_str(ns);
    if let Some(r) = rest {
        out.push(' ');
        out.push_str(r);
    }
    let mut odd = Map::new();
    if let Json::Object(m) = params {
        let mut ws = Vec::new();
        for (k, v) in m {
            if words::is_key(k) && !k.contains('.') {
                kv_words(k, v, &mut ws);
            } else {
                odd.insert(k.clone(), v.clone());
            }
        }
        for w in ws {
            out.push(' ');
            out.push_str(&w);
        }
    }
    if !odd.is_empty() {
        // Keys the words cannot carry go whole in --params.
        out.push_str(" --params ");
        out.push_str(&shell_quote(&Json::Object(odd).to_string()));
    }
    out
}

/// Words for one key: an object with plain keys opens into dotted keys,
/// anything else is `key=value`.
fn kv_words(key: &str, v: &Json, out: &mut Vec<String>) {
    if let Json::Object(m) = v {
        let plain = !m.is_empty() && m.keys().all(|k| words::is_key(k) && !k.contains('.'));
        if plain {
            for (k, sub) in m {
                kv_words(&format!("{key}.{k}"), sub, out);
            }
            return;
        }
    }
    let text = value_text(v);
    out.push(format!("{key}={}", shell_quote(&text)));
}

/// A value as the CLI reads it back to the same JSON.
pub fn value_text(v: &Json) -> String {
    match v {
        Json::String(s) => {
            // Bare when the word reads back as this string: not JSON of
            // its own ("10", "true", "\"x\""), not a bracket literal.
            if words::value(s).ok().as_ref() == Some(v) {
                s.clone()
            } else {
                v.to_string()
            }
        }
        Json::Array(_) | Json::Object(_) => {
            let relaxed = relaxed(v);
            if words::value(&relaxed).ok().as_ref() == Some(v) {
                relaxed
            } else {
                v.to_string()
            }
        }
        _ => v.to_string(),
    }
}

/// The bracket form: `[Enemy, @player]`, `{op: set, value: 3.4}`.
fn relaxed(v: &Json) -> String {
    match v {
        Json::String(s) => {
            if bare_in_brackets(s) {
                s.clone()
            } else {
                v.to_string()
            }
        }
        Json::Array(a) => {
            let items: Vec<String> = a.iter().map(relaxed).collect();
            format!("[{}]", items.join(", "))
        }
        Json::Object(m) => {
            let items: Vec<String> = m
                .iter()
                .map(|(k, x)| {
                    let key = if bare_in_brackets(k) {
                        k.clone()
                    } else {
                        Json::String(k.clone()).to_string()
                    };
                    format!("{key}: {}", relaxed(x))
                })
                .collect();
            format!("{{{}}}", items.join(", "))
        }
        _ => v.to_string(),
    }
}

/// A string that stays one bare word inside brackets and reads back as
/// itself: word characters, and colons between two of them (a def row,
/// `item:meal`, as the reader keeps a glued colon in the word).
fn bare_in_brackets(s: &str) -> bool {
    let cs: Vec<char> = s.chars().collect();
    !cs.is_empty()
        && !words::is_literal_word(s)
        && cs.iter().enumerate().all(|(i, c)| {
            words::is_bare_start(*c)
                || (*c == ':'
                    && i > 0
                    && i + 1 < cs.len()
                    && words::is_bare_start(cs[i - 1])
                    && words::is_bare_start(cs[i + 1]))
        })
}

/// A word for a POSIX shell: as it is when every character is safe there,
/// else single-quoted.
pub fn shell_quote(s: &str) -> String {
    let safe = !s.is_empty()
        && s.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(c, '@' | '%' | '+' | '=' | ':' | ',' | '.' | '/' | '_' | '-')
        });
    if safe {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

/// Split a shell line into words, as a POSIX shell does for these lines:
/// whitespace between words, single quotes literal, double quotes with
/// backslash escapes, a backslash outside quotes escaping the next
/// character.
pub fn split(line: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut cs = line.chars();
    while let Some(c) = cs.next() {
        match c {
            '\'' => {
                in_word = true;
                loop {
                    match cs.next() {
                        Some('\'') => break,
                        Some(d) => cur.push(d),
                        None => return Err("a single quote is not closed".into()),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match cs.next() {
                        Some('"') => break,
                        Some('\\') => match cs.next() {
                            Some(d @ ('"' | '\\' | '$' | '`')) => cur.push(d),
                            Some(d) => {
                                cur.push('\\');
                                cur.push(d);
                            }
                            None => return Err("a double quote is not closed".into()),
                        },
                        Some(d) => cur.push(d),
                        None => return Err("a double quote is not closed".into()),
                    }
                }
            }
            '\\' => {
                in_word = true;
                if let Some(d) = cs.next() {
                    cur.push(d);
                }
            }
            c if c.is_whitespace() => {
                if in_word {
                    out.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            c => {
                in_word = true;
                cur.push(c);
            }
        }
    }
    if in_word {
        out.push(cur);
    }
    Ok(out)
}

/// Read an `argus` line back into its target and params: the words before
/// the first `key=value` or flag name the method (joined with `.`) or the
/// tool; `--params` objects go under the words. Bare values are refused,
/// as [`words::read`] refuses them.
pub fn read_line(line: &str) -> Result<(String, Json), String> {
    let ws = split(line)?;
    let mut it = ws.into_iter().peekable();
    if it.peek().map(String::as_str) == Some("argus") {
        it.next();
    }
    let mut target = Vec::new();
    while let Some(w) = it.peek() {
        if w.starts_with("--") || words::split_kv(w).is_some() {
            break;
        }
        target.push(it.next().unwrap_or_default());
    }
    if target.is_empty() {
        return Err("no method or tool in the line".into());
    }
    let mut kv = Vec::new();
    let mut base = Map::new();
    while let Some(w) = it.next() {
        if w == "--params" {
            let text = it.next().ok_or("--params needs a JSON object")?;
            match serde_json::from_str::<Json>(&text) {
                Ok(Json::Object(m)) => base.extend(m),
                _ => return Err("--params must be a JSON object".into()),
            }
        } else if w.starts_with("--") {
            return Err(format!("{w} is a flag, not part of a call"));
        } else {
            kv.push(w);
        }
    }
    let Json::Object(m) = words::read(&kv)? else {
        unreachable!("words::read makes an object")
    };
    base.extend(m);
    Ok((target.join("."), Json::Object(base)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn round_trip(target: &str, params: Json) {
        let l = line(target, &params);
        let back = read_line(&l).unwrap_or_else(|e| panic!("{l}: {e}"));
        assert_eq!(back, (target.to_string(), params.clone()), "{l}");
    }

    #[test]
    fn calls_are_written_as_the_cli_types_them() {
        assert_eq!(line("sim.play", &json!({})), "argus sim play");
        assert_eq!(
            line("sim.speed", &json!({"value": 10})),
            "argus sim speed value=10"
        );
        assert_eq!(
            line(
                "edit.set",
                &json!({"tx": 3, "entity": "@spawner.north", "path": "WaveSpawner.interval_s", "value": 0.28})
            ),
            "argus edit set entity=@spawner.north path=WaveSpawner.interval_s tx=3 value=0.28"
        );
        assert_eq!(
            line(
                "world.query",
                &json!({"where": {"has": ["Enemy"]}, "limit": 5})
            ),
            "argus world query limit=5 where.has='[Enemy]'"
        );
        assert_eq!(
            line("mark.note", &json!({"id": 2, "note": "dash feels late"})),
            "argus mark note id=2 note='dash feels late'"
        );
        assert_eq!(line("scene.tiles.get", &json!({})), "argus scene tiles.get");
        assert_eq!(
            line("edit", &json!({"author": "human", "note": "brighter", "ops": [{"op": "set", "entity": "@lamp", "path": "Light.r", "value": 2}]})),
            "argus edit author=human note=brighter ops='[{entity: @lamp, op: set, path: Light.r, value: 2}]'"
        );
    }

    #[test]
    fn every_value_reads_back_the_same() {
        round_trip("sim.play", json!({}));
        round_trip("sim.speed", json!({"value": 10}));
        round_trip("sim.speed", json!({"value": 0.1}));
        round_trip(
            "tune.set",
            json!({"entity": "@colony", "path": "Colony.hunger_per_day", "value": 1.35, "author": "human"}),
        );
        round_trip(
            "edit.text",
            json!({"tx": 1, "lang": "fr", "key": "menu.resume", "text": "Reprendre l'aventure"}),
        );
        round_trip(
            "edit.text",
            json!({"tx": 1, "lang": "fr", "key": "k", "text": null}),
        );
        round_trip("mark.note", json!({"id": 1, "note": ""}));
        round_trip(
            "mark.note",
            json!({"id": 1, "note": "  spaces at both ends  "}),
        );
        round_trip("mark.note", json!({"id": 1, "note": "10"}));
        round_trip("mark.note", json!({"id": 1, "note": "true"}));
        round_trip("mark.note", json!({"id": 1, "note": "null"}));
        round_trip("mark.note", json!({"id": 1, "note": "\"quoted\""}));
        round_trip("mark.note", json!({"id": 1, "note": "[not a list"}));
        round_trip("mark.note", json!({"id": 1, "note": "{brace"}));
        round_trip(
            "mark.note",
            json!({"id": 1, "note": "it's $HOME and `ls` and \\ back"}),
        );
        round_trip(
            "mark.note",
            json!({"id": 1, "note": "line one\nline two\ttab"}),
        );
        round_trip("mark.note", json!({"id": 1, "note": "inf"}));
        round_trip("mark.note", json!({"id": 1, "note": "é, ñ, 漢字"}));
        round_trip(
            "world.query",
            json!({"select": ["id", "Transform.pos", "sum(Stockpile.food)"], "where": {"has": ["Enemy"], "within": {"of": "@player", "r": 6.0}}, "order": "dist(@player)", "limit": 5}),
        );
        round_trip("world.query", json!({"where": {}}));
        round_trip("x.y", json!({"list": []}));
        round_trip(
            "x.y",
            json!({"list": [[1, 2], [], {"a": null}], "n": -3, "f": 1e-7, "b": false}),
        );
        round_trip(
            "x.y",
            json!({"odd key": 1, "Colony.hunger_per_day": 2, "fine": "a b"}),
        );
        round_trip(
            "x.y",
            json!({"set": {"Colony.hunger_per_day": 1.2, "Colony.crop_yield": 6}}),
        );
        round_trip(
            "x.y",
            json!({"words": ["inf", "true", "1", "a b", "item:key.brass", "count()", "", "-", "#x"]}),
        );
        round_trip("x.y", json!({"deep": {"a": {"b": {"c": "d"}}, "e": [1]}}));
        round_trip(
            "edit",
            json!({"ops": [{"op": "def", "def": "recipe:meal", "path": "work", "value": 4}, {"k": "a::b", "j": ":x", "i": "y:"}]}),
        );
        assert_eq!(
            line(
                "edit",
                &json!({"ops": [{"op": "def", "def": "item:meal", "path": "food", "value": 0.8}]})
            ),
            "argus edit ops='[{def: item:meal, op: def, path: food, value: 0.8}]'"
        );
        round_trip(
            "edit",
            json!({"author": "human", "note": "paint", "ops": [{"op": "tiles", "file": "scenes/a/b.tilemap.toml", "layer": "ground", "rect": [0, 0, 2, 1], "cells": [1, 2]}]}),
        );
    }

    #[test]
    fn a_shell_splits_the_words_as_written() {
        assert_eq!(
            split(r#"argus mark note 'note=it'\''s' x="a \"b\"" c\ d"#).unwrap(),
            vec!["argus", "mark", "note", "note=it's", "x=a \"b\"", "c d"]
        );
        assert!(split("argus 'open").is_err());
    }
}
